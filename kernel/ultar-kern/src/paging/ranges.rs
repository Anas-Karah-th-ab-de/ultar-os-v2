//! Ranges of an address space: what lies where (`/proc/<pid>/maps`, free for an address wish), the
//! unmapping of large ranges and the faults of the low windows over the kernel's 2-MiB pages.

use super::{low_window, nx, tab, AddrSpace, ADDR, HUGE, OWN, P, U, USER_HIGH, USER_LOW, USER_LOW_END, USER_TOP, W, WIN_END, WIN_LOW};

impl AddrSpace {
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
    /// Every range of the address space for `/proc/<pid>/maps`: (start, end, writable, executable,
    /// shared), sorted – reserved ranges, shared mappings and the mapped pages outside them (program
    /// image, heap, stack). Neighbours with the same rights are joined.
    pub fn ranges(&self) -> alloc::vec::Vec<(u64, u64, bool, bool, bool)> {
        use alloc::vec::Vec;
        let mut prim: Vec<(u64, u64, bool, bool, bool)> = self.lazy.lock().iter().map(|r| (r.0, r.1, r.2, r.3, false)).collect();
        prim.extend(self.shared.lock().iter().map(|r| (r.0, r.1, true, false, true)));
        prim.sort_by_key(|r| r.0);
        let mut runs: Vec<(u64, u64, bool, bool)> = Vec::new();
        for (lo, hi) in [(USER_LOW, USER_LOW_END), (WIN_LOW, WIN_END), (USER_HIGH, USER_TOP)] {
            self.page_runs(lo, hi, &mut runs);
        }
        let mut out = prim.clone();
        let mut j = 0;
        for (mut a, b, w, x) in runs {
            while j < prim.len() && prim[j].1 <= a {
                j += 1;
            }
            let mut k = j;
            while a < b {
                if k < prim.len() && prim[k].0 <= a {
                    a = a.max(prim[k].1);
                    k += 1;
                    continue;
                }
                let end = if k < prim.len() { b.min(prim[k].0) } else { b };
                out.push((a, end, w, x, false));
                a = end;
            }
        }
        out.sort_by_key(|r| r.0);
        let mut merged: Vec<(u64, u64, bool, bool, bool)> = Vec::new();
        for r in out {
            match merged.last_mut() {
                Some(l) if l.1 == r.0 && (l.2, l.3, l.4) == (r.2, r.3, r.4) => l.1 = r.1,
                _ => merged.push(r),
            }
        }
        merged
    }
    /// Runs of own, mapped user pages in `[va, end)`: (start, end, writable, executable).
    pub(super) fn page_runs(&self, va: u64, end: u64, out: &mut alloc::vec::Vec<(u64, u64, bool, bool)>) {
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
            let e = tab(t)[((a >> 12) & 511) as usize];
            if e & P != 0 && e & U != 0 && e & OWN != 0 {
                let (w, x) = (e & W != 0, e & nx() == 0);
                match out.last_mut() {
                    Some(l) if l.1 == a && l.2 == w && l.3 == x => l.1 = a + 4096,
                    _ => out.push((a, a + 4096, w, x)),
                }
            }
            a += 4096;
        }
    }
    /// Unmaps every page of `[va, end)`; levels without own tables are skipped whole (a 64-GiB
    /// reservation is gone in a few steps, not 16 million).
    pub fn unmap_range(&self, va: u64, end: u64) {
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
                self.unmap(a);
            }
            a += 4096;
        }
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
    pub(super) fn any_page(&self, va: u64, end: u64) -> bool {
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
}
