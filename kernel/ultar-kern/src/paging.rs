//! Eigene Seitentabellen.
//!
//! Kern: 1:1-Abbildung (virtuell = physisch) mit 2-MB-Seiten über den ganzen Arbeitsspeicher und jedes
//! Geräte-Fenster (BARs, APIC). Die Tabellen dafür liegen zuerst in einem festen Vorrat im Kernabbild – so
//! hängt der Kern ab dem ersten Moment nicht mehr an den Tabellen der Firmware.
//!
//! Prozesse: eigener Adressraum je Prozess. Die Kern-Einträge werden geteilt (nur Kern-Zugriff), Benutzerseiten
//! (Bit U) liegen in eigenen Tabellen. Einträge, die dem Prozess gehören, tragen Bit 9 (`OWN`).
//! Bereich für fest gebundene Programme (ET_EXEC, z. B. Ubuntu-`busybox`): 0x40_0000 .. 0x1000_0000 –
//! dort legt der Kern selbst keinen Speicher ab (siehe mem.rs).

use core::sync::atomic::{AtomicU64, Ordering};

pub const P: u64 = 1;
pub const W: u64 = 2;
pub const U: u64 = 4;
pub const PWT: u64 = 8;
pub const PCD: u64 = 16;
pub const HUGE: u64 = 0x80;
pub const OWN: u64 = 0x200;
pub const NX: u64 = 1 << 63;
const ADDR: u64 = 0x000f_ffff_ffff_f000;

/// Fenster für fest gebundene Benutzerprogramme
pub const USER_LOW: u64 = 0x40_0000;
pub const USER_LOW_END: u64 = 0x1000_0000;

#[repr(C, align(4096))]
struct Page([u64; 512]);

const POOL_N: usize = 160;
static mut POOL: [Page; POOL_N] = [const { Page([0; 512]) }; POOL_N];
static mut POOL_USED: usize = 0;
static KPML4: AtomicU64 = AtomicU64::new(0);
static NX_OK: AtomicU64 = AtomicU64::new(0);

/// Seite für eine Tabelle: erst aus dem Vorrat, dann aus dem freien Speicher
fn table_page() -> u64 {
    unsafe {
        if POOL_USED < POOL_N {
            let p = core::ptr::addr_of_mut!(POOL[POOL_USED]) as u64;
            POOL_USED += 1;
            core::ptr::write_bytes(p as *mut u8, 0, 4096);
            return p;
        }
    }
    crate::mem::alloc_page().expect("kein Speicher für Seitentabellen")
}

#[inline]
fn tab(pa: u64) -> &'static mut [u64; 512] {
    unsafe { &mut *((pa & ADDR) as *mut [u64; 512]) }
}

pub fn kernel_pml4() -> u64 {
    KPML4.load(Ordering::Relaxed)
}

/// 1:1-Abbildung für [start, end) mit 2-MB-Seiten in der Kern-Tabelle anlegen (Geräte: ohne Zwischenspeicher)
pub fn map_identity(start: u64, end: u64, device: bool) {
    let pml4 = kernel_pml4();
    let mut a = start & !0x1f_ffff;
    let flags = P | W | HUGE | if device { PCD | PWT } else { 0 };
    while a < end {
        let l4 = &mut tab(pml4)[((a >> 39) & 511) as usize];
        if *l4 & P == 0 {
            *l4 = table_page() | P | W;
        }
        let l3 = &mut tab(*l4)[((a >> 30) & 511) as usize];
        if *l3 & P == 0 {
            *l3 = table_page() | P | W;
        }
        let l2 = &mut tab(*l3)[((a >> 21) & 511) as usize];
        if *l2 & P == 0 {
            *l2 = a | flags;
        }
        a += 0x20_0000;
    }
    unsafe { flush_all() };
}

/// Eigene Tabellen aufbauen und laden. `top` = höchste physische Adresse des Arbeitsspeichers.
pub unsafe fn init(top: u64, fb: Option<(u64, u64)>) {
    let pml4 = table_page();
    KPML4.store(pml4, Ordering::Relaxed);
    // NX nur, wenn der Prozessor es kann (EFER.NXE)
    if core::arch::x86_64::__cpuid(0x8000_0001).edx & (1 << 20) != 0 {
        let efer = rdmsr(0xC000_0080);
        wrmsr(0xC000_0080, efer | (1 << 11));
        NX_OK.store(1, Ordering::Relaxed);
    }
    map_identity(0, top.max(1 << 32), false);
    if let Some((b, s)) = fb {
        map_identity(b, b + s, false);
    }
    core::arch::asm!("mov cr3, {}", in(reg) pml4);
}

pub unsafe fn flush_all() {
    let v: u64;
    core::arch::asm!("mov {}, cr3", out(reg) v);
    core::arch::asm!("mov cr3, {}", in(reg) v);
}

pub fn nx() -> u64 {
    if NX_OK.load(Ordering::Relaxed) != 0 {
        NX
    } else {
        0
    }
}

pub unsafe fn rdmsr(m: u32) -> u64 {
    let (lo, hi): (u32, u32);
    core::arch::asm!("rdmsr", in("ecx") m, out("eax") lo, out("edx") hi);
    ((hi as u64) << 32) | lo as u64
}
pub unsafe fn wrmsr(m: u32, v: u64) {
    core::arch::asm!("wrmsr", in("ecx") m, in("eax") v as u32, in("edx") (v >> 32) as u32);
}

// ------------------------------------------------------------------ Adressräume der Prozesse

pub struct AddrSpace {
    pub pml4: u64,
    /// reservierte, noch nicht belegte Bereiche (anonymes mmap): Seiten entstehen erst beim ersten Zugriff
    pub lazy: spin::Mutex<alloc::vec::Vec<(u64, u64, bool, bool)>>,
}

impl AddrSpace {
    pub fn new() -> AddrSpace {
        let p = crate::mem::alloc_page().expect("kein Speicher");
        tab(p).copy_from_slice(tab(kernel_pml4()));
        AddrSpace { pml4: p, lazy: spin::Mutex::new(alloc::vec::Vec::new()) }
    }

    /// Eintrag der nächsten Ebene holen; geteilte Kern-Tabellen/2-MB-Seiten werden für den Prozess ersetzt
    fn next(e: &mut u64, user: bool) -> u64 {
        if *e & P == 0 {
            *e = crate::mem::alloc_page().expect("kein Speicher") | P | W | U | OWN;
        } else if *e & OWN == 0 {
            let old = *e;
            let n = crate::mem::alloc_page().expect("kein Speicher");
            if old & HUGE == 0 {
                // geteilte Kern-Tabelle: Kopie (Kern-Einträge bleiben ohne U)
                tab(n).copy_from_slice(tab(old));
            }
            // 2-MB-Kernseite im Benutzerfenster: entfällt im Prozess
            *e = n | P | W | U | OWN;
        }
        if user {
            *e |= U;
        }
        *e & ADDR
    }

    fn leaf(&self, va: u64) -> &'static mut u64 {
        let l4 = &mut tab(self.pml4)[((va >> 39) & 511) as usize];
        let t3 = Self::next(l4, true);
        let l3 = &mut tab(t3)[((va >> 30) & 511) as usize];
        let t2 = Self::next(l3, true);
        let l2 = &mut tab(t2)[((va >> 21) & 511) as usize];
        let t1 = Self::next(l2, true);
        &mut tab(t1)[((va >> 12) & 511) as usize]
    }

    /// Benutzerseite bei `va` (genullt) bereitstellen; liefert die physische Adresse
    pub fn map(&mut self, va: u64, writable: bool, exec: bool) -> u64 {
        self.map_inner(va, writable, exec)
    }

    fn map_inner(&self, va: u64, writable: bool, exec: bool) -> u64 {
        let e = self.leaf(va);
        if *e & P != 0 {
            if writable {
                *e |= W;
            }
            if exec {
                *e &= !NX;
            }
            return *e & ADDR;
        }
        let pa = crate::mem::alloc_page().expect("kein Speicher für Benutzerseite");
        *e = pa | P | U | OWN | if writable { W } else { 0 } | if exec { 0 } else { nx() };
        pa
    }

    pub fn unmap(&mut self, va: u64) {
        if let Some(e) = self.walk(va) {
            if *e & P != 0 && *e & OWN != 0 {
                crate::mem::free_page(*e & ADDR);
                *e = 0;
                unsafe { core::arch::asm!("invlpg [{}]", in(reg) va) };
            }
        }
    }

    fn walk(&self, va: u64) -> Option<&'static mut u64> {
        let mut t = self.pml4;
        for sh in [39u64, 30, 21] {
            let e = tab(t)[((va >> sh) & 511) as usize];
            if e & P == 0 || e & OWN == 0 || e & HUGE != 0 {
                return None;
            }
            t = e & ADDR;
        }
        Some(&mut tab(t)[((va >> 12) & 511) as usize])
    }

    /// physische Adresse einer Benutzerseite (nur eigene Seiten)
    pub fn translate(&self, va: u64) -> Option<(u64, bool)> {
        if let Some(e) = self.walk(va) {
            let e = *e;
            if e & P != 0 && e & U != 0 {
                return Some(((e & ADDR) | (va & 0xfff), e & W != 0));
            }
        }
        // Kern greift auf eine reservierte, noch leere Seite zu → jetzt belegen
        if self.fault_in(va) {
            let e = *self.walk(va)?;
            return Some(((e & ADDR) | (va & 0xfff), e & W != 0));
        }
        None
    }

    /// Bereich reservieren (Seiten beim ersten Zugriff)
    pub fn reserve(&self, va: u64, len: u64, w: bool, x: bool) {
        self.lazy.lock().push((va, va + len, w, x));
    }

    /// Reservierung (teilweise) aufheben
    pub fn unreserve(&self, va: u64, len: u64) {
        let end = va + len;
        let mut l = self.lazy.lock();
        let mut neu = alloc::vec::Vec::with_capacity(l.len() + 1);
        for &(a, b, w, x) in l.iter() {
            if b <= va || a >= end {
                neu.push((a, b, w, x));
                continue;
            }
            if a < va {
                neu.push((a, va, w, x));
            }
            if b > end {
                neu.push((end, b, w, x));
            }
        }
        *l = neu;
    }

    /// Seitenfehler in einem reservierten Bereich: genullte Seite einsetzen
    pub fn fault_in(&self, va: u64) -> bool {
        let hit = self.lazy.lock().iter().find(|r| va >= r.0 && va < r.1).map(|r| (r.2, r.3));
        match hit {
            Some((w, x)) => {
                self.map_inner(va & !0xfff, w, x);
                true
            }
            None => false,
        }
    }

    /// alle eigenen Benutzerseiten als (va, pa, flags) besuchen
    pub fn for_each(&self, mut f: impl FnMut(u64, u64, u64)) {
        fn rec(t: u64, lvl: u32, base: u64, f: &mut dyn FnMut(u64, u64, u64)) {
            for (i, &e) in tab(t).iter().enumerate() {
                if e & P == 0 || e & OWN == 0 {
                    continue;
                }
                let va = base | ((i as u64) << (12 + 9 * lvl));
                if lvl == 0 {
                    f(va, e & ADDR, e);
                } else {
                    rec(e & ADDR, lvl - 1, va, f);
                }
            }
        }
        rec(self.pml4, 3, 0, &mut f);
    }

    /// Kopie für fork(): jede Seite wird kopiert
    pub fn fork(&self) -> AddrSpace {
        let mut n = AddrSpace::new();
        *n.lazy.lock() = self.lazy.lock().clone();
        self.for_each(|va, pa, fl| {
            let va = if va & (1 << 47) != 0 { va | 0xffff_0000_0000_0000 } else { va };
            let np = n.map(va, fl & W != 0, fl & NX == 0);
            unsafe { core::ptr::copy_nonoverlapping(pa as *const u8, np as *mut u8, 4096) };
        });
        n
    }

    pub fn activate(&self) {
        unsafe { core::arch::asm!("mov cr3, {}", in(reg) self.pml4) };
    }
}

impl Drop for AddrSpace {
    fn drop(&mut self) {
        fn rec(t: u64, lvl: u32) {
            for &e in tab(t).iter() {
                if e & P == 0 || e & OWN == 0 {
                    continue;
                }
                if lvl > 0 {
                    rec(e & ADDR, lvl - 1);
                }
                crate::mem::free_page(e & ADDR);
            }
        }
        rec(self.pml4, 3);
        crate::mem::free_page(self.pml4);
    }
}
