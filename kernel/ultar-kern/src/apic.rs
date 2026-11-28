//! Lokaler APIC (Zeitgeber, Interrupt-Quittung), ACPI-MADT (Prozessorkerne) und Start weiterer Kerne (SMP).

use crate::arch;
use crate::paging::{rdmsr, wrmsr};
use core::arch::global_asm;
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

pub const HZ: u64 = 100;
static BASE: AtomicU64 = AtomicU64::new(0);
static X2: AtomicBool = AtomicBool::new(false);
static TICKS: AtomicU64 = AtomicU64::new(0);
pub static ONLINE: AtomicU32 = AtomicU32::new(1);
static NEXT_CPU: AtomicU32 = AtomicU32::new(1);
static TIMER_COUNT: AtomicU32 = AtomicU32::new(100_000);
static CPUS: spin::Mutex<alloc::vec::Vec<u32>> = spin::Mutex::new(alloc::vec::Vec::new());

fn rd(reg: u32) -> u32 {
    if X2.load(Ordering::Relaxed) {
        unsafe { rdmsr(0x800 + (reg >> 4)) as u32 }
    } else {
        unsafe { core::ptr::read_volatile((BASE.load(Ordering::Relaxed) + reg as u64) as *const u32) }
    }
}
fn wr(reg: u32, v: u32) {
    if X2.load(Ordering::Relaxed) {
        unsafe { wrmsr(0x800 + (reg >> 4), v as u64) }
    } else {
        unsafe { core::ptr::write_volatile((BASE.load(Ordering::Relaxed) + reg as u64) as *mut u32, v) }
    }
}

pub fn id() -> u32 {
    if X2.load(Ordering::Relaxed) {
        rd(0x20)
    } else {
        rd(0x20) >> 24
    }
}

pub fn eoi() {
    wr(0xB0, 0);
}

/// Timer interrupts on all cores, fast or slow: a thread waiting for the GPU looks again after each.
static EPOCH: AtomicU64 = AtomicU64::new(0);
/// Cores whose timer runs fast (one bit per core).
static FAST: AtomicU64 = AtomicU64::new(0);
/// The fast timer: `FAST_DIV` interrupts per normal tick (100 Hz × 40 = every 250 µs).
const FAST_DIV: u64 = 40;
/// Fast interrupts of core 0 since its last counted tick.
static FAST_SUB: AtomicU64 = AtomicU64::new(0);

/// Cores sleeping in `idle` (one bit per core).
static IDLE: AtomicU64 = AtomicU64::new(0);

/// This core sleeps (`true`) or works again; see [`wake_waiters`].
pub fn set_idle(on: bool) {
    let bit = 1u64 << (crate::percpu::id() % 64);
    if on {
        IDLE.fetch_or(bit, Ordering::Relaxed);
    } else {
        IDLE.fetch_and(!bit, Ordering::Relaxed);
    }
}

/// Something that threads wait for happened (the GPU finished a job): their waits end at the
/// next look, and sleeping cores are woken to look now.
pub fn wake_waiters() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
    let others = IDLE.load(Ordering::Relaxed) & !(1u64 << (crate::percpu::id() % 64));
    if others != 0 {
        // fixed delivery, assert, to all cores but this one
        icr(0, crate::trap::VEC_WAKE as u32 | 1 << 14 | 0b11 << 18);
    }
}

/// TLB shootdown: the generation asked for, and the one each core has flushed.
static TLB_GEN: AtomicU64 = AtomicU64::new(0);
static TLB_SEEN: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
/// Most rounds a core waits for another core's flush (a few milliseconds): a core that cannot
/// take interrupts now does not stop this one for ever.
const TLB_WAIT: u32 = 200_000;

/// Rights to user pages were taken away (`mprotect`, `munmap`): every other core drops its cached
/// translations and this core waits until each one did (bounded, [`TLB_WAIT`]). Before, a thread
/// on another core could go on writing or running a page for as long as its TLB held it.
pub fn flush_tlb_others() {
    let online = ONLINE.load(Ordering::Relaxed) as usize;
    if online <= 1 {
        return;
    }
    let gen = TLB_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    // fixed delivery, assert, to all cores but this one
    icr(0, crate::trap::VEC_TLB as u32 | 1 << 14 | 0b11 << 18);
    let me = crate::percpu::id();
    for core in (0..online.min(64)).filter(|&c| c != me) {
        let mut rounds = 0;
        while TLB_SEEN[core].load(Ordering::Acquire) < gen && rounds < TLB_WAIT {
            arch::pause();
            rounds += 1;
        }
    }
}

/// The interrupt of [`flush_tlb_others`] on this core.
pub fn on_flush_tlb() {
    let gen = TLB_GEN.load(Ordering::SeqCst);
    // SAFETY: reloading CR3 with its own value only drops cached translations.
    unsafe { crate::paging::flush_all() };
    TLB_SEEN[crate::percpu::id() % 64].fetch_max(gen, Ordering::Release);
}

/// Number of timer interrupts so far on any core (see [`fast_timer`]).
pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

/// This core's timer every 250 µs (`on`) or at [`HZ`] again. An idle core uses it while threads
/// wait for the GPU: it sleeps in `hlt` and looks again soon, instead of polling with full power
/// (that took power from the GPU) or sleeping a whole tick (that made each wait 10 ms long).
pub fn fast_timer(on: bool) {
    let bit = 1u64 << (crate::percpu::id() % 64);
    let was = FAST.load(Ordering::Relaxed) & bit != 0;
    if was == on {
        return;
    }
    let count = TIMER_COUNT.load(Ordering::Relaxed);
    if on {
        FAST.fetch_or(bit, Ordering::Relaxed);
        wr(0x380, (count as u64 / FAST_DIV).max(1) as u32);
    } else {
        FAST.fetch_and(!bit, Ordering::Relaxed);
        wr(0x380, count);
    }
}

/// Time of all cores in timer steps of the fast rate ([`FAST_DIV`] per normal tick): busy
/// and idle (the core sat in `hlt` when its timer came) – `/proc/stat`'s `cpu` line.
static CPU_BUSY: AtomicU64 = AtomicU64::new(0);
static CPU_IDLE: AtomicU64 = AtomicU64::new(0);

/// Busy and idle time of all cores in ticks of [`HZ`] (`/proc/stat`).
pub fn cpu_times() -> (u64, u64) {
    (
        CPU_BUSY.load(Ordering::Relaxed) / FAST_DIV,
        CPU_IDLE.load(Ordering::Relaxed) / FAST_DIV,
    )
}

pub fn tick() {
    EPOCH.fetch_add(1, Ordering::Relaxed);
    let bit = 1u64 << (crate::percpu::id() % 64);
    let fast = FAST.load(Ordering::Relaxed) & bit != 0;
    // a fast step is 1/FAST_DIV of a normal tick
    let weight = if fast { 1 } else { FAST_DIV };
    if IDLE.load(Ordering::Relaxed) & bit != 0 {
        CPU_IDLE.fetch_add(weight, Ordering::Relaxed);
    } else {
        CPU_BUSY.fetch_add(weight, Ordering::Relaxed);
    }
    if fast && !crate::svcdev::dri::waiting() {
        fast_timer(false);
    }
    // core 0 counts ticks of the normal rate also while its timer runs fast
    if crate::percpu::id() == 0 && (!fast || FAST_SUB.fetch_add(1, Ordering::Relaxed) % FAST_DIV == FAST_DIV - 1) {
        TICKS.fetch_add(1, Ordering::Relaxed);
        let deadline = DEADLINE_MS.load(Ordering::Relaxed);
        if deadline != 0 && crate::arch::uptime_ms() > deadline {
            crate::power::neustart();
        }
    }
}

/// Uptime in ms after which the timer interrupt restarts the computer (0 = off).
static DEADLINE_MS: AtomicU64 = AtomicU64::new(0);

/// Arms the restart deadline: used by the remote hardware test so that a hanging kernel
/// gives the machine back instead of staying down until someone presses the power button.
pub fn arm_deadline(ms_from_now: u64) {
    DEADLINE_MS.store(crate::arch::uptime_ms() + ms_from_now, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

fn enable_local() {
    wr(0x80, 0); // Priorität
    wr(0xF0, 0x100 | 0xFF); // an, Störvektor 0xFF
}

/// APIC einschalten und Zeitgeber mit `HZ` starten (Vektor 32). Danach laufen Unterbrechungen.
pub unsafe fn init() {
    let b = rdmsr(0x1B);
    X2.store(b & (1 << 10) != 0, Ordering::Relaxed);
    let base = b & 0xf_ffff_f000;
    BASE.store(base, Ordering::Relaxed);
    if !X2.load(Ordering::Relaxed) {
        crate::paging::map_identity(base, base + 0x1000, true);
        wrmsr(0x1B, b | (1 << 11));
    }
    enable_local();
    // Zeitgeber am TSC eichen (10 ms, Teiler 16)
    wr(0x3E0, 0x3);
    wr(0x320, 1 << 16);
    wr(0x380, u32::MAX);
    arch::sleep_us(10_000);
    let per_10ms = u32::MAX - rd(0x390);
    let count = (per_10ms as u64 * 100 / HZ).max(1000) as u32;
    wr(0x320, 32 | (1 << 17));
    wr(0x380, count);
    TIMER_COUNT.store(count, Ordering::Relaxed);
    core::arch::asm!("sti");
    crate::kprintln!("[apic] {} · Zeitgeber {} Hz{}", if X2.load(Ordering::Relaxed) { "x2APIC" } else { "xAPIC" }, HZ, "");
}

// ------------------------------------------------------------------ ACPI

unsafe fn sig(p: u64) -> [u8; 4] {
    core::ptr::read_unaligned(p as *const [u8; 4])
}
unsafe fn u32at(p: u64) -> u32 {
    core::ptr::read_unaligned(p as *const u32)
}
unsafe fn u64at(p: u64) -> u64 {
    core::ptr::read_unaligned(p as *const u64)
}

/// ACPI-Tabelle mit Signatur suchen
pub unsafe fn find_table(rsdp: u64, want: &[u8; 4]) -> Option<u64> {
    if rsdp == 0 {
        return None;
    }
    let rev = *((rsdp + 15) as *const u8);
    let (root, wide) = if rev >= 2 && u64at(rsdp + 24) != 0 { (u64at(rsdp + 24), true) } else { (u32at(rsdp + 16) as u64, false) };
    let len = u32at(root + 4) as u64;
    let n = (len - 36) / if wide { 8 } else { 4 };
    for i in 0..n {
        let t = if wide { u64at(root + 36 + i * 8) } else { u32at(root + 36 + i * 4) as u64 };
        if t != 0 && &sig(t) == want {
            return Some(t);
        }
    }
    None
}

/// APIC-Kennungen aller eingeschalteten Prozessorkerne laut MADT
pub unsafe fn madt_cpus(rsdp: u64) -> alloc::vec::Vec<u32> {
    let mut v = alloc::vec::Vec::new();
    let Some(m) = find_table(rsdp, b"APIC") else { return v };
    let len = u32at(m + 4) as u64;
    let mut p = m + 44;
    while p + 2 <= m + len {
        let typ = *(p as *const u8);
        let l = *((p + 1) as *const u8) as u64;
        if l < 2 {
            break;
        }
        match typ {
            0 => {
                let fl = u32at(p + 4);
                if fl & 3 != 0 {
                    v.push(*((p + 3) as *const u8) as u32);
                }
            }
            9 => {
                let fl = u32at(p + 8);
                if fl & 3 != 0 {
                    v.push(u32at(p + 4));
                }
            }
            _ => {}
        }
        p += l;
    }
    v
}

// ------------------------------------------------------------------ SMP

global_asm!(
    r#"
.global ultar_ap_start
.global ultar_ap_end
.global ultar_ap_fix_gdt
.global ultar_ap_fix_j32
.global ultar_ap_fix_j64
.code16
ultar_ap_start:
    cli
    cld
    movw %cs, %ax
    movw %ax, %ds
    xorl %ebx, %ebx
    movw %cs, %bx
    shll $4, %ebx
    lgdtl (ap_gdtr - ultar_ap_start)
    movl %cr0, %eax
    orl $1, %eax
    movl %eax, %cr0
    .byte 0x66, 0xEA
ultar_ap_fix_j32:
    .long 0
    .word 0x08
.code32
ap32:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movl %cr4, %eax
    orl $0x620, %eax
    movl %eax, %cr4
    movl (ap_cr3 - ultar_ap_start)(%ebx), %eax
    movl %eax, %cr3
    movl $0xC0000080, %ecx
    movl (ap_efer - ultar_ap_start)(%ebx), %eax
    xorl %edx, %edx
    wrmsr
    movl %cr0, %eax
    orl $0x80000001, %eax
    movl %eax, %cr0
    .byte 0xEA
ultar_ap_fix_j64:
    .long 0
    .word 0x18
.code64
ap64:
    movw $0x10, %ax
    movw %ax, %ds
    movw %ax, %es
    movw %ax, %ss
    movq (ap_stack - ultar_ap_start)(%rbx), %rsp
    movq (ap_entry - ultar_ap_start)(%rbx), %rax
    callq *%rax
2:
    cli
    hlt
    jmp 2b
.balign 8
ap_gdt:
    .quad 0
    .quad 0x00cf9a000000ffff
    .quad 0x00cf92000000ffff
    .quad 0x00af9a000000ffff
ap_gdtr:
    .word 31
ultar_ap_fix_gdt:
    .long 0
.balign 8
.global ultar_ap_cr3
ultar_ap_cr3:
ap_cr3:
    .quad 0
ap_efer:
    .quad 0
ap_stack:
    .quad 0
ap_entry:
    .quad 0
.global ultar_ap_ap32
ultar_ap_ap32 = ap32
.global ultar_ap_ap64
ultar_ap_ap64 = ap64
.global ultar_ap_gdt
ultar_ap_gdt = ap_gdt
ultar_ap_end:
"#,
    options(att_syntax)
);

extern "C" {
    static ultar_ap_start: u8;
    static ultar_ap_end: u8;
    static ultar_ap_cr3: u8;
    static ultar_ap_fix_gdt: u8;
    static ultar_ap_fix_j32: u8;
    static ultar_ap_fix_j64: u8;
    static ultar_ap_ap32: u8;
    static ultar_ap_ap64: u8;
    static ultar_ap_gdt: u8;
}

extern "sysv64" fn ap_main() -> ! {
    let idx = NEXT_CPU.fetch_add(1, Ordering::SeqCst) as usize;
    if idx >= arch::MAXCPU {
        ONLINE.fetch_add(1, Ordering::SeqCst);
        loop {
            unsafe { core::arch::asm!("cli; hlt") };
        }
    }
    unsafe {
        // auf die eigentliche Kern-Seitentabelle wechseln
        core::arch::asm!("mov cr3, {}", in(reg) crate::paging::kernel_pml4());
        crate::percpu::init(idx);
        arch::load_gdt_ap();
        arch::load_tss(idx);
        arch::load_idt_ap();
        crate::trap::init_msrs();
        // Schutzbits und Speichertypen wie auf dem ersten Kern
        arch::haerten();
        arch::pat_setzen();
    }
    enable_local();
    wr(0x3E0, 0x3);
    wr(0x320, 32 | (1 << 17));
    wr(0x380, TIMER_COUNT.load(Ordering::Relaxed));
    ONLINE.fetch_add(1, Ordering::SeqCst);
    // Planer: dieser Kern nimmt bereite Prozesse auf
    crate::proc::ap_loop()
}

fn icr(dest: u32, low: u32) {
    if X2.load(Ordering::Relaxed) {
        unsafe { wrmsr(0x830, ((dest as u64) << 32) | low as u64) };
    } else {
        wr(0x310, dest << 24);
        wr(0x300, low);
        let mut n = 0;
        while rd(0x300) & (1 << 12) != 0 && n < 1_000_000 {
            n += 1;
            arch::pause();
        }
    }
}

/// Weitere Prozessorkerne starten (INIT-SIPI-SIPI). `tramp`: freie Seite unter 1 MB für den Startcode (0 = keine).
pub unsafe fn start_aps(rsdp: u64, tramp: u64) {
    let tramp_ok = tramp != 0;
    let cpus = madt_cpus(rsdp);
    *CPUS.lock() = cpus.clone();
    let me = id();
    let pml4 = crate::paging::kernel_pml4();
    if cpus.len() <= 1 {
        return crate::kprintln!("[smp] 1 Prozessorkern");
    }
    if !tramp_ok {
        return crate::kprintln!("[smp] {} Kerne gemeldet – Start nicht möglich (keine freie Seite unter 1 MB)", cpus.len());
    }
    let start = core::ptr::addr_of!(ultar_ap_start) as u64;
    let len = core::ptr::addr_of!(ultar_ap_end) as u64 - start;
    core::ptr::copy_nonoverlapping(start as *const u8, tramp as *mut u8, len as usize);
    // feste Adressen im Startcode auf die gewählte Seite umrechnen
    let off = |p: *const u8| p as u64 - start;
    *((tramp + off(core::ptr::addr_of!(ultar_ap_fix_gdt))) as *mut u32) = (tramp + off(core::ptr::addr_of!(ultar_ap_gdt))) as u32;
    *((tramp + off(core::ptr::addr_of!(ultar_ap_fix_j32))) as *mut u32) = (tramp + off(core::ptr::addr_of!(ultar_ap_ap32))) as u32;
    *((tramp + off(core::ptr::addr_of!(ultar_ap_fix_j64))) as *mut u32) = (tramp + off(core::ptr::addr_of!(ultar_ap_ap64))) as u32;
    let vars = tramp + (core::ptr::addr_of!(ultar_ap_cr3) as u64 - start);
    // oberste Seitentabelle für den Start nach unten kopieren (CR3 wird im 32-Bit-Modus geladen)
    let low = tramp + 0x1000;
    core::ptr::copy_nonoverlapping(pml4 as *const u8, low as *mut u8, 4096);
    *(vars as *mut u64) = low;
    *((vars + 8) as *mut u64) = rdmsr(0xC000_0080) & !(1 << 10);
    *((vars + 24) as *mut u64) = ap_main as *const () as usize as u64;
    for &c in cpus.iter().filter(|&&c| c != me) {
        let before = ONLINE.load(Ordering::SeqCst);
        let stack = alloc::vec![0u8; 32 * 1024].leak();
        *((vars + 16) as *mut u64) = (stack.as_ptr() as u64 + stack.len() as u64) & !15;
        icr(c, 0x4500);
        arch::sleep_us(10_000);
        for _ in 0..2 {
            icr(c, 0x4600 | (tramp >> 12) as u32);
            arch::sleep_us(200);
        }
        let t = arch::uptime_ms();
        while ONLINE.load(Ordering::SeqCst) == before && arch::uptime_ms() - t < 200 {
            arch::pause();
        }
    }
    crate::kprintln!("[smp] {} von {} Prozessorkernen aktiv", ONLINE.load(Ordering::SeqCst), cpus.len());
}

pub fn cpu_count() -> (u32, usize) {
    (ONLINE.load(Ordering::Relaxed), CPUS.lock().len().max(1))
}
