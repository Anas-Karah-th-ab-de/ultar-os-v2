//! Daten je Prozessorkern, erreichbar über GS (im Kern: GS_BASE = eigener Eintrag; im Benutzermodus per
//! `swapgs` getauscht). Feste Versätze werden vom Einsprung für `syscall` benutzt.

use crate::arch::MAXCPU;
use crate::paging::wrmsr;

#[repr(C)]
pub struct PerCpu {
    /// 0: Benutzerstapel beim `syscall`
    pub user_rsp: u64,
    /// 8: Kernstapel des laufenden Fadens
    pub kstack_top: u64,
    /// 16: laufender Faden (0 = keiner)
    pub current: u64,
    /// 24: gesicherter Stapel des Planers
    pub sched_ctx: u64,
    /// 32: Kernnummer
    pub id: u64,
    /// 40: Selbstverweis
    pub this: u64,
    /// 48: Rücksprungadresse des laufenden Systemaufrufs (Fristen von poll & Co.)
    pub sys_rip: u64,
    /// 56: resource group that allocations on this processor are charged to (see heap.rs)
    pub charge: u64,
    /// 64: nesting depth of `heap::fallible` (requests may be refused by policy)
    pub fallible: u64,
    /// 72: reason of the last refused request + 1 (0 = none) since the system call began
    pub denied: u64,
    /// 80: error number a lower layer wants the running system call to report (0 = none)
    pub errno: u64,
}

pub static mut CPUS: [PerCpu; MAXCPU] = [const {
    PerCpu {
        user_rsp: 0,
        kstack_top: 0,
        current: 0,
        sched_ctx: 0,
        id: 0,
        this: 0,
        sys_rip: 0,
        charge: 0,
        fallible: 0,
        denied: 0,
        errno: 0,
    }
}; MAXCPU];
static READY: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// GS_BASE auf den Eintrag des Kerns setzen
pub unsafe fn init(cpu: usize) {
    let p = core::ptr::addr_of_mut!(CPUS[cpu]);
    (*p).id = cpu as u64;
    (*p).this = p as u64;
    wrmsr(0xC000_0101, p as u64); // GS_BASE
    wrmsr(0xC000_0102, 0); // KERNEL_GS_BASE (Benutzerwert)
    READY.store(true, core::sync::atomic::Ordering::Relaxed);
}

#[inline]
pub fn me() -> &'static mut PerCpu {
    unsafe {
        if !READY.load(core::sync::atomic::Ordering::Relaxed) {
            return &mut *core::ptr::addr_of_mut!(CPUS[0]);
        }
        let p: u64;
        core::arch::asm!("mov {}, gs:[40]", out(reg) p, options(nostack, readonly, preserves_flags));
        &mut *(p as *mut PerCpu)
    }
}

#[inline]
pub fn id() -> usize {
    me().id as usize
}
