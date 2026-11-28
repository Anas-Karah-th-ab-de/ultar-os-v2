//! Signale nach Linux-Art: Handler (sigaction), Masken, Zustellung über einen Rahmen auf dem Benutzerstapel
//! (rt_sigframe mit ucontext/siginfo/FPU-Zustand), rt_sigreturn, unterbrochene Systemaufrufe (EINTR/SA_RESTART).

use crate::proc::{self, Proc, State, Wait, PROCS};
use crate::trap::TrapFrame;
use alloc::boxed::Box;
use alloc::vec::Vec;

pub const SIGINT: u64 = 2;
pub const SIGKILL: u64 = 9;
pub const SIGSEGV: u64 = 11;
pub const SIGPIPE: u64 = 13;
#[allow(dead_code)] // nothing uses it yet; kept, see docs/OFFEN-WARNUNGEN.md
pub const SIGTERM: u64 = 15;
pub const SIGCHLD: u64 = 17;
const SA_ONSTACK: u64 = 0x0800_0000;
const SA_RESTART: u64 = 0x1000_0000;
const SA_NODEFER: u64 = 0x4000_0000;
const SA_RESETHAND: u64 = 0x8000_0000;

fn bit(sig: u64) -> u64 {
    1u64 << (sig - 1)
}

/// Standardverhalten: ignorieren (true) oder beenden (false)
fn default_ignores(sig: u64) -> bool {
    matches!(sig, 17 | 18 | 19 | 20 | 21 | 22 | 23 | 28)
}

/// Signal an einen Faden (`tid`) oder eine Gruppe (`tgid`, tid = 0) senden. Sperre `PROCS` wird gehalten.
pub fn send_locked(ps: &mut Vec<Box<Proc>>, tgid: u64, tid: u64, sig: u64) -> bool {
    if sig == 0 || sig > 64 {
        return false;
    }
    let Some(lead) = ps.iter().find(|p| p.tgid == tgid && !matches!(p.state, State::Zombie(_) | State::Dead)) else { return false };
    let act = lead.g.sigact[sig as usize];
    crate::socket::bump();
    // von allen Fäden gesperrt: nur vormerken (z. B. für signalfd), keine Standardaktion. A signal for one
    // thread waits at that thread: glibc's raise()/abort() block every signal around tgkill and
    // unblock afterwards – the signal must then reach the caller, not the group's first thread
    // (which may sleep in a futex for ever: the program did not end after abort())
    if sig != SIGKILL && ps.iter().filter(|p| p.tgid == tgid && !matches!(p.state, State::Zombie(_) | State::Dead)).all(|p| p.sigmask & bit(sig) != 0) {
        if let Some(p) = ps.iter_mut().find(|p| p.tgid == tgid && (tid == 0 || p.pid == tid) && !matches!(p.state, State::Zombie(_) | State::Dead)) {
            p.pending |= bit(sig);
        }
        return true;
    }
    if sig != SIGKILL && act.handler == 1 {
        return true;
    }
    if sig == SIGKILL || (act.handler == 0 && !default_ignores(sig)) {
        proc::kill_group(ps, tgid, sig as i32);
        return true;
    }
    if act.handler == 0 {
        return true;
    }
    // Empfänger: gewünschter Faden, sonst einer, der das Signal nicht sperrt
    let idx = ps
        .iter()
        .position(|p| p.tgid == tgid && (tid == 0 || p.pid == tid) && p.sigmask & bit(sig) == 0 && matches!(p.state, State::Ready | State::Blocked(_) | State::Running(_)))
        .or_else(|| ps.iter().position(|p| p.tgid == tgid && (tid == 0 || p.pid == tid) && matches!(p.state, State::Ready | State::Blocked(_) | State::Running(_))));
    let Some(i) = idx else { return false };
    let p = &mut ps[i];
    p.pending |= bit(sig);
    if p.sigmask & bit(sig) == 0 {
        interrupt(p, act.flags & SA_RESTART != 0);
    }
    true
}

/// wartenden Faden für die Signalzustellung wecken (Systemaufruf endet mit EINTR oder wird wiederholt)
fn interrupt(p: &mut Proc, restart: bool) {
    let State::Blocked(w) = p.state else { return };
    match w {
        Wait::Input | Wait::Child(_) | Wait::Pipe | Wait::Net(_) | Wait::Poll(_, _) | Wait::Tick(_) => {
            p.poll_dl = None;
            if !restart {
                p.frame.rip += 2;
                // a write that already delivered part of its data reports that part
                let done = core::mem::take(&mut p.io_done);
                p.frame.rax = if done > 0 { done } else { (-4i64) as u64 };
            }
        }
        Wait::Vfork(_) => return,
        Wait::Sleep(_) | Wait::Futex(_, _) | Wait::Signal => p.frame.rax = (-4i64) as u64,
    }
    p.state = State::Ready;
}

/// Strg+C: SIGINT an alle Gruppen
pub fn interrupt_all() {
    let mut ps = PROCS.lock();
    let groups: Vec<u64> = ps.iter().filter(|p| p.pid == p.tgid && !matches!(p.state, State::Zombie(_) | State::Dead)).map(|p| p.tgid).collect();
    for g in groups {
        send_locked(&mut ps, g, 0, SIGINT);
    }
}

fn w64(buf: &mut [u8], o: usize, v: u64) {
    buf[o..o + 8].copy_from_slice(&v.to_le_bytes());
}
fn r64(buf: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(buf[o..o + 8].try_into().unwrap())
}

const UC: usize = 8;
const UC_SIZE: usize = 304;
const INFO: usize = UC + UC_SIZE;
const FRAME: usize = INFO + 128;

/// Offenes Signal zustellen: Rahmen auf den Benutzerstapel legen und `f` auf den Handler umstellen.
/// Liefert false, wenn nichts zuzustellen war. Bei Standardaktion „beenden“ wird die Gruppe beendet.
pub fn deliver(p: &mut Proc, f: &mut TrapFrame) -> Result<bool, i32> {
    if p.g.space.is_none() {
        return Ok(false);
    }
    let ready = p.pending & !p.sigmask;
    if ready == 0 {
        return Ok(false);
    }
    let sig = ready.trailing_zeros() as u64 + 1;
    p.pending &= !bit(sig);
    let act = p.g.sigact[sig as usize];
    if act.handler == 1 || (act.handler == 0 && default_ignores(sig)) {
        return Ok(false);
    }
    if act.handler == 0 {
        return Err(sig as i32);
    }
    let old_mask = p.sus_mask.take().unwrap_or(p.sigmask);
    let alt = p.altstack;
    let fault = p.fault.take().filter(|_| is_fault_signal(sig));
    // SA_ONSTACK: the handler runs on the alternate stack unless it already does (no red zone there)
    let use_alt = act.flags & SA_ONSTACK != 0 && alt.size != 0 && !on_stack(&alt, f.rsp);
    let mut sp = if use_alt { alt.sp + alt.size } else { f.rsp.wrapping_sub(128) };
    // Gleitkomma-/Vektorzustand wie im Rahmen (FXSAVE oder XSAVE-Standardformat), auf 64 Byte
    sp = (sp - crate::trap::FX_SIZE as u64) & !63;
    let fx_addr = sp;
    sp -= FRAME as u64;
    let frame = (sp & !15) - 8;
    let mut b = alloc::vec![0u8; FRAME];
    w64(&mut b, 0, act.restorer);
    let cr2 = fault.map_or(0, |x| x.0);
    let regs = [
        f.r8, f.r9, f.r10, f.r11, f.r12, f.r13, f.r14, f.r15, f.rdi, f.rsi, f.rbp, f.rbx, f.rdx, f.rax, f.rcx, f.rsp, f.rip, f.rflags, 0x2b, f.err, f.vector, old_mask, cr2,
    ];
    for (i, r) in regs.iter().enumerate() {
        w64(&mut b, UC + 40 + i * 8, *r);
    }
    w64(&mut b, UC + 224, fx_addr);
    w64(&mut b, UC + 296, old_mask);
    write_uc_stack(&mut b, &alt, f.rsp);
    b[INFO..INFO + 4].copy_from_slice(&(sig as i32).to_le_bytes());
    if let Some((addr, code)) = fault {
        b[INFO + 8..INFO + 12].copy_from_slice(&code.to_le_bytes());
        w64(&mut b, INFO + 16, addr);
    }
    let sp_ok = {
        let space = p.space();
        proc::write_user(space, fx_addr, &f.fx) && proc::write_user(space, frame, &b)
    };
    if !sp_ok {
        return Err(SIGSEGV as i32);
    }
    if act.flags & SA_NODEFER == 0 {
        p.sigmask |= bit(sig);
    }
    p.sigmask |= act.mask;
    p.sigmask &= !(bit(SIGKILL) | bit(19));
    if act.flags & SA_RESETHAND != 0 {
        p.g.sigact[sig as usize] = Default::default();
    }
    if use_alt && alt.flags & SS_AUTODISARM != 0 {
        p.altstack = proc::AltStack::default();
    }
    f.rip = act.handler;
    f.rsp = frame;
    f.rdi = sig;
    f.rsi = frame + INFO as u64;
    f.rdx = frame + UC as u64;
    f.rax = 0;
    f.rflags &= !0x400; // DF
    Ok(true)
}

/// Den vom Programm zurückgegebenen Sicherungsbereich so beschneiden, dass `fxrstor`/`xrstor` keinen #GP auslösen:
/// MXCSR-Reservebits, im XSAVE-Kopf nur eingeschaltete Zustände, Standardformat (XCOMP_BV = 0), Reserve null.
fn sanitize_fx(fx: &mut [u8; crate::trap::FX_SIZE]) {
    let mut m = [0u8; 4];
    m.copy_from_slice(&fx[24..28]);
    fx[24..28].copy_from_slice(&(u32::from_le_bytes(m) & 0xffff).to_le_bytes());
    let mut bv = [0u8; 8];
    bv.copy_from_slice(&fx[512..520]);
    fx[512..520].copy_from_slice(&(u64::from_le_bytes(bv) & crate::arch::xcr0()).to_le_bytes());
    fx[520..576].fill(0);
}

/// rt_sigreturn: Zustand aus dem ucontext zurückholen
pub fn sigreturn(p: &mut Proc, f: &mut TrapFrame) -> bool {
    let uc = f.rsp;
    let Some(space) = p.g.space.as_ref() else { return false };
    let Some(b) = proc::read_user(space, uc, UC_SIZE) else { return false };
    let g = |i: usize| r64(&b, 40 + i * 8);
    f.r8 = g(0);
    f.r9 = g(1);
    f.r10 = g(2);
    f.r11 = g(3);
    f.r12 = g(4);
    f.r13 = g(5);
    f.r14 = g(6);
    f.r15 = g(7);
    f.rdi = g(8);
    f.rsi = g(9);
    f.rbp = g(10);
    f.rbx = g(11);
    f.rdx = g(12);
    f.rax = g(13);
    f.rcx = g(14);
    f.rsp = g(15);
    f.rip = g(16);
    f.rflags = (g(17) & 0xcd5) | 0x202;
    f.cs = crate::arch::UCODE;
    f.ss = crate::arch::UDATA;
    let fx = r64(&b, 224);
    if fx != 0 {
        if let Some(d) = proc::read_user(p.space(), fx, crate::trap::FX_SIZE) {
            f.fx.copy_from_slice(&d);
            sanitize_fx(&mut f.fx);
        }
    }
    p.sigmask = r64(&b, 296) & !(bit(SIGKILL) | bit(19));
    restore_altstack(p, &b, f.rsp);
    true
}

// ------------------------------------------------------------------ alternate signal stack

const SS_ONSTACK: u32 = 1;
const SS_DISABLE: u32 = 2;
const SS_AUTODISARM: u32 = 1 << 31;
/// Smallest alternate stack (`MINSIGSTKSZ`).
const MIN_ALT: u64 = 2048;

/// Whether `sp` lies on the alternate stack `alt`.
fn on_stack(alt: &proc::AltStack, sp: u64) -> bool {
    alt.size != 0 && sp > alt.sp && sp - alt.sp <= alt.size
}

/// The `ss_flags` the program reads back for `alt` while its stack pointer is `sp`.
fn ss_flags(alt: &proc::AltStack, sp: u64) -> u32 {
    if alt.size == 0 {
        SS_DISABLE
    } else if on_stack(alt, sp) {
        SS_ONSTACK
    } else {
        alt.flags & SS_AUTODISARM
    }
}

/// `uc_stack` of the ucontext (stack_t: sp, flags, size) at offset 16.
fn write_uc_stack(b: &mut [u8], alt: &proc::AltStack, sp: u64) {
    w64(b, UC + 16, alt.sp);
    b[UC + 24..UC + 28].copy_from_slice(&ss_flags(alt, sp).to_le_bytes());
    w64(b, UC + 32, alt.size);
}

/// rt_sigreturn puts back the alternate stack saved in `uc_stack` (as Linux' `restore_altstack`).
fn restore_altstack(p: &mut Proc, uc: &[u8], sp: u64) {
    let flags = u32::from_le_bytes(uc[24..28].try_into().unwrap());
    let (ss_sp, size) = (r64(uc, 16), r64(uc, 32));
    let _ = set_altstack(p, sp, ss_sp, flags, size);
}

/// Set the alternate stack; `Err(errno)` as Linux' `do_sigaltstack`.
fn set_altstack(p: &mut Proc, sp: u64, ss_sp: u64, flags: u32, size: u64) -> Result<(), i64> {
    if on_stack(&p.altstack, sp) {
        return Err(-1); // EPERM: the program runs on it
    }
    match flags & !SS_AUTODISARM {
        SS_DISABLE => p.altstack = proc::AltStack::default(),
        0 | SS_ONSTACK if size < MIN_ALT => return Err(-12), // ENOMEM
        0 | SS_ONSTACK => p.altstack = proc::AltStack { sp: ss_sp, size, flags: flags & SS_AUTODISARM },
        _ => return Err(-22), // EINVAL
    }
    Ok(())
}

/// `sigaltstack(ss, old_ss)`: `sp` is the caller's stack pointer.
pub fn sigaltstack(p: &mut Proc, sp: u64, ss: u64, old: u64) -> i64 {
    let before = p.altstack;
    if ss != 0 {
        let Some(b) = proc::read_user(p.space(), ss, 24) else { return -14 };
        let flags = u32::from_le_bytes(b[8..12].try_into().unwrap());
        if let Err(e) = set_altstack(p, sp, r64(&b, 0), flags, r64(&b, 16)) {
            return e;
        }
    }
    if old != 0 {
        let mut o = [0u8; 24];
        o[0..8].copy_from_slice(&before.sp.to_le_bytes());
        o[8..12].copy_from_slice(&ss_flags(&before, sp).to_le_bytes());
        o[16..24].copy_from_slice(&before.size.to_le_bytes());
        if !proc::write_user(p.space(), old, &o) {
            return -14;
        }
    }
    0
}

// ------------------------------------------------------------------ processor exceptions

fn is_fault_signal(sig: u64) -> bool {
    matches!(sig, 4 | 7 | 8 | 11)
}

/// What a processor exception reports (`si_addr`, `si_code`): the page-fault address with
/// SEGV_MAPERR/SEGV_ACCERR, else the instruction (ILL_ILLOPN, FPE_INTDIV, SEGV/BUS: SI_KERNEL).
pub fn fault_info(vector: u64, f: &TrapFrame, cr2: u64) -> (u64, i32) {
    match vector {
        14 => (cr2, if f.err & 1 == 0 { 1 } else { 2 }),
        6 => (f.rip, 2),
        0 => (f.rip, 1),
        16 | 19 => (f.rip, 0),
        _ => (0, 0x80),
    }
}
