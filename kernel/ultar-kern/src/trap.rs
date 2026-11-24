//! Unterbrechungen, Ausnahmen und Systemaufrufe über einen gemeinsamen Einstieg.
//!
//! Jeder Einstieg legt einen vollständigen `TrapFrame` (alle Register + SSE-Zustand) auf den Kernstapel.
//! Aus Benutzerprogrammen: Systemaufruf (`syscall`), Zeitgeber, Ausnahmen → proc.rs entscheidet, ob das
//! Programm weiterläuft, wartet oder ein anderes drankommt.

use crate::arch;
use crate::paging::{rdmsr, wrmsr};
use core::arch::global_asm;

#[repr(C, align(16))]
#[derive(Clone)]
pub struct TrapFrame {
    pub fx: [u8; 512],
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub vector: u64,
    pub err: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

impl TrapFrame {
    pub fn zeroed() -> TrapFrame {
        unsafe { core::mem::zeroed() }
    }
    pub fn from_user(&self) -> bool {
        self.cs & 3 == 3
    }
}

pub const VEC_TIMER: u64 = 32;
pub const VEC_SYSCALL: u64 = 0x80;
pub const VEC_SPURIOUS: u64 = 0xff;

global_asm!(
    r#"
.macro TRAP_NOERR n
.global ultar_trap\n
ultar_trap\n:
    push 0
    push \n
    jmp ultar_trap_common
.endm
.macro TRAP_ERR n
.global ultar_trap\n
ultar_trap\n:
    push \n
    jmp ultar_trap_common
.endm

TRAP_NOERR 0
TRAP_NOERR 1
TRAP_NOERR 2
TRAP_NOERR 3
TRAP_NOERR 4
TRAP_NOERR 5
TRAP_NOERR 6
TRAP_NOERR 7
TRAP_ERR 10
TRAP_ERR 11
TRAP_ERR 12
TRAP_ERR 13
TRAP_ERR 14
TRAP_NOERR 16
TRAP_ERR 17
TRAP_NOERR 18
TRAP_NOERR 19
TRAP_NOERR 32
TRAP_NOERR 255

ultar_trap_common:
    test byte ptr [rsp + 24], 3
    jz 1f
    swapgs
1:
ultar_trap_common_noswap:
    push rax
    push rbx
    push rcx
    push rdx
    push rsi
    push rdi
    push rbp
    push r8
    push r9
    push r10
    push r11
    push r12
    push r13
    push r14
    push r15
    sub rsp, 512
    fxsave64 [rsp]
    mov rdi, rsp
    cld
    call ultar_trap_rust
.global ultar_trap_restore
ultar_trap_restore:
    fxrstor64 [rsp]
    add rsp, 512
    pop r15
    pop r14
    pop r13
    pop r12
    pop r11
    pop r10
    pop r9
    pop r8
    pop rbp
    pop rdi
    pop rsi
    pop rdx
    pop rcx
    pop rbx
    pop rax
    add rsp, 16
    test byte ptr [rsp + 8], 3
    jz 2f
    swapgs
2:
    iretq

.global ultar_syscall
ultar_syscall:
    swapgs
    mov gs:[0], rsp
    mov rsp, gs:[8]
    push 0x23
    push qword ptr gs:[0]
    push r11
    push 0x2b
    push rcx
    push 0
    push 0x80
    jmp ultar_trap_common_noswap

.global ultar_resume
ultar_resume:
    push rbx
    push rbp
    push r12
    push r13
    push r14
    push r15
    mov [rsi], rsp
    mov rsp, rdi
    jmp ultar_trap_restore

.global ultar_back
ultar_back:
    mov rsp, [rdi]
    pop r15
    pop r14
    pop r13
    pop r12
    pop rbp
    pop rbx
    mov rax, rsi
    ret
"#
);

extern "sysv64" {
    fn ultar_trap0();
    fn ultar_trap1();
    fn ultar_trap2();
    fn ultar_trap3();
    fn ultar_trap4();
    fn ultar_trap5();
    fn ultar_trap6();
    fn ultar_trap7();
    fn ultar_trap10();
    fn ultar_trap11();
    fn ultar_trap12();
    fn ultar_trap13();
    fn ultar_trap14();
    fn ultar_trap16();
    fn ultar_trap17();
    fn ultar_trap18();
    fn ultar_trap19();
    fn ultar_trap32();
    fn ultar_trap255();
    fn ultar_syscall();
    /// Prozess fortsetzen: `frame` liegt auf seinem Kernstapel; kehrt mit dem Grund aus `back` zurück
    pub fn ultar_resume(frame: *mut TrapFrame, ctx: *mut u64) -> u64;
    /// zurück in den Planer (verlässt den Kernstapel des Prozesses)
    pub fn ultar_back(ctx: *mut u64, reason: u64) -> !;
}

/// syscall-Register eines Kerns einrichten (jeder Kern hat eigene MSRs)
pub unsafe fn init_msrs() {
    wrmsr(0xC000_0080, rdmsr(0xC000_0080) | 1);
    wrmsr(0xC000_0081, (0x18u64 << 48) | (0x08u64 << 32));
    wrmsr(0xC000_0082, ultar_syscall as usize as u64);
    wrmsr(0xC000_0084, 0x4_7700);
}

pub unsafe fn init() {
    let v: [(usize, unsafe extern "sysv64" fn()); 19] = [
        (0, ultar_trap0),
        (1, ultar_trap1),
        (2, ultar_trap2),
        (3, ultar_trap3),
        (4, ultar_trap4),
        (5, ultar_trap5),
        (6, ultar_trap6),
        (7, ultar_trap7),
        (10, ultar_trap10),
        (11, ultar_trap11),
        (12, ultar_trap12),
        (13, ultar_trap13),
        (14, ultar_trap14),
        (16, ultar_trap16),
        (17, ultar_trap17),
        (18, ultar_trap18),
        (19, ultar_trap19),
        (32, ultar_trap32),
        (255, ultar_trap255),
    ];
    for (i, h) in v {
        arch::set_gate(i, h as usize as u64);
    }
    // syscall/sysret: EFER.SCE, Einsprung, Kern-Segmente, beim Eintritt IF/DF/TF/AC löschen
    init_msrs();
}

pub fn name(v: u64) -> &'static str {
    crate::sprache::ausnahme_name(v)
}

#[no_mangle]
extern "sysv64" fn ultar_trap_rust(f: &mut TrapFrame) {
    match f.vector {
        VEC_TIMER => {
            crate::apic::tick();
            crate::apic::eoi();
            if f.from_user() {
                crate::proc::on_timer(f);
            }
        }
        VEC_SPURIOUS => {}
        VEC_SYSCALL => crate::proc::on_syscall(f),
        v if f.from_user() => crate::proc::on_fault(f, v),
        v => {
            let cr2: u64;
            unsafe { core::arch::asm!("mov {}, cr2", out(reg) cr2) };
            crate::power::firmware_fehler();
            crate::sprache::ausnahme_zeile(v, f.rip, f.rsp, Some(f.err), cr2);
            arch::halt_forever()
        }
    }
}
