//! Shared mappings of an address space (`MAP_SHARED` of a memory object, the framebuffer) and the
//! page moves of `mremap`. Shared pages carry no ownership bit: nothing here frees them; the entry in
//! `AddrSpace::shared` keeps their object alive.

use super::{nx, AddrSpace, ADDR, OWN, P, U, W};

impl AddrSpace {
    /// Map `frames` (pages this address space does not own) at `va` and remember `what` keeps them alive.
    /// `false`: no memory for a page table or the list – nothing stays mapped.
    pub fn map_shared(
        &self,
        va: u64,
        frames: &[u64],
        writable: bool,
        what: crate::memfd::Backing,
    ) -> bool {
        let end = va + frames.len() as u64 * 4096;
        {
            let mut l = self.shared.lock();
            if !crate::mem::try_reserve(&mut l, 1) {
                return false;
            }
            l.push((va, end, what));
        }
        for (i, &pa) in frames.iter().enumerate() {
            let a = va + i as u64 * 4096;
            self.unmap(a);
            let Some(e) = self.leaf(a) else {
                self.unmap_shared(va, end - va);
                return false;
            };
            *e = pa | P | U | if writable { W } else { 0 } | nx();
        }
        true
    }

    /// Remove the shared mappings inside `[va, va + len)`: the pages leave the page table, entries that
    /// lie wholly inside are dropped (a partly unmapped entry keeps its object until it is gone entirely).
    pub fn unmap_shared(&self, va: u64, len: u64) {
        let end = va + len;
        let mut l = self.shared.lock();
        if !l.iter().any(|r| r.0 < end && r.1 > va) {
            return;
        }
        for r in l.iter().filter(|r| r.0 < end && r.1 > va) {
            let mut a = r.0.max(va);
            while a < r.1.min(end) {
                self.unmap(a);
                a += 4096;
            }
        }
        l.retain(|r| !(r.0 >= va && r.1 <= end));
    }

    /// Take out the shared mapping that starts at `va` and lies in `[va, va + len)` (its pages leave the
    /// page table).
    pub fn take_shared(&self, va: u64, len: u64) -> Option<crate::memfd::Backing> {
        let mut l = self.shared.lock();
        let i = l.iter().position(|r| r.0 == va && r.1 <= va + len)?;
        let (_, end, what) = l.swap_remove(i);
        drop(l);
        let mut a = va;
        while a < end {
            self.unmap(a);
            a += 4096;
        }
        Some(what)
    }

    /// Move the own page at `from` (if any) to `to`, where nothing is mapped. `false`: no page table.
    pub fn move_page(&self, from: u64, to: u64) -> bool {
        let Some(e) = self.walk(from) else {
            return true;
        };
        if *e & P == 0 || *e & OWN == 0 {
            return true;
        }
        let Some(t) = self.leaf(to) else { return false };
        *t = *e;
        *e = 0;
        // SAFETY: invalidates the translation of one user address; no memory is touched.
        unsafe { core::arch::asm!("invlpg [{}]", in(reg) from) };
        true
    }

    /// Access rights (write, execute) of the reserved range holding `va`.
    pub fn reserved_at(&self, va: u64) -> Option<(bool, bool)> {
        self.lazy
            .lock()
            .iter()
            .find(|r| va >= r.0 && va < r.1)
            .map(|r| (r.2, r.3))
    }

    /// Whether a shared mapping touches `[va, va + len)`.
    pub fn has_shared(&self, va: u64, len: u64) -> bool {
        self.shared
            .lock()
            .iter()
            .any(|r| r.0 < va + len && r.1 > va)
    }

    /// The child of `fork` maps the same shared pages (they stay shared, as on Linux).
    pub(super) fn fork_shared(&self, n: &AddrSpace) -> bool {
        let l = self.shared.lock();
        for (a, b, what) in l.iter() {
            let mut frames = alloc::vec::Vec::new();
            let mut writable = false;
            let mut v = *a;
            while v < *b {
                match self.walk(v) {
                    Some(e) if *e & P != 0 => {
                        writable |= *e & W != 0;
                        if !crate::mem::try_reserve(&mut frames, 1) {
                            return false;
                        }
                        frames.push(*e & ADDR);
                    }
                    _ => break,
                }
                v += 4096;
            }
            if frames.len() as u64 * 4096 == b - a
                && !n.map_shared(*a, &frames, writable, what.clone())
            {
                return false;
            }
        }
        true
    }
}
