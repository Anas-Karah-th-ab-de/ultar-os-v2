//! Systemaufrufe – Linux-kompatible Nummern und Strukturen (x86_64), damit statische Linux-Programme
//! (z. B. Ubuntus `busybox`) unverändert auf dem Ultar-Kern laufen. Eigene Umsetzung, kein Linux-Code.

use crate::fs;
use crate::isolation::path::{link_at, Loc, View};
use crate::isolation::rights::Rights;
use crate::isolation::{calls, signals};
use crate::proc::{self, Fd, FdKind, Pipe, Proc, State, Wait, PROCS};
use crate::trap::TrapFrame;
use alloc::collections::VecDeque;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const ESRCH: i64 = 3;
const EBADF: i64 = 9;
const ECHILD: i64 = 10;
const EAGAIN: i64 = 11;
const ENOMEM: i64 = 12;
const EACCES: i64 = 13;
const EFAULT: i64 = 14;
const EEXIST: i64 = 17;
const EXDEV: i64 = 18;
const ENOTDIR: i64 = 20;
const EISDIR: i64 = 21;
const EINVAL: i64 = 22;
const ENOTTY: i64 = 25;
const ESPIPE: i64 = 29;
const ENOSPC: i64 = 28;
const EROFS: i64 = 30;
const EPIPE: i64 = 32;
const ERANGE: i64 = 34;
const ENOSYS: i64 = 38;
const ELOOP: i64 = 40;
const ENOTCONN_U: i64 = 107;

// ------------------------------------------------------------------ Terminal (Zeilendisziplin)

struct Tty {
    line: Vec<u8>,
    ready: VecDeque<u8>,
    eof: bool,
    icanon: bool,
    echo: bool,
}

static TTY: Mutex<Tty> = Mutex::new(Tty { line: Vec::new(), ready: VecDeque::new(), eof: false, icanon: true, echo: true });

fn echo(b: &[u8]) {
    // nicht über kprint!: Programmausgaben gehören nicht ins Kernprotokoll (siehe console::UserOut)
    let _ = core::fmt::Write::write_str(&mut crate::console::UserOut, &String::from_utf8_lossy(b));
}

/// Tastatur und serielle Schnittstelle abfragen
pub fn poll_input() {
    // the terminal's buffers belong to the kernel, whoever happens to poll
    crate::heap::kernel(poll_terminal)
}

fn poll_terminal() {
    let mut t = TTY.lock();
    for _ in 0..64 {
        let c = match crate::console::serial_get() {
            Some(b) => b as char,
            None if crate::eingabe::roh() => break,
            None => match crate::drivers::ps2kbd::poll() {
                Some(c) => c,
                None => break,
            },
        };
        let mut buf = [0u8; 4];
        let bytes: &[u8] = c.encode_utf8(&mut buf).as_bytes();
        let b0 = if bytes[0] == b'\r' { b'\n' } else { bytes[0] };
        if b0 == 3 {
            t.line.clear();
            proc::INTERRUPT.store(true, Ordering::Relaxed);
            continue;
        }
        if t.icanon {
            match b0 {
                b'\n' => {
                    t.line.push(b'\n');
                    let l = core::mem::take(&mut t.line);
                    t.ready.extend(l);
                    if t.echo {
                        echo(b"\n");
                    }
                }
                0x7f | 0x08 => {
                    if t.line.pop().is_some() && t.echo {
                        echo(b"\x08 \x08");
                    }
                }
                4 => {
                    if t.line.is_empty() {
                        t.eof = true;
                    } else {
                        let l = core::mem::take(&mut t.line);
                        t.ready.extend(l);
                    }
                }
                _ => {
                    if b0 == bytes[0] {
                        t.line.extend_from_slice(bytes);
                    } else {
                        t.line.push(b0);
                    }
                    if t.echo {
                        echo(bytes);
                    }
                }
            }
        } else {
            if b0 == bytes[0] {
                t.ready.extend(bytes.iter().copied());
            } else {
                t.ready.push_back(b0);
            }
            if t.echo {
                echo(bytes);
            }
        }
    }
}

pub fn input_ready() -> bool {
    let t = TTY.lock();
    !t.ready.is_empty() || t.eof
}

// ------------------------------------------------------------------ Hilfen

static RND: AtomicU64 = AtomicU64::new(0x9e37_79b9_7f4a_7c15);

pub fn random_u64() -> u64 {
    let mut x = RND.load(Ordering::Relaxed) ^ crate::arch::rdtsc().rotate_left(17) ^ crate::arch::zufall();
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RND.store(x, Ordering::Relaxed);
    x
}

static EPOCH: AtomicU64 = AtomicU64::new(0);

fn cmos(r: u8) -> u8 {
    unsafe {
        crate::arch::outb(0x70, r);
        crate::arch::inb(0x71)
    }
}

/// Uhrzeit aus der Echtzeituhr (CMOS) lesen – einmal beim Start
pub fn init_clock() {
    let mut n = 0;
    while cmos(0x0A) & 0x80 != 0 && n < 100_000 {
        n += 1;
    }
    let b = cmos(0x0B);
    let bcd = |v: u8| if b & 4 == 0 { (v & 0x0f) + (v >> 4) * 10 } else { v };
    let (s, mi, h, d, mo, y) = (bcd(cmos(0)) as u64, bcd(cmos(2)) as u64, bcd(cmos(4) & 0x7f) as u64, bcd(cmos(7)) as u64, bcd(cmos(8)) as u64, bcd(cmos(9)) as u64 + 2000);
    // Tage seit 1970 (Algorithmus nach Howard Hinnant)
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2 / 400;
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let now = days * 86400 + h * 3600 + mi * 60 + s;
    EPOCH.store(now.saturating_sub(crate::arch::uptime_ms() / 1000), Ordering::Relaxed);
}

pub fn epoch_secs() -> u64 {
    now_ns(0) / 1_000_000_000
}

fn now_ns(clk: u64) -> u64 {
    let up = crate::arch::uptime_ns();
    if clk == 0 || clk == 8 {
        EPOCH.load(Ordering::Relaxed) * 1_000_000_000 + up
    } else {
        up
    }
}

pub(crate) fn with<R>(f: impl FnOnce(&mut Proc) -> R) -> Option<R> {
    let mut ps = PROCS.lock();
    let me = proc::current();
    ps.iter_mut().find(|p| p.pid == me).map(|p| f(p))
}

pub(crate) fn ustr(path: u64) -> Option<String> {
    with(|p| proc::read_cstr(p.space(), path)).flatten()
}

/// Resolve a path of the calling process inside its view (mount namespace + root, see isolation::path).
/// A relative path starts at the working directory or, with a directory descriptor, at the view path that
/// descriptor was opened with – but only if it was opened in the caller's current view (else `EXDEV`).
/// `follow`: follow a symbolic link in the last component. `Err` carries the negative error number.
pub(crate) fn locate(dirfd: i64, path: &str, follow: bool) -> Result<Loc, i64> {
    let snap = with(|p| {
        let view = View::of(&p.g.iso);
        let base = if dirfd >= 0 && !path.starts_with('/') {
            match p.fd(dirfd as u64).map(|f| &f.kind) {
                Some(FdKind::Dir { view: at, tag, .. }) => {
                    if *tag != view.tag() {
                        return Err(crate::isolation::path::PathError::OtherView.errno());
                    }
                    at.clone()
                }
                _ => p.g.cwd.clone(),
            }
        } else {
            p.g.cwd.clone()
        };
        Ok((view, base, p.g.iso.cred, p.g.iso.rights.has(Rights::FILE_ADMIN)))
    });
    let (view, base, cred, admin) = snap.unwrap_or(Err(-ENOENT))?;
    let loc = view.walk(&base, path, follow).map_err(|e| e.errno())?;
    // search permission on the directories that lead there (see perm.rs)
    if !admin {
        crate::perm::search(&view, &loc, &cred)?;
    }
    Ok(loc)
}

/// Number under which the calling thread sees the kernel-wide thread number `global` (0: not visible).
fn shown(p: &Proc, global: u64) -> i64 {
    p.link.ns().local(global).unwrap_or(0) as i64
}

fn put(va: u64, data: &[u8]) -> bool {
    with(|p| proc::write_user(p.space(), va, data)).unwrap_or(false)
}

fn get(va: u64, n: usize) -> Option<Vec<u8>> {
    with(|p| proc::read_user(p.space(), va, n)).flatten()
}

enum Res {
    Val(i64),
    /// Register nicht anfassen (rt_sigreturn)
    Keep,
    /// warten und Aufruf danach wiederholen
    Block(Wait),
    /// warten, Ergebnis steht schon fest
    BlockWith(Wait, i64),
    /// `write` delivered this many bytes and must wait for room to deliver the rest: the call is
    /// repeated for the remainder, its result is the sum
    Partial(u64, Wait),
}

/// Error number of a refused request for memory.
fn refused_errno(r: Res) -> Res {
    match r {
        Res::Val(v) if (-4095..0).contains(&v) => {
            let cpu = crate::percpu::me();
            let forced = core::mem::replace(&mut cpu.errno, 0) as i64;
            let refused = crate::heap::take_refused();
            // a lower layer named the error (group files), or the call failed because memory was refused
            Res::Val(if forced != 0 { -forced } else if refused { -ENOMEM } else { v })
        }
        r => r,
    }
}

fn block(f: &mut TrapFrame, w: Wait, restart: bool, val: i64) -> ! {
    if restart {
        f.rip -= 2;
    } else {
        f.rax = val as u64;
    }
    proc::switch_out(f, Some(State::Blocked(w)))
}

static UNKNOWN: Mutex<Vec<u64>> = Mutex::new(Vec::new());

/// Systemaufrufe protokollieren (`spur an` in der Kern-Konsole)
pub static TRACE: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

pub fn dispatch(f: &mut TrapFrame) {
    let nr = f.rax;
    crate::percpu::me().sys_rip = f.rip;
    let a = [f.rdi, f.rsi, f.rdx, f.r10, f.r8, f.r9];
    let trace = TRACE.load(Ordering::Relaxed) && nr != 1 && nr != 20;
    if trace {
        crate::kprintln!("[spur {}] {nr}({:#x}, {:#x}, {:#x}, {:#x})", proc::current(), a[0], a[1], a[2], a[3]);
        // calls that name a file: show the name (first or, with a directory descriptor, second argument)
        let named = match nr {
            2 | 4 | 6 | 21 | 59 | 89 => Some(a[0]),
            257 | 262 | 267 | 269 | 332 | 439 => Some(a[1]),
            _ => None,
        };
        if let Some(name) = named.and_then(ustr) {
            crate::kprintln!("[spur {}] {nr}: {name}", proc::current());
        }
    }
    let r = match nr {
        // calls guarded by a right the caller does not hold
        n if calls::gate(n).is_some() => Res::Val(-EPERM),
        155 => Res::Val(calls::sys_pivot_root(a[0], a[1])),
        161 => Res::Val(calls::sys_chroot(a[0])),
        165 => Res::Val(calls::sys_mount(a[0], a[1], a[2], a[3], a[4])),
        166 => Res::Val(calls::sys_umount(a[0], a[1])),
        272 => Res::Val(calls::sys_unshare(a[0])),
        319 => Res::Val(crate::memfd::create(a[1])),
        285 => Res::Val(crate::memfd::fallocate(a[0], a[1], a[2], a[3]).unwrap_or(-ENOSYS)),
        308 => Res::Val(calls::sys_setns(a[0], a[1])),
        88 => Res::Val(calls::sys_symlink(a[0], -100, a[1])),
        266 => Res::Val(calls::sys_symlink(a[0], a[1] as i32 as i64, a[2])),
        0 => sys_read(a[0], a[1], a[2], None),
        1 => write_res(a[0], a[1], a[2]),
        2 => Res::Val(sys_open(-100, a[0], a[1], a[2])),
        3 => Res::Val(sys_close(a[0])),
        4 => Res::Val(sys_stat_path(-100, a[0], a[1], 0)),
        6 => Res::Val(sys_stat_path(-100, a[0], a[1], 0x100)),
        5 => Res::Val(sys_fstat(a[0], a[1])),
        7 => sys_poll(a[0], a[1], a[2] as i64),
        8 => Res::Val(sys_lseek(a[0], a[1] as i64, a[2])),
        9 => Res::Val(sys_mmap(a[0], a[1], a[2], a[3], a[4] as i32 as i64, a[5])),
        // mlock/munlock/mlockall/munlockall (149–152): Speicher wird nie ausgelagert – nichts zu tun
        // user and group ids (cred.rs)
        102 | 104..=108 | 113..=120 | 122 | 123 => Res::Val(crate::cred::call(nr, a).unwrap_or(-ENOSYS)),
        // modes and owners (perm.rs)
        90 => Res::Val(crate::perm::sys_chmod(-100, a[0], a[1], 0)),
        91 => Res::Val(crate::perm::sys_fchmod(a[0], a[1])),
        92 | 94 => Res::Val(crate::perm::sys_chown(-100, a[0], a[1], a[2], if nr == 94 { 0x100 } else { 0 })),
        93 => Res::Val(crate::perm::sys_fchown(a[0], a[1], a[2])),
        260 => Res::Val(crate::perm::sys_chown(a[0] as i32 as i64, a[1], a[2], a[3], a[4])),
        268 | 452 => Res::Val(crate::perm::sys_chmod(a[0] as i32 as i64, a[1], a[2], if nr == 452 { a[3] } else { 0 })),
        74 | 75 => Res::Val(sys_fsync(a[0])),
        149 | 150 | 151 | 152 | 10 | 28 | 131 | 273 | 109 => Res::Val(0),
        11 => Res::Val(sys_munmap(a[0], a[1])),
        12 => Res::Val(sys_brk(a[0])),
        13 => Res::Val(sys_sigaction(a[0], a[1], a[2])),
        14 => Res::Val(sys_sigprocmask(a[0], a[1], a[2])),
        15 => {
            let ok = with(|p| crate::signal::sigreturn(p, f)).unwrap_or(false);
            if !ok {
                proc::exit_current(11);
            }
            Res::Keep
        }
        37 => Res::Val(0),
        16 => match crate::shm::vsync_wait(a[0], a[1]) {
            Some(until) => Res::BlockWith(Wait::Sleep(until), 0),
            None => match sys_ioctl(a[0], a[1], a[2]) {
                // a wait on the GPU node that is not over: sleep a moment, then the call repeats
                crate::svcdev::dri::RETRY => Res::Block(Wait::Sleep(crate::arch::uptime_ms() + 1)),
                r => Res::Val(r),
            },
        },
        25 => Res::Val(crate::shm::mremap(a[0], a[1], a[2], a[3], a[4])),
        17 => sys_read(a[0], a[1], a[2], Some(a[3])),
        18 => Res::Val(sys_pwrite(a[0], a[1], a[2], a[3])),
        19 => sys_readv(a[0], a[1], a[2]),
        20 => sys_writev(a[0], a[1], a[2]),
        21 => Res::Val(crate::perm::sys_access(-100, a[0], a[1], 0)),
        22 => Res::Val(sys_pipe(a[0], 0)),
        24 => {
            f.rax = 0;
            proc::switch_out(f, None)
        }
        32 => Res::Val(sys_dup(a[0], None, 0)),
        33 => Res::Val(sys_dup(a[0], Some(a[1]), 0)),
        35 => sys_sleep(a[0], false),
        39 => Res::Val(with(|p| shown(p, p.tgid)).unwrap_or(0)),
        186 => Res::Val(with(|p| shown(p, p.pid)).unwrap_or(0)),
        202 => sys_futex(a[0], a[1], a[2], a[3]),
        204 => {
            let (on, _) = crate::apic::cpu_count();
            let mask: u64 = if on >= 64 { u64::MAX } else { (1u64 << on) - 1 };
            Res::Val(if put(a[2], &mask.to_le_bytes()) { 8 } else { -EFAULT })
        }
        56 => Res::Val(sys_fork(f, a[0], a[1], a[2], a[3], a[4])),
        57 => Res::Val(sys_fork(f, 0x11, 0, 0, 0, 0)),
        58 => Res::Val(sys_fork(f, 0x4111, 0, 0, 0, 0)),
        59 => Res::Val(sys_execve(f, a[0], a[1], a[2])),
        60 => proc::exit_thread(((a[0] & 0xff) << 8) as i32, false),
        231 => proc::exit_thread(((a[0] & 0xff) << 8) as i32, true),
        61 => sys_wait4(a[0] as i32 as i64, a[1], a[2]),
        62 => Res::Val(sys_kill(a[0] as i32 as i64, a[1])),
        63 => Res::Val(sys_uname(a[0])),
        72 => Res::Val(sys_fcntl(a[0], a[1], a[2])),
        73 => Res::Val(crate::flock::sys_flock(a[0], a[1])),
        79 => Res::Val(sys_getcwd(a[0], a[1])),
        80 => Res::Val(sys_chdir(a[0])),
        83 => Res::Val(sys_mkdir(-100, a[0], a[1])),
        87 | 84 => Res::Val(sys_unlink(-100, a[0])),
        89 => Res::Val(sys_readlink(-100, a[0], a[1], a[2])),
        267 => Res::Val(sys_readlink(a[0] as i32 as i64, a[1], a[2], a[3])),
        40 => sys_sendfile(a[0], a[1], a[2], a[3]),
        // prctl: PR_GET_AUXV (Linux 6.4+) gibt es hier nicht – wie ältere Kernel mit EINVAL antworten, damit glibc/Rust
        // ihren Rückfallweg nehmen (sonst lesen Ubuntu-26-Programme eine leere Hilfstabelle und stürzen ab)
        157 => Res::Val(calls::sys_prctl(a[0], a[1])),
        // sync, syncfs: nothing is ever held back – every write to a disk is done when its call returns
        170 | 171 | 203 | 221 | 280 | 162 | 306 => Res::Val(0),
        82 => Res::Val(sys_rename(-100, a[0], -100, a[1])),
        264 | 316 => Res::Val(sys_rename(a[0] as i32 as i64, a[1], a[2] as i32 as i64, a[3])),
        77 => Res::Val(sys_ftruncate(a[0], a[1])),
        188..=199 => Res::Val(-95),
        41 => Res::Val(sys_socket(a[0], a[1], a[2])),
        169 => Res::Val(sys_reboot(a[2])),
        213 | 291 => Res::Val(sys_epoll_create(if nr == 291 { a[0] } else { 0 })),
        233 => Res::Val(sys_epoll_ctl(a[0], a[1], a[2] as i32 as i64, a[3])),
        232 | 281 | 441 => sys_epoll_wait(a[0], a[1], a[2], a[3] as i32 as i64),
        284 | 290 => Res::Val({
            let (init, fl) = (a[0] & 0xffff_ffff, if nr == 290 { a[1] } else { 0 });
            with(|p| p.add_fd(Fd { kind: FdKind::EventFd(Arc::new(spin::Mutex::new(init)), fl & 1 != 0), cloexec: fl & 0x80000 != 0, flags: 2 | (fl as u32 & 0x800) }, 0)).unwrap_or(-EBADF)
        }),
        283 => Res::Val(with(|p| p.add_fd(Fd { kind: FdKind::TimerFd(Arc::new(spin::Mutex::new((0, 0)))), cloexec: a[1] & 0x80000 != 0, flags: 2 | (a[1] as u32 & 0x800) }, 0)).unwrap_or(-EBADF)),
        286 => Res::Val(sys_timerfd_settime(a[0], a[1], a[2], a[3])),
        287 => Res::Val({
            let v = with(|p| match p.fd(a[0]).map(|d| &d.kind) {
                Some(FdKind::TimerFd(t)) => Some(*t.lock()),
                _ => None,
            })
            .flatten();
            match v {
                Some((dl, iv)) => {
                    let rem = dl.saturating_sub(crate::arch::uptime_ms());
                    let mut o = vec![0u8; 32];
                    o[0..8].copy_from_slice(&(iv / 1000).to_le_bytes());
                    o[8..16].copy_from_slice(&((iv % 1000) * 1_000_000).to_le_bytes());
                    o[16..24].copy_from_slice(&(rem / 1000).to_le_bytes());
                    o[24..32].copy_from_slice(&((rem % 1000) * 1_000_000).to_le_bytes());
                    if put(a[1], &o) { 0 } else { -EFAULT }
                }
                None => -EINVAL,
            }
        }),
        282 | 289 => Res::Val(sys_signalfd(a[0] as i32 as i64, a[1], if nr == 289 { a[3] } else { 0 })),
        42 => sys_connect(a[0], a[1], a[2]),
        43 => sys_accept(a[0], a[1], a[2], 0),
        288 => sys_accept(a[0], a[1], a[2], a[3]),
        44 if crate::scm::is_conn(a[0]) => match get(a[1], a[2].min(1 << 20) as usize) {
            Some(d) => scm_res(crate::scm::send(a[0], &d, &[], a[3])),
            None => Res::Val(-EFAULT),
        },
        44 => sys_sendto(a[0], a[1], a[2], a[4], a[5]),
        45 => sys_recvfrom(a[0], a[1], a[2], a[3], a[4], a[5]),
        46 => sys_sendmsg(a[0], a[1], a[2]),
        47 => sys_recvmsg(a[0], a[1], a[2]),
        307 => sys_sendmmsg(a[0], a[1], a[2]),
        48 => Res::Val(sys_shutdown(a[0], a[1])),
        49 => Res::Val(sys_bind(a[0], a[1], a[2])),
        50 => Res::Val(match sock_id(a[0]) {
            Some(id) => crate::socket::listen(id),
            None if unix_sock(a[0]).is_some() => 0,
            None => -88,
        }),
        51 => Res::Val(sys_sockname(a[0], a[1], a[2], false)),
        // getpeername: the real answer for an internet socket. For every other socket (local
        // ones) it stays what it always was here – success without an address –, because
        // call 52 used to stand in the "does nothing" list above and hid this line.
        52 => Res::Val(if sock_id(a[0]).is_some() { sys_sockname(a[0], a[1], a[2], true) } else { 0 }),
        53 => Res::Val(sys_socketpair(a[0], a[1], a[3])),
        54 => Res::Val(0),
        55 => Res::Val(sys_getsockopt(a[0], a[1], a[2], a[3], a[4])),
        137 | 138 => Res::Val(sys_statfs(nr == 138, a[0], a[1])),
        334 | 332 | 435 | 326 | 425 | 426 | 427 => Res::Val(-ENOSYS),
        86 | 265 => Res::Val(-1),
        95 => Res::Val(with(|p| core::mem::replace(&mut p.g.umask, a[0] as u32 & 0o777) as i64).unwrap_or(0)),
        96 => {
            let ns = now_ns(0);
            if a[0] != 0 {
                let mut b = Vec::new();
                b.extend_from_slice(&(ns / 1_000_000_000).to_le_bytes());
                b.extend_from_slice(&((ns % 1_000_000_000) / 1000).to_le_bytes());
                put(a[0], &b);
            }
            Res::Val(0)
        }
        97 => Res::Val(sys_prlimit(a[0], a[1])),
        99 => Res::Val(sys_sysinfo(a[0])),

        110 => Res::Val(with(|p| shown(p, p.ppid)).unwrap_or(0)),
        111 | 121 => Res::Val(with(|p| shown(p, p.pgid)).unwrap_or(0)),
        112 | 124 => Res::Val(with(|p| shown(p, p.pid)).unwrap_or(0)),
        158 => Res::Val(sys_arch_prctl(a[0], a[1])),
        200 => Res::Val(sys_tgkill(None, a[0], a[1])),
        234 => Res::Val(sys_tgkill(Some(a[0]), a[1], a[2])),
        201 => {
            let s = now_ns(0) / 1_000_000_000;
            if a[0] != 0 {
                put(a[0], &s.to_le_bytes());
            }
            Res::Val(s as i64)
        }
        217 => Res::Val(sys_getdents(a[0], a[1], a[2])),
        218 => Res::Val(with(|p| {
            p.clear_tid = a[0];
            shown(p, p.pid)
        })
        .unwrap_or(0)),
        228 => {
            let ns = now_ns(a[0]);
            let mut b = Vec::new();
            b.extend_from_slice(&(ns / 1_000_000_000).to_le_bytes());
            b.extend_from_slice(&(ns % 1_000_000_000).to_le_bytes());
            Res::Val(if put(a[1], &b) { 0 } else { -EFAULT })
        }
        229 => {
            let mut b = vec![0u8; 16];
            b[8..16].copy_from_slice(&1u64.to_le_bytes());
            if a[1] != 0 {
                put(a[1], &b);
            }
            Res::Val(0)
        }
        230 => sys_sleep(a[2], a[1] & 1 != 0),
        257 => Res::Val(sys_open(a[0] as i32 as i64, a[1], a[2], a[3])),
        258 => Res::Val(sys_mkdir(a[0] as i32 as i64, a[1], a[2])),
        262 => Res::Val(sys_stat_path(a[0] as i32 as i64, a[1], a[2], a[3])),
        263 => Res::Val(sys_unlink(a[0] as i32 as i64, a[1])),
        269 | 439 => Res::Val(crate::perm::sys_access(a[0] as i32 as i64, a[1], a[2], if nr == 439 { a[3] } else { 0 })),
        // select: timeval (microseconds), rounded up to whole milliseconds
        23 => sys_select(a[0], a[1], a[2], if a[4] == 0 { -1 } else { get(a[4], 16).map(|b| u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])).saturating_mul(1000).saturating_add(u64::from_le_bytes(b[8..16].try_into().unwrap_or([0; 8])).min(999_999).div_ceil(1000)).min(i64::MAX as u64) as i64).unwrap_or(-1) }),
        270 => sys_select(a[0], a[1], a[2], if a[4] == 0 { -1 } else { get(a[4], 16).map(|b| timespec_ms_up(&b) as i64).unwrap_or(-1) }),
        130 | 34 => {
            if nr == 130 {
                let m = get(a[0], 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())).unwrap_or(0);
                with(|p| {
                    p.sus_mask = Some(p.sigmask);
                    p.sigmask = m & !(1 << 8);
                });
            }
            Res::BlockWith(Wait::Signal, -4)
        }
        // ppoll: the timespec with its nanoseconds (rounded up to whole milliseconds, never to 0 early)
        271 => sys_poll(a[0], a[1], if a[2] == 0 { -1 } else { get(a[2], 16).map(|b| timespec_ms_up(&b) as i64).unwrap_or(-1) }),
        292 => Res::Val(sys_dup(a[0], Some(a[1]), a[2])),
        293 => Res::Val(sys_pipe(a[0], a[1])),
        302 => Res::Val(sys_prlimit(a[1], a[3])),
        318 => {
            let n = a[1].min(1 << 20) as usize;
            match crate::mem::try_vec(n) {
                Some(mut b) => {
                    for c in b.chunks_mut(8) {
                        let r = random_u64().to_le_bytes();
                        c.copy_from_slice(&r[..c.len()]);
                    }
                    Res::Val(if put(a[0], &b) { n as i64 } else { -EFAULT })
                }
                None => Res::Val(-ENOMEM),
            }
        }
        _ => {
            crate::heap::kernel(|| {
                let mut u = UNKNOWN.lock();
                if !u.contains(&nr) {
                    u.push(nr);
                    crate::kprintln!("[syscall] noch nicht umgesetzt: {nr}");
                }
            });
            Res::Val(-ENOSYS)
        }
    };
    if trace {
        if let Res::Val(v) = &r {
            crate::kprintln!("[spur {}] {nr} = {v}", proc::current());
        }
    }
    match refused_errno(r) {
        Res::Val(v) => f.rax = v as u64,
        Res::Keep => {}
        Res::Block(w) => block(f, w, true, 0),
        Res::BlockWith(w, v) => block(f, w, false, v),
        Res::Partial(done, w) => {
            with(|p| p.io_done += done);
            f.rsi += done;
            f.rdx -= done;
            block(f, w, true, 0)
        }
    }
    // offene Signale vor der Rückkehr zustellen
    if let Some(Err(sig)) = with(|p| crate::signal::deliver(p, f)) {
        proc::exit_current(sig);
    }
}

// ------------------------------------------------------------------ Dateien

fn sys_read(fd: u64, buf: u64, n: u64, at: Option<u64>) -> Res {
    if let Some(r) = crate::rawpart::sys_read(fd, buf, n, at) {
        return Res::Val(r);
    }
    match crate::svcdev::hci::sys_read(fd, buf, n) {
        Some(crate::svcdev::hci::Io::Val(v)) => return Res::Val(v),
        // the driver polls: look again in 8 ms or when something happens
        Some(crate::svcdev::hci::Io::Block) => return Res::Block(Wait::Poll(crate::socket::gen(), crate::arch::uptime_ms() + 8)),
        None => {}
    }
    if let Some(r) = crate::svcdev::uinput::sys_read(fd) {
        return Res::Val(r);
    }
    let n = n.min(1 << 24) as usize;
    enum Src {
        Data(Vec<u8>),
        /// already copied to the caller (or failed): the result
        Done(i64),
        Sock(usize),
        SigFd(u64, bool),
        Console,
        Block(Wait),
        Err(i64),
    }
    let src = with(|p| {
        // sources that are copied straight into the caller's memory (no buffer of the caller's size)
        enum Direct {
            Fill(bool),
            File(Arc<Vec<u8>>, u64),
            Mem(Arc<crate::memfd::MemObj>, u64),
            No,
        }
        let direct = match p.fd(fd).map(|d| &d.kind) {
            Some(FdKind::Zero) => Direct::Fill(false),
            Some(FdKind::Random) => Direct::Fill(true),
            Some(FdKind::File { data, pos, .. }) => Direct::File(data.clone(), *pos),
            Some(FdKind::Mem(m, pos)) => Direct::Mem(m.clone(), *pos),
            _ => Direct::No,
        };
        match direct {
            Direct::Fill(random) => {
                let mut page = [0u8; 4096];
                let mut done = 0;
                while done < n {
                    let k = (n - done).min(4096);
                    if random {
                        for c in page[..k].chunks_mut(8) {
                            let r = random_u64().to_le_bytes();
                            c.copy_from_slice(&r[..c.len()]);
                        }
                    }
                    if !proc::write_user(p.space(), buf + done as u64, &page[..k]) {
                        return Src::Done(if done > 0 { done as i64 } else { -EFAULT });
                    }
                    done += k;
                }
                return Src::Done(n as i64);
            }
            Direct::File(data, pos) => {
                let start = at.unwrap_or(pos).min(data.len() as u64) as usize;
                let end = (start + n).min(data.len());
                if end > start && !proc::write_user(p.space(), buf, &data[start..end]) {
                    return Src::Done(-EFAULT);
                }
                if at.is_none() {
                    if let Some(FdKind::File { pos, .. }) = p.fd(fd).map(|d| &mut d.kind) {
                        *pos = end as u64;
                    }
                }
                return Src::Done((end - start) as i64);
            }
            Direct::Mem(m, pos) => {
                let bytes = m.read(at.unwrap_or(pos), n);
                if !bytes.is_empty() && !proc::write_user(p.space(), buf, &bytes) {
                    return Src::Done(-EFAULT);
                }
                if at.is_none() {
                    if let Some(FdKind::Mem(_, pos)) = p.fd(fd).map(|d| &mut d.kind) {
                        *pos += bytes.len() as u64;
                    }
                }
                return Src::Done(bytes.len() as i64);
            }
            Direct::No => {}
        }
        let Some(d) = p.fd(fd) else { return Src::Err(-EBADF) };
        match &mut d.kind {
            FdKind::Console => Src::Console,
            FdKind::Null => Src::Data(Vec::new()),
            // handled above (copied straight into the caller's memory)
            FdKind::Zero | FdKind::Random | FdKind::File { .. } | FdKind::RawPart(_) | FdKind::Mem(..) | FdKind::Hci(_) | FdKind::Uinput(_) | FdKind::Dri(_) => Src::Err(-EBADF),
            FdKind::Dir { .. } => Src::Err(-EISDIR),
            FdKind::Ns(_) => Src::Err(-EINVAL),
            FdKind::Fb(_) => match at {
                Some(off) => Src::Data(crate::eingabe::fb_lesen(off, n)),
                None => Src::Err(-EINVAL),
            },
            FdKind::Eingabe(e) if e.0 >= crate::input::FIRST => match crate::input::read(e.0, e.2, n) {
                Ok(Some(d)) => Src::Data(d),
                Ok(None) if d.flags & 0x800 != 0 => Src::Err(-11),
                Ok(None) => Src::Block(Wait::Poll(crate::socket::gen(), crate::arch::uptime_ms() + 8)),
                Err(e) => Src::Err(e),
            },
            FdKind::Eingabe(e) => match crate::eingabe::lesen(e.2, n) {
                Some(d) => Src::Data(d),
                None if d.flags & 0x800 != 0 => Src::Err(-11),
                None => Src::Block(Wait::Poll(crate::socket::gen(), crate::arch::uptime_ms() + 8)),
            },
            FdKind::PipeR(pp) | FdKind::UnixConn(pp, _) => {
                let mut b = pp.buf.lock();
                if b.is_empty() {
                    if pp.write_closed() {
                        Src::Data(Vec::new())
                    } else if d.flags & 0x800 != 0 {
                        Src::Err(-11)
                    } else {
                        Src::Block(Wait::Net(crate::socket::gen()))
                    }
                } else {
                    match pp.take(&mut b, n) {
                        Some(out) => {
                            // room for a waiting writer
                            crate::socket::bump();
                            Src::Data(out)
                        }
                        None => Src::Err(-ENOMEM),
                    }
                }
            }
            FdKind::PipeW(_) => Src::Err(-EBADF),
            FdKind::Sock(h) => Src::Sock(h.0),
            FdKind::SigFd(m) => Src::SigFd(*m, d.flags & 0x800 != 0),
            FdKind::UnixSock(_) => Src::Err(-ENOTCONN_U),
            FdKind::Epoll(_) => Src::Err(-EINVAL),
            FdKind::EventFd(c, sem) => {
                let mut v = c.lock();
                if *v == 0 {
                    if d.flags & 0x800 != 0 {
                        Src::Err(-11)
                    } else {
                        Src::Block(Wait::Poll(crate::socket::gen(), 0))
                    }
                } else {
                    let out = if *sem { 1 } else { *v };
                    *v -= out;
                    crate::socket::bump();
                    Src::Data(out.to_le_bytes().to_vec())
                }
            }
            FdKind::TimerFd(t) => {
                let mut st = t.lock();
                let now = crate::arch::uptime_ms();
                let (dl, iv) = *st;
                if dl == 0 || now < dl {
                    if d.flags & 0x800 != 0 {
                        Src::Err(-11)
                    } else if dl == 0 {
                        Src::Block(Wait::Poll(crate::socket::gen(), 0))
                    } else {
                        Src::Block(Wait::Poll(crate::socket::gen(), dl))
                    }
                } else {
                    let n = if iv > 0 { 1 + (now - dl) / iv } else { 1 };
                    st.0 = if iv > 0 { dl + n * iv } else { 0 };
                    Src::Data(n.to_le_bytes().to_vec())
                }
            }
        }
    })
    .unwrap_or(Src::Err(-EBADF));
    match src {
        Src::Data(d) => Res::Val(if put(buf, &d) || d.is_empty() { d.len() as i64 } else { -EFAULT }),
        Src::Done(v) => Res::Val(v),
        Src::Err(e) => Res::Val(e),
        Src::Block(w) => Res::Block(w),
        Src::Sock(id) => sock_recv(id, buf, n as u64, 0, 0, 0),
        Src::SigFd(m, nb) => {
            let sig = take_signal(m);
            if sig == 0 {
                return if nb { Res::Val(-11) } else { Res::Block(Wait::Poll(crate::socket::gen(), 0)) };
            }
            let mut b = vec![0u8; 128];
            b[0..4].copy_from_slice(&(sig as u32).to_le_bytes());
            Res::Val(if put(buf, &b) { 128 } else { -EFAULT })
        }
        Src::Console => {
            poll_input();
            let mut t = TTY.lock();
            if t.ready.is_empty() {
                if t.eof {
                    t.eof = false;
                    return Res::Val(0);
                }
                return Res::Block(Wait::Input);
            }
            let mut out = Vec::new();
            while out.len() < n {
                let Some(c) = t.ready.pop_front() else { break };
                out.push(c);
                if c == b'\n' && t.icanon {
                    break;
                }
            }
            drop(t);
            Res::Val(if put(buf, &out) { out.len() as i64 } else { -EFAULT })
        }
    }
}

/// Make the buffer of an open file writable with room for `len` bytes in total. The buffer is
/// shared with other descriptors and the stored file until the first write (copy on write).
/// `None`: no memory – the buffer is unchanged.
fn file_room(data: &mut Arc<Vec<u8>>, len: usize) -> Option<&mut Vec<u8>> {
    if Arc::get_mut(data).is_none() {
        let mut own = crate::mem::try_with_capacity(len.max(data.len()))?;
        own.extend_from_slice(data);
        *data = Arc::new(own);
    }
    let v = Arc::get_mut(data)?;
    if len > v.len() && !crate::mem::try_reserve(v, len - v.len()) {
        return None;
    }
    Some(v)
}

/// Write `data` at `*pos` into the buffer of an open file at host path `path`.
fn file_write(fd_data: &mut Arc<Vec<u8>>, pos: &mut u64, path: &str, data: &[u8]) -> Result<(), i64> {
    let Some(end) = pos.checked_add(data.len() as u64).filter(|&e| e <= MAX_FILE_BYTES) else { return Err(-EFBIG) };
    // quota of the RAM tree the file lives in (tmpfs, RAM layer); other files have none
    if !fs::grow(path, end.max(fd_data.len() as u64)) {
        return Err(-ENOSPC);
    }
    let Some(v) = file_room(fd_data, end as usize) else { return Err(-ENOMEM) };
    let s = *pos as usize;
    if v.len() < s + data.len() {
        v.resize(s + data.len(), 0);
    }
    v[s..s + data.len()].copy_from_slice(data);
    *pos = end;
    Ok(())
}

/// Put `data` into a pipe or Unix socket direction for a `write`-like call. `Ok(n)`: `n` bytes
/// were taken (`n < data.len()`: the buffer is full now). `Err(Res)`: nothing was taken.
fn pipe_write(pp: &Pipe, data: &[u8], nonblock: bool) -> Result<usize, Res> {
    // read before trying: a reader that makes room right after the failed try still wakes this writer
    let seen = crate::socket::gen();
    match pp.push(data) {
        Ok(k) => {
            crate::socket::bump();
            Ok(k)
        }
        Err(crate::pipe::PushError::NoMemory) => Err(Res::Val(-ENOMEM)),
        Err(crate::pipe::PushError::Full) if nonblock => Err(Res::Val(-11)),
        Err(crate::pipe::PushError::Full) => Err(Res::Block(Wait::Net(seen))),
    }
}

fn sys_write(fd: u64, buf: u64, n: u64) -> i64 {
    match write_res(fd, buf, n) {
        Res::Val(v) => v,
        Res::Partial(done, _) => done as i64,
        _ => -11,
    }
}

/// `write`: the result can be "wait and repeat" (full pipe) – see [`Res::Partial`].
fn write_res(fd: u64, buf: u64, n: u64) -> Res {
    if let Some(r) = crate::rawpart::sys_write(fd, buf, n, None) {
        return Res::Val(r);
    }
    if let Some(r) = crate::svcdev::hci::sys_write(fd, buf, n) {
        return Res::Val(r);
    }
    if let Some(r) = crate::svcdev::uinput::sys_write(fd, buf, n) {
        return Res::Val(r);
    }
    let want = n.min(1 << 24) as usize;
    // a large write to a pipe is delivered in pieces (see `Res::Partial`): copy from the caller only
    // what the pipe can hold at once, not the whole request each time the writer wakes up
    let piece = if want > crate::pipe::PIPE_DEFAULT as usize {
        with(|p| match p.fd(fd).map(|d| &d.kind) {
            Some(FdKind::PipeW(pp)) | Some(FdKind::UnixConn(_, pp)) => want.min(pp.capacity() as usize),
            _ => want,
        })
        .unwrap_or(want)
    } else {
        want
    };
    let Some(data) = get(buf, piece) else { return Res::Val(-EFAULT) };
    enum Out {
        Echo,
        Done(usize),
        Sock(usize),
        Fb,
        Group(String),
        Res(Res),
    }
    let r = with(|p| {
        let Some(d) = p.fd(fd) else { return Out::Res(Res::Val(-EBADF)) };
        let nonblock = d.flags & 0x800 != 0;
        match &mut d.kind {
            FdKind::Console => Out::Echo,
            FdKind::Null | FdKind::Zero | FdKind::Random => Out::Done(data.len()),
            FdKind::Mem(m, pos) => match m.write(*pos, &data) {
                Ok(k) => {
                    *pos += k as u64;
                    Out::Done(k)
                }
                Err(e) => Out::Res(Res::Val(e)),
            },
            FdKind::File { data: fd_data, pos, write, append, dirty, path } => {
                if !*write {
                    return Out::Res(Res::Val(-EBADF));
                }
                if crate::groups::files::is_inside(path) || path == PIPE_MAX_FILE || crate::devices::sysfs::is_control(path) {
                    return Out::Group(path.clone());
                }
                if *append {
                    *pos = fd_data.len() as u64;
                }
                match file_write(fd_data, pos, path, &data) {
                    Ok(()) => {
                        *dirty = true;
                        Out::Done(data.len())
                    }
                    Err(e) => Out::Res(Res::Val(e)),
                }
            }
            FdKind::Sock(h) => Out::Sock(h.0),
            FdKind::Fb(_) => Out::Fb,
            FdKind::Eingabe(e) => {
                crate::eingabe::einspeisen(e.0, &data);
                Out::Done(data.len())
            }
            FdKind::EventFd(c, _) => {
                if data.len() < 8 {
                    return Out::Res(Res::Val(-EINVAL));
                }
                *c.lock() += u64::from_le_bytes(data[0..8].try_into().unwrap());
                crate::socket::bump();
                Out::Done(data.len())
            }
            FdKind::PipeW(pp) | FdKind::UnixConn(_, pp) => {
                // a blocking write that was continued (see `Res::Partial`) reports everything it
                // delivered, also when the rest fails
                if pp.read_closed() {
                    p.pending |= 1 << (crate::signal::SIGPIPE - 1);
                    let done = core::mem::take(&mut p.io_done);
                    return if done > 0 { Out::Done(done as usize) } else { Out::Res(Res::Val(-EPIPE)) };
                }
                let seen = crate::socket::gen();
                match pipe_write(pp, &data, nonblock) {
                    Ok(k) if k == want || nonblock => Out::Done(k + core::mem::take(&mut p.io_done) as usize),
                    // a blocking writer waits until everything is delivered (counter read before the
                    // write: room made in between is not missed)
                    Ok(k) => Out::Res(Res::Partial(k as u64, Wait::Net(seen))),
                    Err(Res::Val(e)) => match core::mem::take(&mut p.io_done) {
                        0 => Out::Res(Res::Val(e)),
                        done => Out::Done(done as usize),
                    },
                    Err(r) => Out::Res(r),
                }
            }
            _ => Out::Res(Res::Val(-EBADF)),
        }
    })
    .unwrap_or(Out::Res(Res::Val(-EBADF)));
    Res::Val(match r {
        Out::Echo => {
            // the console's own buffers belong to the kernel, not to the writer
            crate::heap::kernel(|| echo(&data));
            data.len() as i64
        }
        Out::Done(k) => k as i64,
        Out::Sock(id) => crate::socket::send(id, &data, None),
        Out::Fb => crate::eingabe::fb_schreiben(0, &data),
        Out::Group(path) => control_write(&path, &data),
        Out::Res(r) => return r,
    })
}

/// Path of the control file for the largest pipe size.
const PIPE_MAX_FILE: &str = "/proc/sys/fs/pipe-max-size";

/// A write to a kernel control file takes effect at once and reports its error (the data is not
/// kept in the descriptor): the group tree and `/proc/sys/fs/pipe-max-size`.
fn control_write(path: &str, data: &[u8]) -> i64 {
    let text = core::str::from_utf8(data).unwrap_or("").trim();
    if crate::devices::sysfs::is_control(path) {
        if !calls::holds(crate::devices::sysfs::right_for(path)) {
            return -EPERM;
        }
        return crate::devices::sysfs::write(path, text).map_or_else(|e| e, |()| data.len() as i64);
    }
    if !calls::holds(Rights::MANAGE_GROUPS) {
        return -EPERM;
    }
    if path == PIPE_MAX_FILE {
        return match text.parse::<u64>() {
            Ok(v) if crate::pipe::set_max_size(v) => data.len() as i64,
            _ => -EINVAL,
        };
    }
    let own_ns = with(|p| p.link.ns().clone());
    let pid_of = |local: u64| own_ns.as_ref().and_then(|ns| ns.global(local));
    match crate::groups::files::write(path, text, &pid_of) {
        Ok(()) => data.len() as i64,
        Err(e) => e,
    }
}

/// pwrite64: Dateien an Stelle, Bildspeicher ab Versatz
fn sys_pwrite(fd: u64, buf: u64, n: u64, off: u64) -> i64 {
    if let Some(r) = crate::rawpart::sys_write(fd, buf, n, Some(off)) {
        return r;
    }
    let ist_fb = with(|p| matches!(p.fd(fd).map(|d| &d.kind), Some(FdKind::Fb(_)))).unwrap_or(false);
    if ist_fb {
        let Some(data) = get(buf, n.min(1 << 25) as usize) else { return -EFAULT };
        return crate::eingabe::fb_schreiben(off, &data);
    }
    // sonst: Position merken, schreiben, zurücksetzen
    let alt = with(|p| match p.fd(fd).map(|d| &mut d.kind) {
        Some(FdKind::File { pos, append, .. }) if !*append => {
            let a = *pos;
            *pos = off;
            Some(a)
        }
        Some(FdKind::Mem(_, pos)) => Some(core::mem::replace(pos, off)),
        _ => None,
    })
    .flatten();
    let Some(alt) = alt else { return -ESPIPE };
    let r = sys_write(fd, buf, n);
    with(|p| match p.fd(fd).map(|d| &mut d.kind) {
        Some(FdKind::File { pos, .. }) | Some(FdKind::Mem(_, pos)) => *pos = alt,
        _ => {}
    });
    r
}

fn iov(base: u64, cnt: u64) -> Vec<(u64, u64)> {
    let Some(b) = get(base, (cnt.min(1024) * 16) as usize) else { return Vec::new() };
    b.chunks(16).map(|c| (u64::from_le_bytes(c[0..8].try_into().unwrap()), u64::from_le_bytes(c[8..16].try_into().unwrap()))).collect()
}

fn sys_writev(fd: u64, base: u64, cnt: u64) -> Res {
    let mut total = 0;
    for (b, l) in iov(base, cnt) {
        if l == 0 {
            continue;
        }
        match write_res(fd, b, l) {
            Res::Val(r) if r < 0 => return Res::Val(if total > 0 { total } else { r }),
            Res::Val(r) => {
                total += r;
                if (r as u64) < l.min(1 << 24) {
                    break;
                }
            }
            // full pipe: what was delivered so far is the result (a short write); nothing yet: wait
            Res::Partial(done, _) => return Res::Val(total + done as i64),
            other => return if total > 0 { Res::Val(total) } else { other },
        }
    }
    Res::Val(total)
}

fn sys_readv(fd: u64, base: u64, cnt: u64) -> Res {
    let v = iov(base, cnt);
    let mut total = 0i64;
    for (b, l) in v {
        if l == 0 {
            continue;
        }
        match sys_read(fd, b, l, None) {
            Res::Val(r) if r < 0 => return Res::Val(if total > 0 { total } else { r }),
            Res::Val(r) => {
                total += r;
                if (r as u64) < l {
                    break;
                }
            }
            other => {
                if total > 0 {
                    return Res::Val(total);
                }
                return other;
            }
        }
    }
    Res::Val(total)
}

fn sys_open(dirfd: i64, path: u64, flags: u64, _mode: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    let nofollow = flags & 0x20000 != 0;
    let loc = match locate(dirfd, &p, !nofollow) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if nofollow && link_at(&loc).is_some() {
        return -ELOOP;
    }
    let path = loc.host.clone();
    let acc = flags & 3;
    let (creat, excl, trunc, append, dironly, cloexec) = (flags & 0x40 != 0, flags & 0x80 != 0, flags & 0x200 != 0, flags & 0x400 != 0, flags & 0x10000 != 0, flags & 0x80000 != 0);
    let rights = with(|p| p.g.iso.rights).unwrap_or(Rights::NONE);
    // namespace handle /proc/<pid>/ns/{mnt,pid} (argument of setns)
    if path.starts_with("/proc/") {
        if let Some((tgid, entry)) = crate::procfs::pid_entry(&path) {
            if let Some(kind) = entry.strip_prefix("ns/") {
                let Some(ns) = proc::ns_ref(tgid, kind) else { return -ENOENT };
                return with(|p| p.add_fd(Fd { kind: FdKind::Ns(ns), cloexec, flags: flags as u32 }, 0)).unwrap_or(-EBADF);
            }
        }
        if acc != 0 && (path == "/proc/ultar/firewall" || path == "/proc/ultar/verbund") && !rights.has(Rights::NET_ADMIN) {
            return -EACCES;
        }
    }
    // limits are changed through the group tree and /proc/sys/fs/pipe-max-size: only with the right
    if acc != 0 && (crate::groups::files::is_tree(&path) || path == PIPE_MAX_FILE) && !rights.has(Rights::MANAGE_GROUPS) {
        return -EACCES;
    }
    // drivers are bound and unbound through the device tree: only its control files, only with the right
    if acc != 0 && crate::devices::sysfs::is_tree(&path) && !(crate::devices::sysfs::is_control(&path) && rights.has(crate::devices::sysfs::right_for(&path))) {
        return -EACCES;
    }
    if let Err(e) = crate::perm::open_gate(&path, acc, trunc, creat) {
        return e;
    }
    let kind = match fs::open(&path) {
        Some(_) if creat && excl => return -EEXIST,
        Some(fs::Node::Dev(d)) => match d {
            "null" => FdKind::Null,
            "zero" => FdKind::Zero,
            "urandom" | "random" => FdKind::Random,
            d if d.starts_with("ultar/part/") => match crate::rawpart::open(d, acc != 0, rights.has(Rights::RAW_DEVICE)) {
                Ok(k) => k,
                Err(e) => return e,
            },
            "ultar/disk" | "ultar/medium" => {
                let dev = path.strip_prefix("/dev/").unwrap_or("");
                match crate::rawpart::open_installer(dev, acc != 0, rights.has(Rights::RAW_DEVICE)) {
                    Ok(k) => k,
                    Err(e) => return e,
                }
            }
            d if crate::svcdev::node(d).is_some() => match crate::svcdev::open(d, rights) {
                Ok(k) => k,
                Err(e) => return e,
            },
            "fb0" => match if acc == 0 { crate::eingabe::FbFd::lesend() } else { crate::eingabe::FbFd::neu() } {
                Some(f) => FdKind::Fb(Arc::new(f)),
                None => return -19,
            },
            "input/event0" | "input/event1" | "input/usb" => {
                let Some(art) = path.rsplit('/').next().and_then(crate::input::parse) else { return -ENOENT };
                FdKind::Eingabe(Arc::new(if acc == 1 { crate::eingabe::EingabeFd::schreibend(art) } else { crate::eingabe::EingabeFd::neu(art) }))
            }
            _ => FdKind::Console,
        },
        Some(fs::Node::Dir(ents)) => {
            if acc != 0 {
                return -EISDIR;
            }
            FdKind::Dir { ents: Arc::new(ents), pos: 0, path: path.clone(), view: loc.view.clone(), tag: loc.tag }
        }
        Some(fs::Node::File(data)) => {
            if dironly {
                return -ENOTDIR;
            }
            if acc != 0 && loc.read_only {
                return -EROFS;
            }
            let data = if trunc && acc != 0 { Arc::new(Vec::new()) } else { data };
            FdKind::File { data, pos: 0, path: path.clone(), append, write: acc != 0, dirty: trunc && acc != 0 }
        }
        None if creat => {
            if loc.read_only {
                return -EROFS;
            }
            if loc.confined && kernel_tree(&path) {
                return -EPERM;
            }
            let parent = fs::normalize(&path, "..");
            if !matches!(fs::stat(&parent), Some((1, _))) {
                return -ENOENT;
            }
            // nothing can be created in the kernel's control trees; elsewhere the RAM tree may be full
            if path.starts_with("/proc/") || crate::groups::files::is_tree(&path) {
                return -EPERM;
            }
            let empty = Arc::new(Vec::new());
            if !fs::store_file(&path, empty.clone()) {
                return -ENOSPC;
            }
            crate::perm::created(&path, crate::perm::S_IFREG, _mode as u32);
            FdKind::File { data: empty, pos: 0, path: path.clone(), append, write: true, dirty: false }
        }
        None => return -ENOENT,
    };
    with(|p| p.add_fd(Fd { kind, cloexec, flags: flags as u32 }, 0)).unwrap_or(-EBADF)
}

/// Host paths at or below `/proc` and `/dev`: the kernel's own trees. They are the same for every
/// process, so a confined view (a cell) must not add, replace or remove entries there – such an entry
/// would land in the host's global tree, visible to the host and to every other cell.
fn kernel_tree(host: &str) -> bool {
    ["/proc", "/dev"].iter().any(|d| crate::isolation::mount::below(host, d).is_some())
}

/// Largest size of one file held in memory (RAM layer, tmpfs, descriptor buffers). A size beyond it
/// is refused with `EFBIG` instead of asking the kernel heap for it (an allocation that fails stops
/// the whole machine).
const MAX_FILE_BYTES: u64 = 64 << 20;
/// End of the user half of the address space (canonical lower half).
const USER_TOP: u64 = 0x8000_0000_0000;
const EFBIG: i64 = 27;

fn stat_buf(mode: u32, size: u64, ino: u64, rdev: u64) -> Vec<u8> {
    let mut b = vec![0u8; 144];
    b[0..8].copy_from_slice(&1u64.to_le_bytes());
    b[8..16].copy_from_slice(&ino.to_le_bytes());
    b[16..24].copy_from_slice(&1u64.to_le_bytes());
    b[24..28].copy_from_slice(&mode.to_le_bytes());
    b[40..48].copy_from_slice(&rdev.to_le_bytes());
    b[48..56].copy_from_slice(&size.to_le_bytes());
    b[56..64].copy_from_slice(&4096u64.to_le_bytes());
    b[64..72].copy_from_slice(&size.div_ceil(512).to_le_bytes());
    let t = now_ns(0) / 1_000_000_000;
    for o in [72usize, 88, 104] {
        b[o..o + 8].copy_from_slice(&t.to_le_bytes());
    }
    b
}

/// `struct stat` of the entry at host path `host` with its real mode, owner and group (perm.rs);
/// `fallback` is the mode of an entry that has gone meanwhile.
fn stat_entry(host: &str, fallback: u32, size: u64) -> Vec<u8> {
    let a = crate::perm::of(host).unwrap_or(crate::perm::Attr { mode: fallback, uid: 0, gid: 0 });
    let mut b = stat_buf(a.mode, size, ino(host), crate::perm::rdev(host));
    b[28..32].copy_from_slice(&a.uid.to_le_bytes());
    b[32..36].copy_from_slice(&a.gid.to_le_bytes());
    b
}

/// `fsync`, `fdatasync`. A file's data lives in the descriptor's buffer until it is stored: this
/// stores it now – on a writable ext4 that writes the blocks to the disk before the call returns
/// (every disk write of this kernel is synchronous), in the RAM layer or a tmpfs it makes the data
/// visible to others and is "durable" only as far as memory is. Pipes and sockets: `EINVAL`.
fn sys_fsync(fd: u64) -> i64 {
    if let Some(r) = crate::rawpart::sys_fsync(fd) {
        return r;
    }
    let job = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::File { data, path, write: true, dirty: true, .. }) => Ok(Some((path.clone(), data.clone()))),
        Some(FdKind::File { .. }) | Some(FdKind::Dir { .. }) => Ok(None),
        Some(_) => Err(-EINVAL),
        None => Err(-EBADF),
    })
    .unwrap_or(Err(-EBADF));
    let (path, data) = match job {
        Ok(Some(j)) => j,
        Ok(None) => return 0,
        Err(e) => return e,
    };
    if let Err(e) = fs::store_file_res(&path, data) {
        return e;
    }
    // stored: nothing left to write back until the next write
    with(|p| {
        if let Some(FdKind::File { dirty, .. }) = p.fd(fd).map(|d| &mut d.kind) {
            *dirty = false;
        }
    });
    0
}

/// `close`: a written file is stored first, and an error doing so is the result (`EIO`,
/// `ENOSPC`) – the data is then not on the medium, and the caller must know.
fn sys_close(fd: u64) -> i64 {
    let taken = with(|p| p.g.fds.get_mut(fd as u32 as usize).and_then(Option::take)).flatten();
    let Some(mut f) = taken else { return -EBADF };
    let job = match &mut f.kind {
        FdKind::File { data, path, write: true, dirty, .. } if *dirty => {
            // stored here, not again when the descriptor is dropped
            *dirty = false;
            Some((path.clone(), data.clone()))
        }
        _ => None,
    };
    drop(f);
    match job {
        Some((path, data)) => fs::store_file_res(&path, data).err().unwrap_or(0),
        None => 0,
    }
}

fn ino(path: &str) -> u64 {
    path.bytes().fold(1469598103934665603u64, |h, c| (h ^ c as u64).wrapping_mul(1099511628211)) | 1
}

fn sys_stat_path(dirfd: i64, path: u64, buf: u64, flags: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    if p.is_empty() && flags & 0x1000 != 0 {
        return sys_fstat(dirfd as u64, buf);
    }
    let loc = match locate(dirfd, &p, flags & 0x100 == 0) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let path = loc.host.clone();
    if flags & 0x100 != 0 {
        if let Some(target) = link_at(&loc) {
            return if put(buf, &stat_entry(&path, 0o120777, target.len() as u64)) { 0 } else { -EFAULT };
        }
    }
    let b = match fs::stat(&path) {
        Some((0, size)) => stat_entry(&path, 0o100755, size),
        Some((1, _)) => stat_entry(&path, 0o040755, 4096),
        Some(_) => stat_entry(&path, 0o020666, 0),
        None => return -ENOENT,
    };
    if put(buf, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_fstat(fd: u64, buf: u64) -> i64 {
    // a file's attributes are looked up after the process lock is released (a look into /proc takes it)
    let b = with(|p| {
        let d = p.fd(fd)?;
        Some(match &d.kind {
            FdKind::Console => Ok(stat_buf(0o020620, 0, 3, (136 << 8) | 0)),
            FdKind::Null | FdKind::Zero | FdKind::Random => Ok(stat_buf(0o020666, 0, 4, (1 << 8) | 3)),
            FdKind::File { data, path, .. } => Err((path.clone(), 0o100755, data.len() as u64)),
            // a memory object: its size, and as many 512-byte blocks as its pages hold (all of it is allocated)
            FdKind::Mem(m, _) => {
                let mut b = stat_buf(0o100600, m.size(), 13, 0);
                b[64..72].copy_from_slice(&(m.pages() * 8).to_le_bytes());
                Ok(b)
            }
            FdKind::Dir { path, .. } => Err((path.clone(), 0o040755, 4096)),
            FdKind::PipeR(_) | FdKind::PipeW(_) => Ok(stat_buf(0o010600, 0, 5, 0)),
            FdKind::Sock(h) => Ok(stat_buf(0o140777, 0, 6 + h.0 as u64, 0)),
            FdKind::SigFd(_) => Ok(stat_buf(0o000600, 0, 7, 0)),
            FdKind::UnixConn(..) | FdKind::UnixSock(_) => Ok(stat_buf(0o140777, 0, 8, 0)),
            FdKind::Epoll(_) | FdKind::EventFd(..) | FdKind::TimerFd(_) => Ok(stat_buf(0o000600, 0, 9, 0)),
            FdKind::Fb(_) => Err((String::from("/dev/fb0"), 0o020600, 0)),
            FdKind::Eingabe(e) => Err((alloc::format!("/dev/input/event{}", e.0), 0o020600, 0)),
            FdKind::Ns(_) => Ok(stat_buf(0o100444, 0, 12, 0)),
            FdKind::RawPart(r) => Ok(stat_buf(0o060600, r.size(), 13, 0)),
            FdKind::Hci(_) | FdKind::Uinput(_) => Ok(stat_buf(0o020600, 0, 14, 0)),
            FdKind::Dri(_) => Ok(stat_buf(0o020600, 0, 14, crate::svcdev::rdev(crate::svcdev::dri::NODE).unwrap_or(0))),
        })
    })
    .flatten();
    match b.map(|r| r.unwrap_or_else(|(path, mode, size)| stat_entry(&path, mode, size))) {
        Some(b) => {
            if put(buf, &b) {
                0
            } else {
                -EFAULT
            }
        }
        None => -EBADF,
    }
}

fn sys_lseek(fd: u64, off: i64, wh: u64) -> i64 {
    if let Some(r) = crate::rawpart::sys_lseek(fd, off, wh) {
        return r;
    }
    with(|p| {
        let Some(d) = p.fd(fd) else { return -EBADF };
        match &mut d.kind {
            FdKind::File { data, pos, .. } => {
                let base = match wh {
                    0 => 0,
                    1 => *pos as i64,
                    2 => data.len() as i64,
                    _ => return -EINVAL,
                };
                let n = base + off;
                if n < 0 {
                    return -EINVAL;
                }
                *pos = n as u64;
                n
            }
            FdKind::Dir { pos, .. } => {
                *pos = off.max(0) as usize;
                off
            }
            FdKind::Mem(m, pos) => {
                let base = match wh {
                    0 => 0,
                    1 => *pos as i64,
                    2 => m.size() as i64,
                    _ => return -EINVAL,
                };
                let n = base.saturating_add(off);
                if n < 0 {
                    return -EINVAL;
                }
                *pos = n as u64;
                n
            }
            FdKind::Null | FdKind::Zero | FdKind::Random => 0,
            _ => -ESPIPE,
        }
    })
    .unwrap_or(-EBADF)
}

fn sys_getdents(fd: u64, buf: u64, count: u64) -> i64 {
    // one call fills at most 64 KiB (the caller asks again for the rest)
    let count = count.min(65536) as usize;
    let out = with(|p| {
        let Some(d) = p.fd(fd) else { return Err(-EBADF) };
        let FdKind::Dir { ents, pos, path, .. } = &mut d.kind else { return Err(-ENOTDIR) };
        let Some(mut out) = crate::mem::try_with_capacity::<u8>(count) else { return Err(-ENOMEM) };
        // entries 0 and 1 are "." and "..", then the listing
        let total = ents.len() + 2;
        while *pos < total {
            let (name, t) = match *pos {
                0 => (".", 4u8),
                1 => ("..", 4),
                i => (ents[i - 2].name.as_str(), if ents[i - 2].dir { 4 } else { 8 }),
            };
            let reclen = (19 + name.len() + 1 + 7) & !7;
            if out.len() + reclen > count {
                break;
            }
            let start = out.len();
            out.resize(start + reclen, 0);
            let r = &mut out[start..];
            r[0..8].copy_from_slice(&ino(&alloc::format!("{path}/{name}")).to_le_bytes());
            r[8..16].copy_from_slice(&((*pos + 1) as u64).to_le_bytes());
            r[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
            r[18] = t;
            r[19..19 + name.len()].copy_from_slice(name.as_bytes());
            *pos += 1;
        }
        if out.is_empty() && *pos < total {
            return Err(-EINVAL);
        }
        Ok(out)
    })
    .unwrap_or(Err(-EBADF));
    match out {
        Ok(v) => {
            if put(buf, &v) || v.is_empty() {
                v.len() as i64
            } else {
                -EFAULT
            }
        }
        Err(e) => e,
    }
}

fn sys_readlink(dirfd: i64, path: u64, buf: u64, n: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    let loc = match locate(dirfd, &p, false) {
        Ok(l) => l,
        Err(e) => return e,
    };
    // /proc/<pid>/exe (own process) and /proc/<pid>/ns/*; otherwise a symbolic link of the file tree
    let entry = if loc.host.starts_with("/proc/") { crate::procfs::pid_entry(&loc.host) } else { None };
    let t = match entry {
        Some((tgid, "exe")) if tgid == proc::current_group() => with(|p| p.g.exe.clone()).unwrap_or_default(),
        Some((tgid, "ns/mnt")) | Some((tgid, "ns/pid")) => match crate::procfs::ns_name(tgid, &loc.host[loc.host.len() - 3..]) {
            Some(name) => name,
            None => return -ENOENT,
        },
        Some(_) => return -EINVAL,
        None => match link_at(&loc) {
            Some(target) => target,
            None => return -EINVAL,
        },
    };
    let b = &t.as_bytes()[..t.len().min(n as usize)];
    if put(buf, b) {
        b.len() as i64
    } else {
        -EFAULT
    }
}

fn sys_sendfile(out: u64, inp: u64, offp: u64, count: u64) -> Res {
    // the input file's buffer is shared, not copied
    let src = with(|p| match p.fd(inp).map(|d| &d.kind) {
        Some(FdKind::File { data, pos, .. }) => Some((data.clone(), *pos)),
        _ => None,
    })
    .flatten();
    let Some((data, pos)) = src else { return Res::Val(-EINVAL) };
    let s = (pos as usize).min(data.len());
    let e = s.saturating_add(count.min(1 << 24) as usize).min(data.len());
    if s == e {
        return Res::Val(0);
    }
    let r = write_bytes(out, &data[s..e]);
    // the position moves by what was really written
    if let (Res::Val(n), true) = (&r, offp == 0) {
        if *n > 0 {
            with(|p| {
                if let Some(FdKind::File { pos, .. }) = p.fd(inp).map(|d| &mut d.kind) {
                    *pos = s as u64 + *n as u64;
                }
            });
        }
    }
    r
}

/// Bytes aus dem Kern an einen Dateideskriptor schreiben. A full pipe takes what fits (short
/// count); if nothing fits the caller waits and repeats the call, or gets `EAGAIN`.
fn write_bytes(fd: u64, data: &[u8]) -> Res {
    let r = with(|p| {
        let Some(d) = p.fd(fd) else { return Err(Res::Val(-EBADF)) };
        let nonblock = d.flags & 0x800 != 0;
        match &mut d.kind {
            FdKind::Console => Ok(None),
            FdKind::Null => Ok(Some(data.len())),
            FdKind::File { data: fd_data, pos, write: true, dirty, path, .. } => match file_write(fd_data, pos, path, data) {
                Ok(()) => {
                    *dirty = true;
                    Ok(Some(data.len()))
                }
                Err(e) => Err(Res::Val(e)),
            },
            FdKind::PipeW(pp) | FdKind::UnixConn(_, pp) => {
                if pp.read_closed() {
                    p.pending |= 1 << (crate::signal::SIGPIPE - 1);
                    return Err(Res::Val(-EPIPE));
                }
                pipe_write(pp, data, nonblock).map(Some)
            }
            _ => Err(Res::Val(-EBADF)),
        }
    })
    .unwrap_or(Err(Res::Val(-EBADF)));
    match r {
        Ok(None) => {
            crate::heap::kernel(|| echo(data));
            Res::Val(data.len() as i64)
        }
        Ok(Some(n)) => Res::Val(n as i64),
        Err(r) => r,
    }
}

/// `statfs(path, buf)` / `fstatfs(fd, buf)`: a path in a RAM tree (tmpfs, the host's RAM layer)
/// reports that tree's quota and use; everything else the machine's memory as before.
fn sys_statfs(by_fd: bool, what: u64, buf: u64) -> i64 {
    let host = if by_fd {
        with(|p| match p.fd(what).map(|d| &d.kind) {
            Some(FdKind::File { path, .. }) | Some(FdKind::Dir { path, .. }) => Some(path.clone()),
            _ => None,
        })
        .flatten()
    } else {
        ustr(what).and_then(|p| locate(-100, &p, true).ok()).map(|l| l.host)
    };
    let (mut total, mut free, mut kind) = (crate::mem::total_bytes(), crate::mem::free_bytes(), 0xEF53u64);
    if let Some((used, quota)) = host.as_deref().and_then(fs::ram_usage) {
        (total, free, kind) = (quota, quota.saturating_sub(used), 0x0102_1994);
    }
    let mut b = vec![0u8; 120];
    let w = |b: &mut Vec<u8>, o: usize, v: u64| b[o..o + 8].copy_from_slice(&v.to_le_bytes());
    w(&mut b, 0, kind);
    w(&mut b, 8, 4096);
    w(&mut b, 16, total / 4096);
    w(&mut b, 24, free / 4096);
    w(&mut b, 32, free / 4096);
    w(&mut b, 40, 1 << 20);
    w(&mut b, 48, 1 << 19);
    w(&mut b, 64, 255);
    w(&mut b, 72, 4096);
    if put(buf, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_rename(od: i64, op: u64, nd: i64, np: u64) -> i64 {
    let (Some(o), Some(n)) = (ustr(op), ustr(np)) else { return -EFAULT };
    let (ol, nl) = match (locate(od, &o, false), locate(nd, &n, false)) {
        (Ok(o), Ok(n)) => (o, n),
        (Err(e), _) | (_, Err(e)) => return e,
    };
    if ol.read_only || nl.read_only {
        return -EROFS;
    }
    // nothing can be renamed into /proc or /dev (a rename must not reach the kernel's control files)
    if kernel_tree(&nl.host) || ol.confined && kernel_tree(&ol.host) {
        return -EPERM;
    }
    if let Err(e) = crate::perm::may_remove(&ol.host).and_then(|_| crate::perm::may_remove(&nl.host)) {
        return e;
    }
    // a symbolic link is moved as a link (links exist on tmpfs only)
    if let Some(target) = link_at(&ol) {
        if !crate::isolation::tmpfs::is_private(&nl.host) {
            return -EXDEV;
        }
        fs::unlink(&nl.host);
        if !crate::isolation::tmpfs::symlink(&nl.host, &target) {
            return -EEXIST;
        }
        crate::perm::moved(&ol.host, &nl.host);
        fs::unlink(&ol.host);
        return 0;
    }
    // a link at the new name is replaced, not written through
    let replaced_link = link_at(&nl).is_some();
    let (o, n) = (ol.host, nl.host);
    let Some(data) = fs::read_all(&o) else { return -ENOENT };
    if replaced_link {
        fs::unlink(&n);
    }
    if let Err(e) = fs::store_file_res(&n, data) {
        return e;
    }
    // mode and owner go with the entry (before the old name – and its record – is removed)
    crate::perm::moved(&o, &n);
    if !fs::unlink(&o) {
        return -EROFS;
    }
    0
}

fn sys_ftruncate(fd: u64, len: u64) -> i64 {
    let obj = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::Mem(m, _)) => Some(m.clone()),
        _ => None,
    })
    .flatten();
    if let Some(m) = obj {
        // the pages are charged to the caller's group (set for this call)
        return m.set_size(len).map_or_else(|e| e, |()| 0);
    }
    with(|p| match p.fd(fd).map(|d| &mut d.kind) {
        Some(FdKind::File { write: true, .. }) if len > MAX_FILE_BYTES => -EFBIG,
        Some(FdKind::File { data, write: true, dirty, path, .. }) => {
            if len as usize > data.len() && !fs::grow(path, len) {
                return -ENOSPC;
            }
            match file_room(data, len as usize) {
                Some(v) => {
                    v.resize(len as usize, 0);
                    *dirty = true;
                    0
                }
                None => -ENOMEM,
            }
        }
        _ => -EINVAL,
    })
    .unwrap_or(-EBADF)
}

fn sys_mkdir(dirfd: i64, path: u64, mode: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    match locate(dirfd, &p, false) {
        Ok(loc) if loc.read_only => -EROFS,
        Ok(loc) if loc.confined && kernel_tree(&loc.host) => -EPERM,
        Ok(loc) if fs::stat(&loc.host).is_none() && crate::perm::may_create(&loc.host).is_err() => -EACCES,
        Ok(loc) if fs::mkdir(&loc.host) => {
            crate::perm::created(&loc.host, crate::perm::S_IFDIR, mode as u32);
            0
        }
        Ok(_) => -EEXIST,
        Err(e) => e,
    }
}

fn sys_unlink(dirfd: i64, path: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    let loc = match locate(dirfd, &p, false) {
        Ok(l) => l,
        Err(e) => return e,
    };
    if loc.read_only {
        return -EROFS;
    }
    if loc.confined && kernel_tree(&loc.host) {
        return -EPERM;
    }
    let path = loc.host;
    if let Err(e) = crate::perm::may_remove(&path) {
        return e;
    }
    if fs::unlink(&path) {
        crate::perm::forget(&path);
        0
    } else if fs::stat(&path).is_some() {
        -EROFS
    } else {
        -ENOENT
    }
}

fn sys_getcwd(buf: u64, size: u64) -> i64 {
    let cwd = with(|p| p.g.cwd.clone()).unwrap_or_default();
    if cwd.len() + 1 > size as usize {
        return -ERANGE;
    }
    let mut b = cwd.into_bytes();
    b.push(0);
    if put(buf, &b) {
        b.len() as i64
    } else {
        -EFAULT
    }
}

fn sys_chdir(path: u64) -> i64 {
    let Some(p) = ustr(path) else { return -EFAULT };
    let loc = match locate(-100, &p, true) {
        Ok(l) => l,
        Err(e) => return e,
    };
    match fs::stat(&loc.host) {
        Some((1, _)) if crate::perm::check(&loc.host, crate::perm::X).is_err() => -EACCES,
        Some((1, _)) => {
            with(|p| p.g.cwd = loc.view);
            0
        }
        Some(_) => -ENOTDIR,
        None => -ENOENT,
    }
}

fn sys_dup(old: u64, new: Option<u64>, flags: u64) -> i64 {
    with(|p| {
        let Some(d) = p.fd(old) else { return -EBADF };
        let mut n = d.dup();
        n.cloexec = flags & 0x80000 != 0;
        match new {
            Some(nf) => {
                let (nf, old) = (nf as u32 as u64, old as u32 as u64);
                if nf == old {
                    return nf as i64;
                }
                let nf = nf as usize;
                if nf >= 65536 {
                    return -EBADF;
                }
                if nf >= p.g.fds.len() {
                    let more = nf + 1 - p.g.fds.len();
                    if !crate::mem::try_reserve(&mut p.g.fds, more) {
                        return -ENOMEM;
                    }
                    p.g.fds.resize_with(nf + 1, || None);
                }
                p.g.fds[nf] = Some(n);
                nf as i64
            }
            None => p.add_fd(n, 0),
        }
    })
    .unwrap_or(-EBADF)
}

fn sys_fcntl(fd: u64, cmd: u64, arg: u64) -> i64 {
    match cmd {
        // the lowest number asked for is bounded like dup2 (the table is a vector indexed by number)
        0 | 1030 if arg >= 65536 => -EINVAL,
        0 | 1030 => with(|p| {
            let Some(d) = p.fd(fd) else { return -EBADF };
            let mut n = d.dup();
            n.cloexec = cmd == 1030;
            p.add_fd(n, arg as usize)
        })
        .unwrap_or(-EBADF),
        1 => with(|p| p.fd(fd).map(|d| d.cloexec as i64).unwrap_or(-EBADF)).unwrap_or(-EBADF),
        2 => with(|p| p.fd(fd).map(|d| {
            d.cloexec = arg & 1 != 0;
            0
        }).unwrap_or(-EBADF))
        .unwrap_or(-EBADF),
        3 => with(|p| p.fd(fd).map(|d| d.flags as i64).unwrap_or(-EBADF)).unwrap_or(-EBADF),
        4 => {
            if let Some(id) = sock_id(fd) {
                crate::socket::set_nonblock(id, arg & 0x800 != 0);
            }
            with(|p| p.fd(fd).map(|d| {
                d.flags = (d.flags & 3) | (arg as u32 & !3);
                0
            }).unwrap_or(-EBADF))
            .unwrap_or(-EBADF)
        }
        // F_ADD_SEALS / F_GET_SEALS of a memory object
        1033 | 1034 => {
            let obj = with(|p| match p.fd(fd).map(|d| &d.kind) {
                Some(FdKind::Mem(m, _)) => Ok(m.clone()),
                Some(_) => Err(-EINVAL),
                None => Err(-EBADF),
            })
            .unwrap_or(Err(-EBADF));
            match obj {
                Ok(m) if cmd == 1034 => m.seals() as i64,
                Ok(m) => m.add_seals(arg as u32).map_or_else(|e| e, |()| 0),
                Err(e) => e,
            }
        }
        // F_SETPIPE_SZ / F_GETPIPE_SZ
        1031 | 1032 => {
            let pipe = with(|p| match p.fd(fd).map(|d| &d.kind) {
                Some(FdKind::PipeR(pp)) | Some(FdKind::PipeW(pp)) => Some(pp.clone()),
                _ => None,
            })
            .flatten();
            let Some(pipe) = pipe else { return -EBADF };
            if cmd == 1032 {
                return pipe.capacity() as i64;
            }
            // above the configured maximum only with the right to manage resource groups
            if arg > crate::pipe::PIPE_MAX_CEILING || arg > crate::pipe::max_size() && !calls::holds(Rights::MANAGE_GROUPS) {
                return -EPERM;
            }
            match pipe.set_capacity(arg) {
                Some(cap) => cap as i64,
                None => -16,
            }
        }
        _ => 0,
    }
}

fn sys_pipe(fds: u64, flags: u64) -> i64 {
    // both ends count as open from the start: a descriptor that cannot be placed is dropped and
    // gives its count back
    let p = Pipe::new();
    p.readers.store(1, Ordering::SeqCst);
    p.writers.store(1, Ordering::SeqCst);
    let cl = flags & 0x80000 != 0;
    let (r, w) = with(|pr| {
        // O_NONBLOCK (0x800) aus pipe2 übernehmen
        let nb = (flags & 0x800) as u32;
        let r = pr.add_fd(Fd { kind: FdKind::PipeR(p.clone()), cloexec: cl, flags: nb }, 0);
        if r < 0 {
            return (r, r);
        }
        let w = pr.add_fd(Fd { kind: FdKind::PipeW(p.clone()), cloexec: cl, flags: 1 | nb }, 0);
        if w < 0 {
            pr.g.fds[r as usize] = None;
            return (w, w);
        }
        (r, w)
    })
    .unwrap_or((-EBADF, -EBADF));
    if r < 0 {
        return r;
    }
    let mut b = Vec::new();
    b.extend_from_slice(&(r as i32).to_le_bytes());
    b.extend_from_slice(&(w as i32).to_le_bytes());
    if put(fds, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_ioctl(fd: u64, req: u64, arg: u64) -> i64 {
    if let Some(id) = sock_id(fd) {
        return match req {
            0x541B => {
                let n = match crate::socket::recv(id, 1 << 20, true) {
                    crate::socket::Recv::Data(d, _) => d.len() as u32,
                    _ => 0,
                };
                if put(arg, &n.to_le_bytes()) {
                    0
                } else {
                    -EFAULT
                }
            }
            0x5421 => {
                let v = get(arg, 4).map(|b| b != [0, 0, 0, 0]).unwrap_or(false);
                crate::socket::set_nonblock(id, v);
                0
            }
            _ => -ENOTTY,
        };
    }
    // für alle Deskriptoren: FIOCLEX/FIONCLEX (close-on-exec), FIONBIO (nicht blockierend)
    match req {
        0x5451 | 0x5450 => return with(|p| p.fd(fd).map(|d| d.cloexec = req == 0x5451).map(|_| 0).unwrap_or(-EBADF)).unwrap_or(-EBADF),
        0x5421 => {
            let v = get(arg, 4).map(|b| b != [0, 0, 0, 0]).unwrap_or(false);
            return with(|p| p.fd(fd).map(|d| if v { d.flags |= 0x800 } else { d.flags &= !0x800 }).map(|_| 0).unwrap_or(-EBADF)).unwrap_or(-EBADF);
        }
        _ => {}
    }
    if let Some(r) = crate::svcdev::uinput::sys_ioctl(fd, req, arg) {
        return r;
    }
    if let Some(r) = crate::svcdev::dri::sys_ioctl(fd, req, arg) {
        return r;
    }
    let geraet = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::Fb(_)) => Some(None),
        Some(FdKind::Eingabe(e)) => Some(Some(e.0)),
        _ => None,
    })
    .flatten();
    if let Some(g) = geraet {
        let antwort = match g {
            None => crate::eingabe::fb_ioctl(req),
            Some(art) => crate::eingabe::ioctl(art, req),
        };
        return match antwort {
            Some(b) if b.is_empty() => 0,
            Some(b) => {
                // variable-length requests (EVIOCGNAME, EVIOCGBIT, …): at most the length in the request
                let var = crate::input::returns_len(req);
                let max = if var { ((req >> 16) & 0x3fff) as usize } else { b.len() };
                let b = &b[..b.len().min(max)];
                if put(arg, b) {
                    if var { b.len() as i64 } else { 0 }
                } else {
                    -EFAULT
                }
            }
            None => -EINVAL,
        };
    }
    let console = with(|p| matches!(p.fd(fd).map(|d| &d.kind), Some(FdKind::Console))).unwrap_or(false);
    if !console {
        return if with(|p| p.fd(fd).is_some()).unwrap_or(false) { -ENOTTY } else { -EBADF };
    }
    match req {
        0x5401 => {
            let t = TTY.lock();
            let mut b = vec![0u8; 36];
            b[0..4].copy_from_slice(&0x500u32.to_le_bytes()); // ICRNL|IXON
            b[4..8].copy_from_slice(&0x5u32.to_le_bytes()); // OPOST|ONLCR
            b[8..12].copy_from_slice(&0xbfu32.to_le_bytes()); // B38400|CS8|CREAD
            let lflag = 0x8a31u32 & !(if t.icanon { 0 } else { 2 }) & !(if t.echo { 0 } else { 8 }); // ISIG|ICANON|ECHO…
            b[12..16].copy_from_slice(&lflag.to_le_bytes());
            let cc = [3u8, 28, 127, 21, 4, 0, 1, 0, 17, 19, 26, 0, 18, 15, 23, 22];
            b[17..17 + cc.len()].copy_from_slice(&cc);
            drop(t);
            if put(arg, &b) {
                0
            } else {
                -EFAULT
            }
        }
        0x5402..=0x5404 => {
            let Some(b) = get(arg, 36) else { return -EFAULT };
            let lflag = u32::from_le_bytes(b[12..16].try_into().unwrap());
            let mut t = TTY.lock();
            t.icanon = lflag & 2 != 0;
            t.echo = lflag & 8 != 0;
            if !t.icanon {
                let l = core::mem::take(&mut t.line);
                t.ready.extend(l);
            }
            0
        }
        0x5413 => {
            let (cols, rows) = crate::console::size();
            let mut b = vec![0u8; 8];
            b[0..2].copy_from_slice(&(rows as u16).to_le_bytes());
            b[2..4].copy_from_slice(&(cols as u16).to_le_bytes());
            if put(arg, &b) {
                0
            } else {
                -EFAULT
            }
        }
        0x540F => {
            let g = with(|p| p.pgid).unwrap_or(0) as u32;
            put(arg, &g.to_le_bytes());
            0
        }
        0x5410 | 0x5414 | 0x540E => 0,
        _ => -EINVAL,
    }
}

/// Signals pending for the calling process: its own thread's and, for a signal sent to the whole
/// process, those parked at any other thread (`signal::send_locked` puts a signal that every thread
/// blocks on one of them) – what a `signalfd` sees, in whichever thread it is read.
fn group_pending(ps: &[alloc::boxed::Box<Proc>], me: u64) -> u64 {
    let Some(tgid) = ps.iter().find(|p| p.pid == me).map(|p| p.tgid) else { return 0 };
    ps.iter().filter(|p| p.tgid == tgid && !matches!(p.state, proc::State::Zombie(_) | proc::State::Dead)).fold(0, |a, p| a | p.pending)
}

/// Take the lowest signal of `mask` pending for the calling process (own thread first); 0: none.
fn take_signal(mask: u64) -> u64 {
    let mut ps = proc::PROCS.lock();
    let me = proc::current();
    let Some(tgid) = ps.iter().find(|p| p.pid == me).map(|p| p.tgid) else { return 0 };
    let live = |p: &Proc| p.tgid == tgid && !matches!(p.state, proc::State::Zombie(_) | proc::State::Dead);
    let Some(i) = ps.iter().position(|p| p.pid == me && p.pending & mask != 0).or_else(|| ps.iter().position(|p| live(p) && p.pending & mask != 0)) else { return 0 };
    let sig = (ps[i].pending & mask).trailing_zeros() as u64 + 1;
    ps[i].pending &= !(1 << (sig - 1));
    sig
}

/// [`fd_events`] for the calling thread, with the signals of the whole process for a `signalfd`.
fn ready_of(fd: i64, ev: u32, inp: bool) -> u32 {
    let mut ps = proc::PROCS.lock();
    let me = proc::current();
    let pending = group_pending(&ps, me);
    ps.iter_mut().find(|p| p.pid == me).map(|p| fd_events(p, fd, ev, inp, pending)).unwrap_or(0x20)
}

/// Bereitschaft eines Deskriptors (POLLIN 1, POLLOUT 4, POLLHUP 0x10, POLLNVAL 0x20)
fn fd_events(p: &mut Proc, fd: i64, ev: u32, inp: bool, pending: u64) -> u32 {
    let Some(d) = p.fd(fd as u64) else { return 0x20 };
    let r = |b: bool| (ev & 1) * b as u32;
    let w = |b: bool| (ev & 4) * b as u32;
    match &d.kind {
        FdKind::Console => r(inp) | w(true),
        FdKind::PipeR(pp) => r(!pp.buf.lock().is_empty() || pp.writers.load(Ordering::SeqCst) == 0) | if pp.writers.load(Ordering::SeqCst) == 0 { 0x10 } else { 0 },
        FdKind::PipeW(pp) => w(pp.writable()) | if pp.read_closed() { 8 } else { 0 },
        FdKind::Sock(h) => {
            let (rr, ww) = crate::socket::readiness(h.0);
            r(rr) | w(ww)
        }
        FdKind::SigFd(m) => r(pending & *m != 0),
        // end of file: readable; POLLRDHUP when the peer stopped sending (or we shut reading down),
        // POLLHUP when both directions are closed
        FdKind::UnixConn(rx, tx) => {
            r(!rx.buf.lock().is_empty() || rx.write_closed())
                | w(!tx.read_closed() && tx.writable())
                | if rx.write_closed() { ev & 0x2000 } else { 0 }
                | if rx.write_closed() && tx.read_closed() { 0x10 } else { 0 }
        }
        FdKind::UnixSock(us) => r(matches!(&*us.lock(), proc::UnixState::Listening(_, q) if !q.is_empty())),
        FdKind::EventFd(c, _) => r(*c.lock() > 0) | w(true),
        FdKind::TimerFd(t) => {
            let (dl, _) = *t.lock();
            r(dl != 0 && crate::arch::uptime_ms() >= dl)
        }
        FdKind::Epoll(_) => 0,
        FdKind::Eingabe(e) => r(crate::eingabe::bereit(e.0, e.2)),
        FdKind::Hci(h) => r(crate::svcdev::hci::ready(h)) | w(true),
        FdKind::Uinput(_) => w(true),
        FdKind::Fb(_) => w(true),
        _ => ev & 5,
    }
}

/// früheste Frist eines timerfd unter den Deskriptoren (für Wartezeiten)
fn timer_deadline(p: &mut Proc, fds: &[i64]) -> u64 {
    let mut best = 0u64;
    for &fd in fds {
        if let Some(Fd { kind: FdKind::TimerFd(t), .. }) = p.fd(fd as u64) {
            let dl = t.lock().0;
            if dl != 0 && (best == 0 || dl < best) {
                best = dl;
            }
        }
        // Eingabegeräte werden abgefragt (kein Interrupt): alle 8 ms neu prüfen
        if let Some(Fd { kind: FdKind::Eingabe(_) | FdKind::Hci(_), .. }) = p.fd(fd as u64) {
            let dl = crate::arch::uptime_ms() + 8;
            if best == 0 || dl < best {
                best = dl;
            }
        }
    }
    best
}

fn sys_epoll_create(flags: u64) -> i64 {
    with(|p| p.add_fd(Fd { kind: FdKind::Epoll(Arc::new(spin::Mutex::new(Vec::new()))), cloexec: flags & 0x80000 != 0, flags: 2 }, 0)).unwrap_or(-EBADF)
}

fn sys_epoll_ctl(epfd: u64, op: u64, fd: i64, ev: u64) -> i64 {
    let e = with(|p| match p.fd(epfd).map(|d| &d.kind) {
        Some(FdKind::Epoll(e)) => Some(e.clone()),
        _ => None,
    })
    .flatten();
    let Some(e) = e else { return -EINVAL };
    let (events, data) = if ev != 0 {
        match get(ev, 12) {
            Some(b) => (u32::from_le_bytes(b[0..4].try_into().unwrap()), u64::from_le_bytes(b[4..12].try_into().unwrap())),
            None => return -EFAULT,
        }
    } else {
        (0, 0)
    };
    let mut l = e.lock();
    let pos = l.iter().position(|x| x.0 as i64 == fd);
    match (op, pos) {
        (1, None) => {
            if l.len() >= proc::MAX_FDS || !crate::mem::try_reserve(&mut l, 1) {
                return -ENOMEM;
            }
            l.push((fd as i32, events, data))
        }
        (1, Some(_)) => return -EEXIST,
        (2, Some(i)) => {
            l.remove(i);
        }
        (3, Some(i)) => l[i] = (fd as i32, events, data),
        (2 | 3, None) => return -ENOENT,
        _ => return -EINVAL,
    }
    0
}

fn sys_epoll_wait(epfd: u64, out: u64, max: u64, timeout: i64) -> Res {
    let e = with(|p| match p.fd(epfd).map(|d| &d.kind) {
        Some(FdKind::Epoll(e)) => Some(e.clone()),
        _ => None,
    })
    .flatten();
    let Some(e) = e else { return Res::Val(-EINVAL) };
    // the event counter before the descriptors are looked at: a change in between wakes the wait at once
    let seen = crate::socket::gen();
    poll_input();
    crate::socket::poll();
    let inp = input_ready();
    let list = e.lock().clone();
    let mut buf = Vec::new();
    let mut n = 0;
    let mut oneshot = Vec::new();
    for &(fd, events, data) in &list {
        if n >= max.min(1024) {
            break;
        }
        let r = ready_of(fd as i64, events | 0x18, inp);
        let r = r & (events | 0x18);
        if r != 0 {
            buf.extend_from_slice(&r.to_le_bytes());
            buf.extend_from_slice(&data.to_le_bytes());
            n += 1;
            if events & (1 << 30) != 0 {
                oneshot.push(fd);
            }
        }
    }
    if !oneshot.is_empty() {
        let mut l = e.lock();
        for x in l.iter_mut().filter(|x| oneshot.contains(&x.0)) {
            x.1 = 0;
        }
    }
    if n > 0 || timeout == 0 {
        poll_done();
        return Res::Val(if n == 0 || put(out, &buf) { n as i64 } else { -EFAULT });
    }
    let fds: Vec<i64> = list.iter().map(|x| x.0 as i64).collect();
    let tdl = with(|p| timer_deadline(p, &fds)).unwrap_or(0);
    poll_wait(timeout, tdl, seen)
}

/// Warteentscheidung für poll/select/epoll_wait: bereit → None; sonst wiederholbares Warten bis Ereignis/Frist.
/// `timeout` < 0 = unbegrenzt. Die Frist gilt für den ganzen Aufruf (über Wiederholungen hinweg).
/// Wait for the next event after `seen` (the counter read before the caller looked at its descriptors – a
/// buffer drained or filled while it looked is not missed), the timeout or a timer.
fn poll_wait(timeout: i64, timer_dl: u64, seen: u64) -> Res {
    let g = seen;
    let now = crate::arch::uptime_ms();
    let rip = crate::percpu::me().sys_rip;
    let dl = if timeout >= 0 {
        with(|p| match p.poll_dl {
            Some((r, d)) if r == rip => d,
            _ => {
                let d = now + timeout as u64;
                p.poll_dl = Some((rip, d));
                d
            }
        })
        .unwrap_or(now)
    } else {
        0
    };
    if dl != 0 && now >= dl {
        with(|p| p.poll_dl = None);
        return Res::Val(0);
    }
    let mut w = dl;
    if timer_dl != 0 && (w == 0 || timer_dl < w) {
        w = timer_dl;
    }
    Res::Block(Wait::Poll(g, w))
}

fn poll_done() {
    with(|p| p.poll_dl = None);
}

/// A wait time given as `timespec` in milliseconds, rounded up (1 ns waits 1 ms, not 0); huge values saturate.
fn timespec_ms_up(b: &[u8]) -> u64 {
    let (sec, ns) = (u64::from_le_bytes(b[0..8].try_into().unwrap_or([0; 8])), u64::from_le_bytes(b[8..16].try_into().unwrap_or([0; 8])));
    sec.saturating_mul(1000).saturating_add(ns.min(999_999_999).div_ceil(1_000_000)).min(i64::MAX as u64)
}

fn timespec_ms(b: &[u8]) -> u64 {
    u64::from_le_bytes(b[0..8].try_into().unwrap()) * 1000 + u64::from_le_bytes(b[8..16].try_into().unwrap()) / 1_000_000
}

fn sys_timerfd_settime(fd: u64, flags: u64, new: u64, old: u64) -> i64 {
    let t = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::TimerFd(t)) => Some(t.clone()),
        _ => None,
    })
    .flatten();
    let Some(t) = t else { return -EINVAL };
    let Some(b) = get(new, 32) else { return -EFAULT };
    let (iv, val) = (timespec_ms(&b[0..16]), timespec_ms(&b[16..32]));
    let now = crate::arch::uptime_ms();
    if old != 0 {
        let (dl, oiv) = *t.lock();
        let rem = dl.saturating_sub(now);
        let mut o = vec![0u8; 32];
        o[0..8].copy_from_slice(&(oiv / 1000).to_le_bytes());
        o[8..16].copy_from_slice(&((oiv % 1000) * 1_000_000).to_le_bytes());
        o[16..24].copy_from_slice(&(rem / 1000).to_le_bytes());
        o[24..32].copy_from_slice(&((rem % 1000) * 1_000_000).to_le_bytes());
        put(old, &o);
    }
    let b0 = b[16..32].iter().all(|&x| x == 0);
    let dl = if b0 {
        0
    } else if flags & 1 != 0 {
        // absolute Zeit: Uhr des timerfd (MONOTONIC = Laufzeit)
        val.max(1).max(now.min(val))
    } else {
        now + val.max(1)
    };
    *t.lock() = (dl, iv);
    0
}

fn sys_poll(fds: u64, n: u64, timeout: i64) -> Res {
    let Some(b) = get(fds, (n.min(256) * 8) as usize) else { return Res::Val(-EFAULT) };
    let seen = crate::socket::gen();
    poll_input();
    let inp = input_ready();
    let mut out = b.clone();
    let mut ready = 0;
    for (i, c) in b.chunks(8).enumerate() {
        let fd = i32::from_le_bytes(c[0..4].try_into().unwrap());
        let ev = u16::from_le_bytes([c[4], c[5]]);
        let kind_ready = ready_of(fd as i64, ev as u32, inp) as u16;
        out[i * 8 + 6..i * 8 + 8].copy_from_slice(&kind_ready.to_le_bytes());
        if kind_ready != 0 {
            ready += 1;
        }
    }
    if ready > 0 || timeout == 0 {
        poll_done();
        put(fds, &out);
        return Res::Val(ready);
    }
    let _ = has_sock_or_pipe(&b);
    let fdl: Vec<i64> = b.chunks(8).map(|c| i32::from_le_bytes(c[0..4].try_into().unwrap()) as i64).collect();
    let tdl = with(|p| timer_deadline(p, &fdl)).unwrap_or(0);
    match poll_wait(timeout, tdl, seen) {
        Res::Val(v) => {
            put(fds, &out);
            Res::Val(v)
        }
        r => r,
    }
}

/// enthält die poll-Liste Sockets oder Rohre? (dann bei Netz-/Rohrereignissen neu prüfen)
fn has_sock_or_pipe(b: &[u8]) -> bool {
    b.chunks(8).any(|c| {
        let fd = i32::from_le_bytes(c[0..4].try_into().unwrap());
        with(|p| matches!(p.fd(fd as u64).map(|d| &d.kind), Some(FdKind::Sock(_)) | Some(FdKind::PipeR(_)))).unwrap_or(false)
    })
}

fn sys_select(n: u64, rfds: u64, wfds: u64, timeout_ms: i64) -> Res {
    let n = n.min(1024) as usize;
    let bytes = n.div_ceil(64) * 8;
    let rd = if rfds != 0 { get(rfds, bytes) } else { None };
    let wr = if wfds != 0 { get(wfds, bytes) } else { None };
    let seen = crate::socket::gen();
    poll_input();
    let inp = input_ready();
    let bit = |v: &Vec<u8>, i: usize| v[i / 8] & (1 << (i % 8)) != 0;
    let mut ro = vec![0u8; bytes];
    let mut wo = vec![0u8; bytes];
    let mut cnt = 0;
    for i in 0..n {
        let ev = ready_of(i as i64, 5, inp);
        let kind = if ev & 0x20 != 0 { None } else { Some(ev & 1 != 0) };
        let wready = ev & 4 != 0;
        if let Some(v) = &rd {
            if bit(v, i) && kind == Some(true) {
                ro[i / 8] |= 1 << (i % 8);
                cnt += 1;
            }
        }
        if let Some(v) = &wr {
            if bit(v, i) && kind.is_some() && wready {
                wo[i / 8] |= 1 << (i % 8);
                cnt += 1;
            }
        }
    }
    if cnt > 0 || timeout_ms == 0 {
        poll_done();
        if rfds != 0 {
            put(rfds, &ro);
        }
        if wfds != 0 {
            put(wfds, &wo);
        }
        return Res::Val(cnt);
    }
    let fdl: Vec<i64> = (0..n as i64).collect();
    let tdl = with(|p| timer_deadline(p, &fdl)).unwrap_or(0);
    match poll_wait(timeout_ms, tdl, seen) {
        Res::Val(v) => {
            if rfds != 0 {
                put(rfds, &ro);
            }
            if wfds != 0 {
                put(wfds, &wo);
            }
            Res::Val(v)
        }
        r => r,
    }
}

// ------------------------------------------------------------------ Netz

fn sock_id(fd: u64) -> Option<usize> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::Sock(h)) => Some(h.0),
        _ => None,
    })
    .flatten()
}

fn sys_reboot(cmd: u64) -> i64 {
    if !calls::holds(Rights::REBOOT) {
        return -EPERM;
    }
    match cmd {
        0 | 0x89AB_CDEF => 0,
        0x0123_4567 => {
            crate::kprintln!("[kern] {} ({} {})", crate::sprache::t("neustart"), crate::sprache::t("von_prozess"), proc::current());
            crate::power::neustart()
        }
        0x4321_FEDC | 0xCDEF_0123 => {
            crate::kprintln!("[kern] {} ({} {})", crate::sprache::t("aus"), crate::sprache::t("von_prozess"), proc::current());
            crate::power::aus()
        }
        _ => -EINVAL,
    }
}

fn sys_signalfd(fd: i64, mask: u64, flags: u64) -> i64 {
    let Some(m) = get(mask, 8).map(|b| u64::from_le_bytes(b.try_into().unwrap())) else { return -EFAULT };
    if fd >= 0 {
        return with(|p| match p.fd(fd as u64) {
            Some(Fd { kind: FdKind::SigFd(x), .. }) => {
                *x = m;
                fd
            }
            _ => -EINVAL,
        })
        .unwrap_or(-EBADF);
    }
    with(|p| p.add_fd(Fd { kind: FdKind::SigFd(m), cloexec: flags & 0x80000 != 0, flags: (flags as u32) & 0x800 }, 0)).unwrap_or(-EBADF)
}

fn sys_socket(domain: u64, typ: u64, proto: u64) -> i64 {
    if domain == 1 {
        let st = Arc::new(spin::Mutex::new(proc::UnixState::Unbound));
        return with(|p| p.add_fd(Fd { kind: FdKind::UnixSock(st), cloexec: typ & 0x80000 != 0, flags: 2 | (typ as u32 & 0x800) }, 0)).unwrap_or(-EBADF);
    }
    if domain == crate::devices::events::AF_NETLINK {
        return crate::devices::events::socket(typ, proto);
    }
    if domain != 2 {
        return -97;
    }
    let udp = match typ & 0xf {
        1 => false,
        2 => true,
        _ => return -93,
    };
    // ENFILE: the socket table is full, or this group holds its share of it
    let Some(id) = crate::socket::create(udp) else { return -23 };
    crate::socket::set_nonblock(id, typ & 0x800 != 0);
    let h = Arc::new(crate::socket::Handle(id));
    with(|p| p.add_fd(Fd { kind: FdKind::Sock(h), cloexec: typ & 0x80000 != 0, flags: 2 | (typ as u32 & 0x800) }, 0)).unwrap_or(-EBADF)
}

/// Name in a `sockaddr_un` and whether it is an abstract one (leading NUL: no node in the file tree).
fn unix_path(addr: u64, len: u64) -> Option<(String, bool)> {
    let b = get(addr, (len as usize).clamp(2, 110))?;
    if u16::from_le_bytes([b[0], b[1]]) != 1 {
        return None;
    }
    let raw = &b[2..];
    let abstract_name = raw.first() == Some(&0);
    let raw = if abstract_name { &raw[1..] } else { raw };
    let end = raw.iter().position(|&c| c == 0).unwrap_or(raw.len());
    Some((String::from_utf8_lossy(&raw[..end]).into_owned(), abstract_name))
}

/// A listening Unix socket as the list sees it: weak, so the list never keeps a closed listener alive.
type Listener = alloc::sync::Weak<spin::Mutex<proc::UnixState>>;

/// Listening Unix sockets: host path, the socket, and who bound it (what a connector sees as its peer).
static UNIX_LISTEN: spin::Mutex<Vec<(String, Listener, crate::uds::Peer)>> = spin::Mutex::new(Vec::new());

/// The listening socket `sock` closed (its path may meanwhile belong to a newer listener).
pub fn unix_unlisten(sock: &Arc<spin::Mutex<proc::UnixState>>) {
    // the path is copied first: the list is always locked before a socket, never inside one
    let path = match &*sock.lock() {
        proc::UnixState::Listening(p, _) => p.clone(),
        _ => return,
    };
    UNIX_LISTEN.lock().retain(|(p, s, _)| s.strong_count() > 0 && !(*p == path && core::ptr::eq(s.as_ptr(), Arc::as_ptr(sock))));
}

fn unix_sock(fd: u64) -> Option<Arc<spin::Mutex<proc::UnixState>>> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::UnixSock(s)) => Some(s.clone()),
        _ => None,
    })
    .flatten()
}

fn sys_socketpair(domain: u64, _typ: u64, sv: u64) -> i64 {
    if domain != 1 {
        return -95;
    }
    let (a, b) = proc::unix_pair();
    // each end's peer is the creator
    if let FdKind::UnixConn(r, w) = &a {
        crate::uds::mark(r, w, crate::uds::me());
    }
    let cl = _typ & 0x80000 != 0;
    let (x, y) = with(|p| (p.add_fd(Fd { kind: a, cloexec: cl, flags: 2 }, 0), p.add_fd(Fd { kind: b, cloexec: cl, flags: 2 }, 0))).unwrap_or((-1, -1));
    let mut out = Vec::new();
    out.extend_from_slice(&(x as i32).to_le_bytes());
    out.extend_from_slice(&(y as i32).to_le_bytes());
    if put(sv, &out) {
        0
    } else {
        -EFAULT
    }
}

fn sys_connect(fd: u64, addr: u64, len: u64) -> Res {
    if unix_sock(fd).is_some() {
        let Some((path, abstract_name)) = unix_path(addr, len) else { return Res::Val(-22) };
        // socket paths live in the caller's view like every other path; the node decides who may connect
        let path = match locate(-100, &path, true).and_then(|l| crate::uds::connect_node(&l, abstract_name).map(|_| l.host)) {
            Ok(host) => host,
            Err(e) => return Res::Val(e),
        };
        let l = UNIX_LISTEN.lock().iter().find(|(p, _, _)| *p == path).and_then(|(_, s, who)| Some((s.upgrade()?, *who)));
        let Some((l, listener)) = l else { return Res::Val(-111) };
        let (mine, theirs) = proc::unix_pair();
        if let FdKind::UnixConn(from_them, to_them) = &mine {
            crate::uds::mark(to_them, from_them, listener);
        }
        // Die Zähler von `theirs` gehen auf den Eintrag in der Warteschlange über (FdKind hat kein Drop)
        if let (proc::UnixState::Listening(_, q), FdKind::UnixConn(r, w)) = (&mut *l.lock(), theirs) {
            q.push_back((r, w));
        }
        crate::socket::bump();
        with(|p| {
            if let Some(d) = p.fd(fd) {
                d.kind = mine;
            }
        });
        return Res::Val(0);
    }
    let Some(id) = sock_id(fd) else { return Res::Val(-88) };
    let Some(b) = get(addr, (len as usize).min(128)) else { return Res::Val(-EFAULT) };
    let Some((ip, port)) = crate::socket::parse_addr(&b) else { return Res::Val(-97) };
    match crate::socket::connect(id, ip, port) {
        Ok(true) => Res::Val(0),
        Ok(false) if crate::socket::is_nonblock(id) => Res::Val(-115),
        Ok(false) => Res::Block(Wait::Net(crate::socket::gen())),
        Err(e) => Res::Val(e),
    }
}

/// `shutdown`. Unix stream: `SHUT_RD` (0) ends our reading (end of file once the buffer is empty;
/// the peer's writes get `EPIPE`), `SHUT_WR` (1) ends our writing (`EPIPE`; the peer reads the rest, then
/// end of file), `SHUT_RDWR` (2) both; blocked readers and writers of any thread wake up. A listening
/// socket stops listening: a blocked `accept` ends with `EINVAL`, queued connections are refused.
fn sys_shutdown(fd: u64, how: u64) -> i64 {
    if how > 2 {
        return -EINVAL;
    }
    if let Some(id) = sock_id(fd) {
        if how >= 1 {
            crate::socket::shutdown_write(id);
        }
        return 0;
    }
    if let Some(us) = unix_sock(fd) {
        unix_unlisten(&us);
        let queued = match core::mem::replace(&mut *us.lock(), proc::UnixState::Unbound) {
            proc::UnixState::Listening(_, q) => q,
            proc::UnixState::Unbound => return -ENOTCONN_U,
        };
        // the queued connections were never accepted: their ends close (the connector sees EOF/EPIPE)
        for (r, w) in queued {
            r.readers.fetch_sub(1, Ordering::SeqCst);
            w.writers.fetch_sub(1, Ordering::SeqCst);
        }
        crate::socket::bump();
        return 0;
    }
    let pipes = with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::UnixConn(rx, tx)) => Ok((rx.clone(), tx.clone())),
        Some(_) => Err(-88),
        None => Err(-EBADF),
    })
    .unwrap_or(Err(-EBADF));
    match pipes {
        Ok((rx, tx)) => {
            if how != 1 {
                rx.shut_down();
            }
            if how != 0 {
                tx.shut_down();
            }
            // wake every thread blocked on either direction
            crate::socket::bump();
            0
        }
        Err(e) => e,
    }
}

fn sys_accept(fd: u64, addr: u64, lenp: u64, flags: u64) -> Res {
    if let Some(us) = unix_sock(fd) {
        let got = match &mut *us.lock() {
            proc::UnixState::Listening(_, q) => q.pop_front(),
            _ => return Res::Val(-22),
        };
        return match got {
            Some((r, w)) => {
                // unbenannter Gegenüber: nur die Adressfamilie AF_UNIX
                if addr != 0 && lenp != 0 {
                    put(addr, &1u16.to_le_bytes());
                    put(lenp, &2u32.to_le_bytes());
                }
                Res::Val(with(|p| p.add_fd(Fd { kind: FdKind::UnixConn(r, w), cloexec: flags & 0x80000 != 0, flags: 2 | (flags as u32 & 0x800) }, 0)).unwrap_or(-EBADF))
            }
            None => {
                let nb = with(|p| p.fd(fd).map(|d| d.flags & 0x800 != 0)).flatten().unwrap_or(false);
                if nb {
                    Res::Val(-11)
                } else {
                    Res::Block(Wait::Poll(crate::socket::gen(), 0))
                }
            }
        };
    }
    let Some(id) = sock_id(fd) else { return Res::Val(-88) };
    crate::socket::poll();
    match crate::socket::accept(id) {
        Some(nid) => {
            if addr != 0 {
                if let Some((_, (rip, rport))) = crate::socket::names(nid) {
                    put(addr, &crate::socket::make_addr(rip, rport));
                    put(lenp, &16u32.to_le_bytes());
                }
            }
            let h = Arc::new(crate::socket::Handle(nid));
            crate::socket::set_nonblock(nid, flags & 0x800 != 0);
            Res::Val(with(|p| p.add_fd(Fd { kind: FdKind::Sock(h), cloexec: flags & 0x80000 != 0, flags: 2 }, 0)).unwrap_or(-EBADF))
        }
        None if crate::socket::is_nonblock(id) => Res::Val(-crate::socket::EAGAIN),
        None => Res::Block(Wait::Net(crate::socket::gen())),
    }
}

fn sock_recv(id: usize, buf: u64, n: u64, flags: u64, addr: u64, lenp: u64) -> Res {
    match crate::socket::recv(id, n.min(1 << 20) as usize, flags & 2 != 0) {
        crate::socket::Recv::Data(d, from) => {
            if let (Some((ip, port)), true) = (from, addr != 0) {
                put(addr, &crate::socket::make_addr(ip, port));
                if lenp != 0 {
                    put(lenp, &16u32.to_le_bytes());
                }
            }
            Res::Val(if put(buf, &d) || d.is_empty() { d.len() as i64 } else { -EFAULT })
        }
        crate::socket::Recv::Eof => Res::Val(0),
        crate::socket::Recv::Err(e) => Res::Val(e),
        crate::socket::Recv::Wait => {
            if flags & 0x40 != 0 || crate::socket::is_nonblock(id) {
                Res::Val(-crate::socket::EAGAIN)
            } else {
                Res::Block(Wait::Net(crate::socket::gen()))
            }
        }
    }
}

fn sys_sendto(fd: u64, buf: u64, len: u64, addr: u64, alen: u64) -> Res {
    let Some(id) = sock_id(fd) else { return write_res(fd, buf, len) };
    let Some(data) = get(buf, len.min(1 << 20) as usize) else { return Res::Val(-EFAULT) };
    let to = if addr != 0 { get(addr, (alen as usize).min(128)).and_then(|b| crate::socket::parse_addr(&b)) } else { None };
    let r = crate::socket::send(id, &data, to);
    if r == -crate::socket::EAGAIN && !crate::socket::is_nonblock(id) {
        return Res::Block(Wait::Net(crate::socket::gen()));
    }
    Res::Val(r)
}

fn sys_recvfrom(fd: u64, buf: u64, len: u64, flags: u64, addr: u64, lenp: u64) -> Res {
    if crate::scm::is_conn(fd) {
        return match crate::scm::recv(fd, &[(buf, len)], 0, 0, flags) {
            Ok(r) => Res::Val(r.len),
            Err(s) => scm_res(s),
        };
    }
    let Some(id) = sock_id(fd) else { return sys_read(fd, buf, len, None) };
    sock_recv(id, buf, len, flags, addr, lenp)
}

/// msghdr lesen: (Name, Namenslänge, iovec-Liste)
fn msghdr(m: u64) -> Option<(u64, u64, Vec<(u64, u64)>)> {
    let b = get(m, 56)?;
    let r = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
    Some((r(0), r(8) & 0xffff_ffff, iov(r(16), r(24))))
}

fn scm_res(s: crate::scm::Step) -> Res {
    match s {
        crate::scm::Step::Val(v) => Res::Val(v),
        crate::scm::Step::Wait(seen) => Res::Block(Wait::Net(seen)),
    }
}

/// The control buffer of a msghdr (at most 4 KiB are read).
fn msg_control(m: u64) -> Option<Vec<u8>> {
    let b = get(m + 32, 16)?;
    let (at, len) = (u64::from_le_bytes(b[0..8].try_into().ok()?), u64::from_le_bytes(b[8..16].try_into().ok()?));
    if at == 0 || len == 0 {
        return Some(Vec::new());
    }
    get(at, len.min(4096) as usize)
}

fn sys_sendmsg(fd: u64, m: u64, flags: u64) -> Res {
    let Some((name, nlen, v)) = msghdr(m) else { return Res::Val(-EFAULT) };
    // one message: at most 1 MiB in total
    let mut data = Vec::new();
    for (b, l) in v {
        let room = (1usize << 20) - data.len();
        if room == 0 {
            break;
        }
        match get(b, (l as usize).min(room)) {
            Some(d) if crate::mem::try_reserve(&mut data, d.len()) => data.extend_from_slice(&d),
            Some(_) => return Res::Val(-ENOMEM),
            None => {}
        }
    }
    if crate::scm::is_conn(fd) {
        let Some(ctl) = msg_control(m) else { return Res::Val(-EFAULT) };
        return scm_res(crate::scm::send(fd, &data, &ctl, flags));
    }
    let Some(id) = sock_id(fd) else {
        // pipe or other descriptor: write the bytes
        return write_bytes(fd, &data);
    };
    let to = if name != 0 { get(name, (nlen as usize).min(128)).and_then(|b| crate::socket::parse_addr(&b)) } else { None };
    Res::Val(crate::socket::send(id, &data, to))
}

fn sys_sendmmsg(fd: u64, vec: u64, n: u64) -> Res {
    let mut done = 0;
    for i in 0..n.min(64) {
        let m = vec + i * 64;
        match sys_sendmsg(fd, m, 0) {
            Res::Val(r) if r >= 0 => {
                put(m + 56, &(r as u32).to_le_bytes());
                done += 1;
            }
            Res::Val(e) => return Res::Val(if done > 0 { done } else { e }),
            other => return other,
        }
    }
    Res::Val(done)
}

fn sys_recvmsg(fd: u64, m: u64, flags: u64) -> Res {
    let Some((name, _, v)) = msghdr(m) else { return Res::Val(-EFAULT) };
    if crate::scm::is_conn(fd) {
        let Some(c) = get(m + 32, 16) else { return Res::Val(-EFAULT) };
        let (ctl, room) = (u64::from_le_bytes(c[0..8].try_into().unwrap_or([0; 8])), u64::from_le_bytes(c[8..16].try_into().unwrap_or([0; 8])));
        return match crate::scm::recv(fd, &v, ctl, if ctl == 0 { 0 } else { room }, flags) {
            Ok(r) => {
                put(m + 40, &r.ctl_len.to_le_bytes());
                put(m + 48, &r.msg_flags.to_le_bytes());
                put(m + 8, &0u32.to_le_bytes());
                Res::Val(r.len)
            }
            Err(s) => scm_res(s),
        };
    }
    let Some(id) = sock_id(fd) else {
        // other descriptors: read into the first buffer, no ancillary data
        put(m + 40, &0u64.to_le_bytes());
        return match v.first() {
            Some(&(b, l)) => sys_read(fd, b, l, None),
            None => Res::Val(0),
        };
    };
    let total: u64 = v.iter().map(|x| x.1).sum();
    match crate::socket::recv(id, total.min(1 << 20) as usize, flags & 2 != 0) {
        crate::socket::Recv::Data(d, from) => {
            let mut off = 0usize;
            for (b, l) in v {
                if off >= d.len() {
                    break;
                }
                let k = (l as usize).min(d.len() - off);
                put(b, &d[off..off + k]);
                off += k;
            }
            if let (Some((ip, port)), true) = (from, name != 0) {
                put(name, &crate::socket::make_addr(ip, port));
                put(m + 8, &16u32.to_le_bytes());
            }
            put(m + 40, &0u64.to_le_bytes());
            put(m + 48, &0u32.to_le_bytes());
            Res::Val(d.len() as i64)
        }
        crate::socket::Recv::Eof => Res::Val(0),
        crate::socket::Recv::Err(e) => Res::Val(e),
        crate::socket::Recv::Wait => {
            if flags & 0x40 != 0 || crate::socket::is_nonblock(id) {
                Res::Val(-crate::socket::EAGAIN)
            } else {
                Res::Block(Wait::Net(crate::socket::gen()))
            }
        }
    }
}

fn sys_bind(fd: u64, addr: u64, len: u64) -> i64 {
    // a device event socket listens to everything from its creation on: the address is not needed
    if crate::devices::events::is_event_fd(fd) {
        return 0;
    }
    if let Some(us) = unix_sock(fd) {
        let Some((path, abstract_name)) = unix_path(addr, len) else { return -22 };
        // a path gets a node in the file tree (EADDRINUSE if something is there); an abstract name
        // is taken while its listener lives
        let path = match locate(-100, &path, true).and_then(|l| crate::uds::bind_node(&l, abstract_name).map(|_| l.host)) {
            Ok(host) => host,
            Err(e) => return e,
        };
        let mut list = UNIX_LISTEN.lock();
        if abstract_name && list.iter().any(|(p, s, _)| *p == path && s.strong_count() > 0) {
            return -98;
        }
        // a listener whose node was removed is no longer reachable under this path
        list.retain(|(p, _, _)| *p != path);
        *us.lock() = proc::UnixState::Listening(path.clone(), alloc::collections::VecDeque::new());
        // the list is shared (its growth belongs to the kernel); the entry goes when the socket closes
        let who = crate::uds::me();
        crate::heap::kernel(|| list.push((path, Arc::downgrade(&us), who)));
        return 0;
    }
    let Some(id) = sock_id(fd) else { return -88 };
    let Some(b) = get(addr, (len as usize).min(128)) else { return -EFAULT };
    let Some((ip, port)) = crate::socket::parse_addr(&b) else { return -97 };
    crate::socket::bind(id, ip, port)
}

fn sys_sockname(fd: u64, addr: u64, lenp: u64, peer: bool) -> i64 {
    let Some(id) = sock_id(fd) else { return -88 };
    let Some((l, r)) = crate::socket::names(id) else { return -EBADF };
    let (ip, port) = if peer { r } else { l };
    if peer && port == 0 {
        return -107;
    }
    put(addr, &crate::socket::make_addr(ip, port));
    put(lenp, &16u32.to_le_bytes());
    0
}

fn sys_getsockopt(fd: u64, level: u64, opt: u64, val: u64, lenp: u64) -> i64 {
    let Some(id) = sock_id(fd) else { return crate::uds::getsockopt(fd, level, opt, val, lenp) };
    let v: i32 = if level == 1 && opt == 4 { -(crate::socket::pending_error(id) as i32) } else if level == 1 && opt == 3 { if crate::socket::names(id).is_some() { 1 } else { 2 } } else { 0 };
    put(val, &v.to_le_bytes());
    put(lenp, &4u32.to_le_bytes());
    0
}

// ------------------------------------------------------------------ Speicher

fn sys_brk(addr: u64) -> i64 {
    with(|p| {
        if addr <= p.g.brk_start || addr > p.g.brk_limit {
            return p.g.brk as i64;
        }
        let new = (addr + 4095) & !4095;
        let mut a = p.g.brk;
        while a < new {
            if p.space_mut().map(a, true, false).is_none() {
                // no memory: like Linux the break stays where it was
                let old = p.g.brk;
                while a > old {
                    a -= 4096;
                    p.space_mut().unmap(a);
                }
                return old as i64;
            }
            a += 4096;
        }
        let mut a = new;
        while a < p.g.brk {
            p.space_mut().unmap(a);
            a += 4096;
        }
        p.g.brk = new;
        addr as i64
    })
    .unwrap_or(-ENOMEM)
}

/// Where a new mapping of `len` bytes goes: the fixed address `addr` (whatever was there is removed)
/// or the next free one.
pub(crate) fn place(p: &mut Proc, addr: u64, len: u64, flags: u64) -> Result<u64, i64> {
    if flags & 0x10 == 0 {
        let v = p.g.mmap_next;
        if v + len + 4096 > proc::STACK_TOP - (1 << 30) {
            return Err(-ENOMEM);
        }
        p.g.mmap_next += len + 4096;
        return Ok(v);
    }
    // above the user half the page-table index wraps (non-canonical addresses) or names the
    // kernel half: a mapping there would replace the kernel's own mappings in this address space
    if addr & 0xfff != 0 || addr < crate::paging::USER_LOW || (addr >= crate::paging::USER_LOW_END && addr < 0x1000_0000_0000) || addr >= USER_TOP || addr + len > USER_TOP {
        return Err(-EINVAL);
    }
    if !p.space().unreserve(addr, len) {
        return Err(-ENOMEM);
    }
    p.space().unmap_shared(addr, len);
    let mut a = addr;
    while a < addr + len {
        p.space().unmap(a);
        a += 4096;
    }
    Ok(addr)
}

fn sys_mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: i64, off: u64) -> i64 {
    if len == 0 || len > 1 << 34 {
        return -EINVAL;
    }
    let len = (len + 4095) & !4095;
    if flags & 0x20 == 0 {
        // a memory object or the framebuffer: shared pages or a copy
        if let Some(r) = crate::shm::mmap(addr, len, prot, flags, fd, off) {
            return r;
        }
    }
    let data = if flags & 0x20 == 0 {
        match with(|p| match p.fd(fd as u64).map(|d| &d.kind) {
            Some(FdKind::File { data, .. }) => Some(data.clone()),
            _ => None,
        })
        .flatten()
        {
            Some(d) => Some(d),
            None => return -EBADF,
        }
    } else {
        None
    };
    with(|p| {
        let va = match place(p, addr, len, flags) {
            Ok(v) => v,
            Err(e) => return e,
        };
        let (w, x) = (prot & 2 != 0 || data.is_some(), prot & 4 != 0);
        if data.is_none() {
            // anonym: nur reservieren – Seiten entstehen beim ersten Zugriff (glibc reserviert z. B. 64 MB je Arena)
            if !p.space().unreserve(va, len) || !p.space().reserve(va, len, w || prot & 1 != 0 || prot == 0, x) {
                return -ENOMEM;
            }
            return va as i64;
        }
        let mut a = va;
        while a < va + len {
            let Some(pa) = p.space_mut().map(a, w || prot & 1 != 0 || prot == 0, x) else {
                // no memory: take back what this call mapped
                while a > va {
                    a -= 4096;
                    p.space_mut().unmap(a);
                }
                return -ENOMEM;
            };
            if let Some(d) = &data {
                let s = (off + (a - va)) as usize;
                if s < d.len() {
                    let n = (d.len() - s).min(4096);
                    unsafe { core::ptr::copy_nonoverlapping(d[s..].as_ptr(), pa as *mut u8, n) };
                }
            }
            a += 4096;
        }
        va as i64
    })
    .unwrap_or(-ENOMEM)
}

fn sys_munmap(addr: u64, len: u64) -> i64 {
    // same bound as mmap: without it one call walks up to 2^52 pages while holding the process lock
    if addr & 0xfff != 0 || addr < crate::paging::USER_LOW || len > 1 << 34 || addr >= USER_TOP || addr + len > USER_TOP {
        return -EINVAL;
    }
    with(|p| {
        if !p.space().unreserve(addr, (len + 4095) & !4095) {
            return -ENOMEM;
        }
        p.space().unmap_shared(addr, (len + 4095) & !4095);
        let mut a = addr;
        while a < addr + ((len + 4095) & !4095) {
            p.space_mut().unmap(a);
            a += 4096;
        }
        0
    })
    .unwrap_or(-EINVAL)
}

// ------------------------------------------------------------------ Prozesse

fn sys_fork(f: &mut TrapFrame, flags: u64, stack: u64, ptid: u64, ctid: u64, tls: u64) -> i64 {
    const VM: u64 = 0x100;
    const FILES: u64 = 0x400;
    const VFORK: u64 = 0x4000;
    const THREAD: u64 = 0x10000;
    const SETTLS: u64 = 0x80000;
    const PARENT_SETTID: u64 = 0x10_0000;
    const CHILD_CLEARTID: u64 = 0x20_0000;
    const CHILD_SETTID: u64 = 0x100_0000;
    let pid = proc::new_pid();
    let fs_now = unsafe { crate::paging::rdmsr(0xC000_0100) };
    let thread = flags & VM != 0 && flags & VFORK == 0;
    // namespaces: only mount and pid exist; a thread cannot get its own
    let new_ns = flags & (calls::CLONE_NEWNS | calls::CLONE_NEWPID);
    if flags & calls::CLONE_NEW_UNSUPPORTED != 0 || new_ns != 0 && (thread || flags & THREAD != 0) {
        return -EINVAL;
    }
    let child = with(|p| {
        if new_ns != 0 && !p.g.iso.rights.has(Rights::MOUNT) {
            return Err(-EPERM);
        }
        // pid namespace of the child: a new one, or the one set for children (unshare/setns)
        let child_ns = if thread {
            if !Arc::ptr_eq(p.link.ns(), &p.g.iso.pid_ns_children) {
                return Err(-EINVAL);
            }
            p.link.ns().clone()
        } else if flags & calls::CLONE_NEWPID != 0 {
            match crate::isolation::pid::PidNs::child(&p.g.iso.pid_ns_children) {
                Some(ns) => ns,
                None => return Err(-28),
            }
        } else {
            p.g.iso.pid_ns_children.clone()
        };
        if child_ns.is_dead() {
            return Err(-ENOMEM);
        }
        // resource group: a process started in new namespaces (a cell) gets its own group, every
        // other child stays in the parent's. The thread is counted first (`pids.max`), and all
        // that the child needs is charged to its group.
        let res = if new_ns != 0 {
            match crate::groups::create_cell(p.g.res, pid) {
                Ok(g) => g,
                Err(e) => return Err(e.errno()),
            }
        } else {
            p.g.res
        };
        let Some(pids) = crate::groups::PidCharge::new(res) else {
            if new_ns != 0 {
                let _ = crate::groups::remove(res);
            }
            return Err(-EAGAIN);
        };
        crate::heap::scope(res, || {
        let g = if thread {
            // Faden: Adressraum, Dateien, Signale gemeinsam
            p.g.clone()
        } else {
            let mut iso = p.g.iso.clone();
            iso.pid_ns_children = child_ns.clone();
            if flags & calls::CLONE_NEWNS != 0 {
                iso.mnt = iso.mnt.copy();
            }
            let mut fds: Vec<Option<Fd>> = crate::mem::try_with_capacity(p.g.fds.len()).ok_or(-ENOMEM)?;
            for o in p.g.fds.iter() {
                fds.push(o.as_ref().map(|d| {
                    let mut n = d.dup();
                    // CLONE_FILES is not modelled as sharing: the child gets its own table either way
                    n.cloexec = if flags & FILES != 0 { false } else { d.cloexec };
                    n
                }));
            }
            proc::Shared::new(proc::Group {
                // vfork with CLONE_VM: the child runs in the parent's memory until execve/_exit
                // (glibc's posix_spawn reports a failed execve through it); else a copy
                space: Some(if flags & VM != 0 { p.g.space.clone().ok_or(-ENOMEM)? } else { Arc::new(p.space().fork().ok_or(-ENOMEM)?) }),
                fds,
                cwd: p.g.cwd.clone(),
                brk_start: p.g.brk_start,
                brk: p.g.brk,
                brk_limit: p.g.brk_limit,
                mmap_next: p.g.mmap_next,
                umask: p.g.umask,
                exe: p.g.exe.clone(),
                cmdline: p.g.cmdline.clone(),
                sigact: p.g.sigact,
                auxv: p.g.auxv.clone(),
                iso,
                exe_host: p.g.exe_host.clone(),
                res,
            })
        };
        let kstack = crate::mem::try_vec(proc::KSTACK).ok_or(-ENOMEM)?;
        // numbers of the child: as the parent sees it (return value) and as it sees itself
        // (the tables of the namespaces are shared: they belong to the kernel)
        let link = crate::heap::kernel(|| crate::isolation::pid::PidLink::new(child_ns, pid));
        let seen_by_parent = shown(p, pid);
        let seen_by_child = link.ns().local(pid).unwrap_or(0);
        let mut fr = f.clone();
        fr.rax = 0;
        if stack != 0 {
            fr.rsp = stack;
        }
        if flags & PARENT_SETTID != 0 && ptid != 0 {
            proc::write_user(p.space(), ptid, &(seen_by_parent as u32).to_le_bytes());
        }
        if flags & CHILD_SETTID != 0 && ctid != 0 {
            proc::write_user(g.space.as_ref().unwrap(), ctid, &(seen_by_child as u32).to_le_bytes());
        }
        let tgid = if thread || flags & THREAD != 0 { p.tgid } else { pid };
        Ok((seen_by_parent, Proc {
            pid,
            tgid,
            ppid: if tgid == pid { p.tgid } else { p.ppid },
            pgid: p.pgid,
            name: p.name.clone(),
            state: State::Ready,
            frame: alloc::boxed::Box::new(fr),
            kstack,
            fs_base: if flags & SETTLS != 0 { tls } else { fs_now },
            clear_tid: if flags & CHILD_CLEARTID != 0 { ctid } else { 0 },
            sigmask: p.sigmask,
            pending: 0,
            sus_mask: None,
            killed: None,
            on_cpu: false,
            poll_dl: None,
            g,
            link,
            pids,
            io_done: 0,
        }))
        })
        .and_then(|(seen, child)| {
            // the box is the child's (charged to its group); the process table is the kernel's
            let boxed = crate::heap::scope(res, || alloc::boxed::Box::new(child));
            Ok((seen, boxed))
        })
    });
    let (seen, child) = match child {
        Some(Ok(c)) => c,
        Some(Err(e)) => return e,
        None => return -ENOMEM,
    };
    {
        // the namespace's pid 1 may have ended meanwhile: checked under the same lock that ends it
        let mut ps = PROCS.lock();
        if child.link.ns().is_dead() {
            return -ENOMEM;
        }
        crate::heap::kernel(|| ps.push(child));
    }
    if flags & VFORK != 0 {
        proc::vfork_started(pid, with(|p| p.tgid).unwrap_or(0));
        block(f, Wait::Vfork(pid), false, seen);
    }
    seen
}

fn sys_futex(addr: u64, op: u64, val: u64, timeout: u64) -> Res {
    let cmd = op & 0x7f;
    let Some(pa) = with(|p| p.space().translate(addr).map(|x| x.0)).flatten() else { return Res::Val(-EFAULT) };
    match cmd {
        0 | 9 => {
            let cur = unsafe { core::ptr::read_volatile(pa as *const u32) };
            if cur != val as u32 {
                return Res::Val(-11);
            }
            let deadline = if timeout != 0 {
                match get(timeout, 16) {
                    Some(b) => {
                        let ms = u64::from_le_bytes(b[0..8].try_into().unwrap()) * 1000 + u64::from_le_bytes(b[8..16].try_into().unwrap()) / 1_000_000;
                        if cmd == 9 {
                            // absolute Zeit (CLOCK_MONOTONIC bzw. REALTIME)
                            let now = now_ns(if op & 256 != 0 { 0 } else { 1 }) / 1_000_000;
                            crate::arch::uptime_ms() + ms.saturating_sub(now).max(1)
                        } else {
                            crate::arch::uptime_ms() + ms.max(1)
                        }
                    }
                    None => 0,
                }
            } else {
                0
            };
            Res::BlockWith(Wait::Futex(pa, deadline), 0)
        }
        1 | 10 => Res::Val(proc::futex_wake_locked(&mut PROCS.lock(), pa, val.max(1)) as i64),
        3 | 4 => {
            // REQUEUE: alle wecken (einfach und korrekt, nur weniger effizient)
            Res::Val(proc::futex_wake_locked(&mut PROCS.lock(), pa, u64::MAX) as i64)
        }
        5 => Res::Val(proc::futex_wake_locked(&mut PROCS.lock(), pa, val.max(1)) as i64),
        _ => Res::Val(-ENOSYS),
    }
}

fn sys_execve(f: &mut TrapFrame, path: u64, argv: u64, envp: u64) -> i64 {
    let args = with(|p| {
        let sp = p.space();
        Some((proc::read_cstr(sp, path)?, proc::read_strv(sp, argv)?, proc::read_strv(sp, envp)?))
    })
    .flatten();
    let Some((path, argv, envp)) = args else { return -EFAULT };
    // the program is looked up in the caller's view; `full` is its place in the kernel's tree
    let loc = match locate(-100, &path, true) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let full = loc.host.clone();
    if let Err(e) = crate::perm::may_exec(&full) {
        return e;
    }
    let data = match fs::open(&full) {
        Some(fs::Node::File(d)) => d,
        Some(_) => return -13,
        None => return -ENOENT,
    };
    if data.starts_with(b"#!") {
        // Skript: Interpreter aus der ersten Zeile
        let line = data.split(|&b| b == b'\n').next().unwrap_or(&[]);
        let l = String::from(String::from_utf8_lossy(&line[2..]).trim());
        let mut it = l.split_whitespace();
        let interp = String::from(it.next().unwrap_or("/bin/sh"));
        let mut nargv: Vec<String> = vec![interp.clone()];
        nargv.extend(it.map(String::from));
        nargv.push(loc.view.clone());
        nargv.extend(argv.into_iter().skip(1));
        let iloc = match locate(-100, &interp, true) {
            Ok(l) => l,
            Err(e) => return e,
        };
        if let Err(e) = crate::perm::may_exec(&iloc.host) {
            return e;
        }
        let idata = match fs::read_all(&iloc.host) {
            Some(d) => d,
            None => return -ENOENT,
        };
        return exec_image(f, &idata, nargv, envp, &iloc);
    }
    exec_image(f, &data, argv, envp, &loc)
}

fn exec_image(f: &mut TrapFrame, data: &[u8], argv: Vec<String>, envp: Vec<String>, loc: &Loc) -> i64 {
    let path = loc.view.as_str();
    // the dynamic loader named by the program is read through the caller's view as well
    let read = |p: &str| locate(-100, p, true).ok().and_then(|l| fs::read_all(&l.host));
    let img = match proc::load_elf(data, &argv, &envp, path, &read) {
        Ok(i) => i,
        Err(e) => {
            crate::kprintln!("[exec] {path}: {e}");
            return -8;
        }
    };
    unsafe { core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4()) };
    with(|p| {
        p.g.auxv = img.auxv.clone();
        p.g.space = Some(Arc::new(img.space));
        p.g.brk_start = img.brk;
        p.g.brk = img.brk;
        p.g.brk_limit = img.brk_limit;
        p.g.mmap_next = proc::MMAP_BASE;
        p.fs_base = 0;
        p.name = argv.first().cloned().unwrap_or_default();
        p.g.exe = String::from(path);
        p.g.exe_host = loc.host.clone();
        p.g.cmdline = argv.clone();
        for d in p.g.fds.iter_mut() {
            if d.as_ref().map(|x| x.cloexec).unwrap_or(false) {
                *d = None;
            }
        }
        p.space().activate();
    });
    unsafe { crate::paging::wrmsr(0xC000_0100, 0) };
    *f = proc::fresh_frame(img.entry, img.sp);
    // a parent waiting in vfork goes on now (the child no longer uses its memory)
    proc::vfork_exec(proc::current());
    0
}

fn sys_wait4(pid: i64, status: u64, opts: u64) -> Res {
    let Some((me, ns)) = with(|p| (p.tgid, p.link.ns().clone())) else { return Res::Val(-ECHILD) };
    // the caller names a child by the number of its own pid namespace
    let pid = if pid > 0 {
        match ns.global(pid as u64) {
            Some(g) => g as i64,
            None => return Res::Val(-ECHILD),
        }
    } else {
        pid
    };
    let mut ps = PROCS.lock();
    let mine = |c: &&alloc::boxed::Box<Proc>| c.pid == c.tgid && c.ppid == me && (pid <= 0 || c.pid == pid as u64);
    if !ps.iter().any(|c| mine(&c)) {
        return Res::Val(-ECHILD);
    }
    if let Some(i) = ps.iter().position(|c| mine(&c) && matches!(c.state, State::Zombie(_)) && !c.on_cpu) {
        let c = ps.remove(i);
        let State::Zombie(st) = c.state else { unreachable!() };
        drop(ps);
        if status != 0 {
            put(status, &st.to_le_bytes());
        }
        return Res::Val(ns.local(c.pid).unwrap_or(0) as i64);
    }
    drop(ps);
    if opts & 1 != 0 {
        return Res::Val(0);
    }
    Res::Block(Wait::Child(pid))
}

/// `kill`: the targets are named in the caller's pid namespace; visibility, the pid 1 rule and the
/// `KILL_ANY` right are decided in isolation::signals.
fn sys_kill(pid: i64, sig: u64) -> i64 {
    let mut ps = PROCS.lock();
    let me = proc::current();
    let Some(sender) = ps.iter().position(|p| p.pid == me) else { return -ESRCH };
    let targets = match signals::kill_targets(&ps, sender, pid) {
        Ok(t) => t,
        Err(e) => return e,
    };
    let live = match signals::permitted(&ps, sender, &targets, sig, pid > 0) {
        Ok(t) => t,
        Err(e) => return e,
    };
    if sig == 0 {
        return 0;
    }
    for t in &live {
        crate::signal::send_locked(&mut ps, *t, 0, sig);
    }
    let me_dead = ps.iter().find(|p| p.pid == me).map(|p| matches!(p.state, State::Zombie(_) | State::Dead)).unwrap_or(false);
    drop(ps);
    // nothing on this stack is dropped once the process is gone
    drop((targets, live));
    if me_dead {
        proc::exit_thread(sig as i32, true);
    }
    0
}

/// `tkill(tid, sig)` (`tgid` = None) and `tgkill(tgid, tid, sig)`: signal one thread, same rules as `kill`.
fn sys_tgkill(tgid: Option<u64>, tid: u64, sig: u64) -> i64 {
    let mut ps = PROCS.lock();
    let me = proc::current();
    let Some(sender) = ps.iter().position(|p| p.pid == me) else { return -ESRCH };
    let ns = ps[sender].link.ns().clone();
    let Some(thread) = ns.global(tid) else { return -ESRCH };
    let Some(group) = ps.iter().find(|p| p.pid == thread).map(|p| p.tgid) else { return -ESRCH };
    if let Some(want) = tgid {
        if ns.global(want) != Some(group) {
            return -ESRCH;
        }
    }
    match signals::permitted(&ps, sender, &[group], sig, true) {
        Err(e) => e,
        Ok(t) if t.is_empty() || sig == 0 => 0,
        Ok(_) => {
            // tgkill reports a thread that cannot take the signal; tkill never did
            if !crate::signal::send_locked(&mut ps, group, thread, sig) && tgid.is_some() {
                return -ESRCH;
            }
            0
        }
    }
}

fn sys_sigaction(sig: u64, act: u64, old: u64) -> i64 {
    if sig == 0 || sig > 64 {
        return -EINVAL;
    }
    let cur = with(|p| p.g.sigact[sig as usize]).unwrap_or_default();
    if old != 0 {
        let mut b = vec![0u8; 32];
        b[0..8].copy_from_slice(&cur.handler.to_le_bytes());
        b[8..16].copy_from_slice(&cur.flags.to_le_bytes());
        b[16..24].copy_from_slice(&cur.restorer.to_le_bytes());
        b[24..32].copy_from_slice(&cur.mask.to_le_bytes());
        if !put(old, &b) {
            return -EFAULT;
        }
    }
    if act != 0 {
        if sig == 9 || sig == 19 {
            return -EINVAL;
        }
        let Some(b) = get(act, 32) else { return -EFAULT };
        let r = |o: usize| u64::from_le_bytes(b[o..o + 8].try_into().unwrap());
        let n = proc::SigAct { handler: r(0), flags: r(8), restorer: r(16), mask: r(24) };
        with(|p| p.g.sigact[sig as usize] = n);
    }
    0
}

fn sys_sigprocmask(how: u64, set: u64, old: u64) -> i64 {
    let cur = with(|p| p.sigmask).unwrap_or(0);
    if old != 0 && !put(old, &cur.to_le_bytes()) {
        return -EFAULT;
    }
    if set != 0 {
        let Some(b) = get(set, 8) else { return -EFAULT };
        let m = u64::from_le_bytes(b.try_into().unwrap());
        let n = match how {
            0 => cur | m,
            1 => cur & !m,
            2 => m,
            _ => return -EINVAL,
        };
        with(|p| p.sigmask = n & !((1 << 8) | (1 << 18)));
    }
    0
}

fn sys_uname(buf: u64) -> i64 {
    let mut b = vec![0u8; 65 * 6];
    for (i, s) in ["Linux", "ultar", "6.8.0-ultar-kern", concat!("#1 Ultar-Kern ", ultar_brand::vendor!()), "x86_64", "(none)"].iter().enumerate() {
        b[i * 65..i * 65 + s.len()].copy_from_slice(s.as_bytes());
    }
    if put(buf, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_arch_prctl(code: u64, addr: u64) -> i64 {
    match code {
        0x1002 => {
            unsafe { crate::paging::wrmsr(0xC000_0100, addr) };
            with(|p| p.fs_base = addr);
            0
        }
        0x1003 => {
            let v = unsafe { crate::paging::rdmsr(0xC000_0100) };
            if put(addr, &v.to_le_bytes()) {
                0
            } else {
                -EFAULT
            }
        }
        _ => -EINVAL,
    }
}

fn sys_prlimit(res: u64, old: u64) -> i64 {
    if old == 0 {
        return 0;
    }
    let (cur, max) = match res {
        3 => (8u64 << 20, u64::MAX),
        7 => (1024, 4096),
        _ => (u64::MAX, u64::MAX),
    };
    let mut b = Vec::new();
    b.extend_from_slice(&cur.to_le_bytes());
    b.extend_from_slice(&max.to_le_bytes());
    if put(old, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_sysinfo(buf: u64) -> i64 {
    let (total, _, _, _) = crate::mem::stats();
    let free = crate::mem::free_bytes();
    let mut b = vec![0u8; 112];
    b[0..8].copy_from_slice(&(crate::arch::uptime_ms() / 1000).to_le_bytes());
    b[32..40].copy_from_slice(&total.to_le_bytes());
    b[40..48].copy_from_slice(&free.to_le_bytes());
    b[80..82].copy_from_slice(&(PROCS.lock().len() as u16).to_le_bytes());
    b[104..108].copy_from_slice(&1u32.to_le_bytes());
    if put(buf, &b) {
        0
    } else {
        -EFAULT
    }
}

fn sys_sleep(req: u64, abs: bool) -> Res {
    let Some(b) = get(req, 16) else { return Res::Val(-EFAULT) };
    let s = u64::from_le_bytes(b[0..8].try_into().unwrap());
    let ns = u64::from_le_bytes(b[8..16].try_into().unwrap());
    let ms = s * 1000 + ns / 1_000_000;
    let until = if abs { ms.saturating_sub(now_ns(1) / 1_000_000) + crate::arch::uptime_ms() } else { crate::arch::uptime_ms() + ms };
    Res::BlockWith(Wait::Sleep(until), 0)
}
