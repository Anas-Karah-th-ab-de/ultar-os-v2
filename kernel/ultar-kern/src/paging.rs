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
/// Bit 10 (free for software): the page holds a copy of a file below a `noexec` mount – `mprotect`
/// never makes it executable.
pub const NOEXEC_FILE: u64 = 0x400;
pub const NX: u64 = 1 << 63;
const ADDR: u64 = 0x000f_ffff_ffff_f000;

/// Fenster für fest gebundene Benutzerprogramme
pub const USER_LOW: u64 = 0x40_0000;
pub const USER_LOW_END: u64 = 0x1000_0000;
/// Second low window, for Windows programs (docs/integration/kern-wine.md): Wine's preloader is linked at
/// 0x7d40_0000 and every Windows program expects the shared user data page at 0x7ffe_0000. The kernel
/// takes no memory from this range (mem.rs); it is open only when the firmware map holds nothing but free
/// memory or nothing at all there (see `set_win_window`).
pub const WIN_LOW: u64 = 0x7c00_0000;
pub const WIN_END: u64 = 0x8000_0000;
/// Lowest address of the upper user area (below it lies the kernel's identity map).
pub const USER_HIGH: u64 = 0x1000_0000_0000;
/// End of the user half.
pub const USER_TOP: u64 = 0x8000_0000_0000;
static WIN_OK: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);
static FB_IN_WIN: core::sync::atomic::AtomicBool = core::sync::atomic::AtomicBool::new(false);

/// Whether the firmware framebuffer lies in the second low window (it then stays closed).
pub fn fb_in_win_window() -> bool {
    FB_IN_WIN.load(Ordering::Relaxed)
}

/// Open or close the second low window (decided once at start-up from the firmware map).
pub fn set_win_window(open: bool) {
    WIN_OK.store(open, Ordering::Relaxed);
}

/// Whether the second low window is open.
pub fn win_window() -> bool {
    WIN_OK.load(Ordering::Relaxed)
}

/// The low window (start, end) that holds all of `[va, end)`, if any.
pub fn low_window(va: u64, end: u64) -> Option<(u64, u64)> {
    if va >= USER_LOW && end <= USER_LOW_END && va <= end {
        return Some((USER_LOW, USER_LOW_END));
    }
    if win_window() && va >= WIN_LOW && end <= WIN_END && va <= end {
        return Some((WIN_LOW, WIN_END));
    }
    None
}

/// Whether user pages may lie in `[va, va + len)`: one of the low windows or the upper user area.
pub fn user_range(va: u64, len: u64) -> bool {
    let Some(end) = va.checked_add(len) else { return false };
    low_window(va, end).is_some() || (va >= USER_HIGH && end <= USER_TOP)
}

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
    // identity map of the kernel (start-up, device windows): forced, outside every group
    crate::mem::alloc_page_kernel().expect("kein Speicher für Seitentabellen")
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
        FB_IN_WIN.store(b < WIN_END && b + s > WIN_LOW, Ordering::Relaxed);
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
    /// user pages mapped right now (resident size, without page tables)
    pub resident: AtomicU64,
    /// no reserved range reaches up to or beyond this address (lets `unreserve` skip the search for
    /// a fresh address above everything – the usual case of `mmap` without a fixed address)
    lazy_end: AtomicU64,
    /// shared mappings (`MAP_SHARED` of a memory object, the framebuffer): (start, end, what). Their
    /// pages carry no ownership bit – nothing here frees them; the entry keeps the object alive.
    pub shared: spin::Mutex<alloc::vec::Vec<(u64, u64, crate::memfd::Backing)>>,
}

/// Most reserved ranges (anonymous mappings) of one address space, like Linux' `max_map_count`.
pub const MAX_RANGES: usize = 65530;

mod shared;

/// Outcome of a page fault in an address space.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// A page was put in place.
    Mapped,
    /// The address lies in no reserved range.
    NotReserved,
    /// The range is reserved but no page could be had (see `heap::last_denial`).
    NoMemory,
}

impl AddrSpace {
    /// Empty address space that shares the kernel's mappings; `None` without memory.
    pub fn new() -> Option<AddrSpace> {
        let p = crate::mem::alloc_page()?;
        tab(p).copy_from_slice(tab(kernel_pml4()));
        Some(AddrSpace { pml4: p, lazy: spin::Mutex::new(alloc::vec::Vec::new()), resident: AtomicU64::new(0), lazy_end: AtomicU64::new(0), shared: spin::Mutex::new(alloc::vec::Vec::new()) })
    }

    /// Eintrag der nächsten Ebene holen; geteilte Kern-Tabellen/2-MB-Seiten werden für den Prozess ersetzt
    fn next(e: &mut u64, user: bool) -> Option<u64> {
        if *e & P == 0 {
            *e = crate::mem::alloc_page()? | P | W | U | OWN;
        } else if *e & OWN == 0 {
            let old = *e;
            let n = crate::mem::alloc_page()?;
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
        Some(*e & ADDR)
    }

    fn leaf(&self, va: u64) -> Option<&'static mut u64> {
        let l4 = &mut tab(self.pml4)[((va >> 39) & 511) as usize];
        let t3 = Self::next(l4, true)?;
        let l3 = &mut tab(t3)[((va >> 30) & 511) as usize];
        let t2 = Self::next(l3, true)?;
        let l2 = &mut tab(t2)[((va >> 21) & 511) as usize];
        // a 2-MiB kernel page in a user window gives way to an own table: its translation must not
        // stay in this core's TLB (it would keep the new user pages unreachable)
        let huge = *l2 & P != 0 && *l2 & HUGE != 0;
        let t1 = Self::next(l2, true)?;
        if huge {
            // SAFETY: invalidates the translation of one address; no memory is touched.
            unsafe { core::arch::asm!("invlpg [{}]", in(reg) va) };
        }
        Some(&mut tab(t1)[((va >> 12) & 511) as usize])
    }

    /// A user access at `va` in a low window met a 2-MiB kernel page of the shared tables (present,
    /// without the user bit): the window's page is not mapped yet – a reserved range there is put in
    /// place like an absent page.
    pub fn kernel_page_at(&self, va: u64) -> bool {
        if low_window(va, va + 1).is_none() {
            return false;
        }
        let mut t = self.pml4;
        for sh in [39u64, 30, 21] {
            let e = tab(t)[((va >> sh) & 511) as usize];
            if e & P == 0 {
                return false;
            }
            if e & HUGE != 0 {
                return e & OWN == 0;
            }
            t = e & ADDR;
        }
        false
    }

    /// The page at `va` already allows the access (`write`, `exec`): the fault came from a stale
    /// translation of another core (a kernel page this window replaced) – nothing to do but retry.
    pub fn allows(&self, va: u64, write: bool, exec: bool) -> bool {
        let Some(e) = self.walk(va) else { return false };
        let e = *e;
        e & P != 0 && e & U != 0 && (!write || e & W != 0) && (!exec || e & nx() == 0)
    }

    /// Benutzerseite bei `va` (genullt) bereitstellen; liefert die physische Adresse.
    /// `None`: no memory (limit of the group or the machine) – nothing was mapped at `va`.
    pub fn map(&self, va: u64, writable: bool, exec: bool) -> Option<u64> {
        self.map_inner(va, writable, exec)
    }

    fn map_inner(&self, va: u64, writable: bool, exec: bool) -> Option<u64> {
        let e = self.leaf(va)?;
        if *e & P != 0 {
            if writable {
                *e |= W;
            }
            if exec {
                *e &= !NX;
            }
            return Some(*e & ADDR);
        }
        let pa = crate::mem::alloc_page()?;
        *e = pa | P | U | OWN | if writable { W } else { 0 } | if exec { 0 } else { nx() };
        self.resident.fetch_add(1, Ordering::Relaxed);
        Some(pa)
    }

    /// Marks the page at `va` as a copy of a `noexec` file (see [`NOEXEC_FILE`]).
    pub fn mark_noexec_file(&self, va: u64) {
        if let Some(e) = self.walk(va) {
            if *e & P != 0 {
                *e |= NOEXEC_FILE;
            }
        }
    }

    /// Whether `mprotect` may give the pages of `[va, va + len)` the rights `w`/`x`: never
    /// executable for a copy of a `noexec` file, and a shared page (a memory object, the
    /// framebuffer) gets no right its mapping did not have.
    pub fn may_protect(&self, va: u64, len: u64, w: bool, x: bool) -> bool {
        let mut a = va;
        while a < va + len {
            if let Some(e) = self.walk(a) {
                let e = *e;
                if e & P != 0 {
                    if x && (e & NOEXEC_FILE != 0 || (e & OWN == 0 && e & nx() != 0)) {
                        return false;
                    }
                    if w && e & OWN == 0 && e & W == 0 {
                        return false;
                    }
                }
            }
            a += 4096;
        }
        true
    }

    /// `mprotect` of the mapped page at `va`: exactly writable `w`, executable `x` (readable always –
    /// x86 has no write-only or no-access user page). Nothing for an unmapped page.
    pub fn protect(&self, va: u64, w: bool, x: bool) {
        if let Some(e) = self.walk(va) {
            if *e & P != 0 && *e & U != 0 {
                *e = (*e & !(W | NX)) | if w { W } else { 0 } | if x { 0 } else { nx() };
                // SAFETY: invalidates the translation of one user address; no memory is touched.
                unsafe { core::arch::asm!("invlpg [{}]", in(reg) va) };
            }
        }
    }

    /// Gives the reserved, not yet touched parts of `[va, va + len)` the rights `w`/`x`, so pages
    /// that appear there later get them. `false`: no room for the split ranges – nothing changed.
    pub fn relabel(&self, va: u64, len: u64, w: bool, x: bool) -> bool {
        let end = va + len;
        let parts = {
            let l = self.lazy.lock();
            let n = l.iter().filter(|r| r.0 < end && r.1 > va).count();
            if n == 0 {
                return true;
            }
            // the ranges are split at both ends at most once more, plus the relabelled parts
            if l.len() + n + 2 > MAX_RANGES {
                return false;
            }
            let Some(mut parts) = crate::mem::try_with_capacity::<(u64, u64)>(n) else { return false };
            parts.extend(l.iter().filter(|r| r.0 < end && r.1 > va).map(|r| (r.0.max(va), r.1.min(end))));
            parts
        };
        if !self.unreserve(va, len) {
            return false;
        }
        parts.iter().all(|&(a, b)| self.reserve(a, b - a, w, x))
    }

    pub fn unmap(&self, va: u64) {
        if let Some(e) = self.walk(va) {
            if *e & P != 0 && *e & U != 0 {
                // an own page is freed; a shared one (no ownership bit) only leaves this address space
                if *e & OWN != 0 {
                    crate::mem::free_page(*e & ADDR);
                    self.resident.fetch_sub(1, Ordering::Relaxed);
                }
                *e = 0;
                // SAFETY: invalidates the translation of one user address; no memory is touched.
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
        if self.fault_in(va) == Fault::Mapped {
            let e = *self.walk(va)?;
            return Some(((e & ADDR) | (va & 0xfff), e & W != 0));
        }
        None
    }

    /// Bereich reservieren (Seiten beim ersten Zugriff). `false`: the address space already holds
    /// [`MAX_RANGES`] ranges or the list cannot grow – nothing was reserved.
    pub fn reserve(&self, va: u64, len: u64, w: bool, x: bool) -> bool {
        let mut l = self.lazy.lock();
        if l.len() >= MAX_RANGES || !crate::mem::try_reserve(&mut l, 1) {
            return false;
        }
        l.push((va, va + len, w, x));
        self.lazy_end.fetch_max(va + len, Ordering::Relaxed);
        true
    }

    /// Reservierung (teilweise) aufheben. `false`: a range would have to be split and the list
    /// cannot grow (see `reserve`) – nothing was changed.
    pub fn unreserve(&self, va: u64, len: u64) -> bool {
        let end = va + len;
        let mut l = self.lazy.lock();
        if va >= self.lazy_end.load(Ordering::Relaxed) {
            return true;
        }
        // a range that covers the hole on both sides is split in two: one more entry each
        let splits = l.iter().filter(|r| r.0 < va && r.1 > end).count();
        if splits > 0 && (l.len() + splits > MAX_RANGES || !crate::mem::try_reserve(&mut l, splits)) {
            return false;
        }
        for i in 0..l.len() {
            let (a, b, w, x) = l[i];
            if a < va && b > end {
                l[i].1 = va;
                l.push((end, b, w, x));
            }
        }
        l.retain_mut(|r| {
            if r.1 <= va || r.0 >= end {
                true
            } else if r.0 < va {
                r.1 = va;
                true
            } else if r.1 > end {
                r.0 = end;
                true
            } else {
                false
            }
        });
        true
    }

    /// Whether nothing of this address space lies in `[va, va + len)`: no reserved range, no shared
    /// mapping, no own user page (`mmap` with an address wish or `MAP_FIXED_NOREPLACE`).
    pub fn range_free(&self, va: u64, len: u64) -> bool {
        let end = va + len;
        if self.lazy.lock().iter().any(|r| r.0 < end && r.1 > va) || self.has_shared(va, len) {
            return false;
        }
        !self.any_page(va, end)
    }

    /// Whether an own user page lies in `[va, end)`; tables that are absent or the kernel's are skipped
    /// a whole level at a time (a reservation of gigabytes costs a few steps).
    fn any_page(&self, va: u64, end: u64) -> bool {
        let mut a = va;
        'outer: while a < end {
            let mut t = self.pml4;
            for sh in [39u64, 30, 21] {
                let e = tab(t)[((a >> sh) & 511) as usize];
                if e & P == 0 || e & OWN == 0 || e & HUGE != 0 {
                    a = ((a >> sh) + 1) << sh;
                    continue 'outer;
                }
                t = e & ADDR;
            }
            if tab(t)[((a >> 12) & 511) as usize] & P != 0 {
                return true;
            }
            a += 4096;
        }
        false
    }

    /// Seitenfehler in einem reservierten Bereich: genullte Seite einsetzen
    pub fn fault_in(&self, va: u64) -> Fault {
        let hit = self.lazy.lock().iter().find(|r| va >= r.0 && va < r.1).map(|r| (r.2, r.3));
        match hit {
            Some((w, x)) => match self.map_inner(va & !0xfff, w, x) {
                Some(_) => Fault::Mapped,
                None => Fault::NoMemory,
            },
            None => Fault::NotReserved,
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

    /// Kopie für fork(): jede Seite wird kopiert. `None`: no memory for the copy (what was copied
    /// so far is freed again).
    pub fn fork(&self) -> Option<AddrSpace> {
        let n = AddrSpace::new()?;
        {
            let from = self.lazy.lock();
            let mut to = crate::mem::try_with_capacity(from.len())?;
            to.extend_from_slice(&from);
            *n.lazy.lock() = to;
            n.lazy_end.store(self.lazy_end.load(Ordering::Relaxed), Ordering::Relaxed);
        }
        let mut ok = true;
        self.for_each(|va, pa, fl| {
            if !ok {
                return;
            }
            let va = if va & (1 << 47) != 0 { va | 0xffff_0000_0000_0000 } else { va };
            match n.map(va, fl & W != 0, fl & NX == 0) {
                Some(np) => {
                    unsafe { core::ptr::copy_nonoverlapping(pa as *const u8, np as *mut u8, 4096) };
                    if fl & NOEXEC_FILE != 0 {
                        n.mark_noexec_file(va);
                    }
                }
                None => ok = false,
            }
        });
        (ok && self.fork_shared(&n)).then_some(n)
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
