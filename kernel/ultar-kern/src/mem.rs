//! Speicher: Seitenrahmen aus der UEFI-Speicherkarte (für DMA-Puffer der Treiber) und Kern-Heap (alloc).
//! Die Firmware hat allen Speicher 1:1 abgebildet (virtuell = physisch); das behalten wir vorerst bei.

use crate::efi::Boot;
use linked_list_allocator::LockedHeap;

#[global_allocator]
static HEAP: LockedHeap = LockedHeap::empty();

pub const PAGE: u64 = 4096;

struct Region {
    start: u64,
    end: u64,
}

const MAXR: usize = 256;
static mut REGIONS: [Region; MAXR] = [const { Region { start: 0, end: 0 } }; MAXR];
static mut NREG: usize = 0;
static mut TOTAL: u64 = 0;
static mut HEAP_SIZE: u64 = 0;
static FRAMES: spin::Mutex<(usize, u64)> = spin::Mutex::new((0, 0)); // (Bereich, nächste Adresse)

/// höchste Adresse des Arbeitsspeichers laut Firmware-Karte
pub fn top(boot: &Boot) -> u64 {
    boot.regions().filter(|d| matches!(d.typ, 1..=7 | 9 | 10)).map(|d| d.phys + d.pages * PAGE).max().unwrap_or(1 << 32)
}

unsafe fn add(start: u64, end: u64) {
    if end > start + PAGE && NREG < MAXR {
        TOTAL += end - start;
        REGIONS[NREG] = Region { start, end };
        NREG += 1;
    }
}

pub unsafe fn init(boot: &Boot) {
    // Stapel der Firmware (auf ihm laufen wir noch) und das Benutzerfenster für fest gebundene Programme
    // (siehe paging.rs) bleiben frei; unter 1 MB: alte BIOS-Bereiche und Startcode der weiteren Prozessorkerne.
    let rsp: u64;
    core::arch::asm!("mov {}, rsp", out(reg) rsp);
    let (s0, s1) = ((rsp & !0x1f_ffff).saturating_sub(0x20_0000), (rsp | 0x1f_ffff) + 1);
    let low = crate::paging::USER_LOW_END;
    for d in boot.regions() {
        if !crate::efi::usable(d.typ) {
            continue;
        }
        let start = d.phys.max(low);
        let end = d.phys + d.pages * PAGE;
        if end <= start {
            continue;
        }
        if start < s1 && s0 < end {
            add(start, s0.max(start));
            add(s1.min(end), end);
        } else {
            add(start, end);
        }
    }
    // größter Bereich: ein Viertel (max. 256 MB) wird Heap
    let mut best = 0;
    for i in 0..NREG {
        if REGIONS[i].end - REGIONS[i].start > REGIONS[best].end - REGIONS[best].start {
            best = i;
        }
    }
    let r = &mut REGIONS[best];
    let size = ((r.end - r.start) / 4).min(256 << 20).max(4 << 20) & !(PAGE - 1);
    HEAP.lock().init(r.start as *mut u8, size as usize);
    HEAP_SIZE = size;
    r.start += size;
    *FRAMES.lock() = (0, REGIONS[0].start);
}

/// `n` zusammenhängende, genullte Seiten (für DMA: physische Adresse = Zeiger)
pub fn alloc_pages(n: usize) -> Option<u64> {
    let mut g = FRAMES.lock();
    unsafe {
        while g.0 < NREG {
            let r = &REGIONS[g.0];
            let a = (g.1.max(r.start) + PAGE - 1) & !(PAGE - 1);
            if a + n as u64 * PAGE <= r.end {
                g.1 = a + n as u64 * PAGE;
                core::ptr::write_bytes(a as *mut u8, 0, n * PAGE as usize);
                return Some(a);
            }
            g.0 += 1;
            if g.0 < NREG {
                g.1 = REGIONS[g.0].start;
            }
        }
    }
    None
}

static FREE: spin::Mutex<alloc::vec::Vec<u64>> = spin::Mutex::new(alloc::vec::Vec::new());
static FREE_N: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// eine genullte Seite (wiederverwendet freigegebene Seiten)
pub fn alloc_page() -> Option<u64> {
    if let Some(p) = FREE.lock().pop() {
        FREE_N.fetch_sub(1, core::sync::atomic::Ordering::Relaxed);
        unsafe { core::ptr::write_bytes(p as *mut u8, 0, PAGE as usize) };
        return Some(p);
    }
    alloc_pages(1)
}

pub fn free_page(p: u64) {
    FREE.lock().push(p);
    FREE_N.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

/// noch nicht vergebener Speicher in Bytes (ohne Heap)
pub fn free_bytes() -> u64 {
    let g = FRAMES.lock();
    let mut n = FREE_N.load(core::sync::atomic::Ordering::Relaxed) * PAGE;
    unsafe {
        for i in g.0..NREG {
            let r = &REGIONS[i];
            n += r.end - if i == g.0 { g.1.max(r.start) } else { r.start };
        }
    }
    n
}

pub fn stats() -> (u64, u64, u64, u64) {
    let h = HEAP.lock();
    unsafe { (TOTAL, HEAP_SIZE, h.used() as u64, NREG as u64) }
}
