//! Where a new mapping goes (`mmap`, `mremap`): a fixed address, an address wish, `MAP_FIXED_NOREPLACE`,
//! `MAP_32BIT`, or the next free address above everything (docs/integration/kern-wine.md).
//!
//! The user half has three areas (paging.rs): the window for fixed-address programs, the second low
//! window (Wine's preloader, the Windows shared user data page) and the upper user area. Below the
//! upper area lies the kernel's identity map; nothing of a process may go there.

use crate::paging::{self, USER_LOW, USER_LOW_END, WIN_END, WIN_LOW};
use crate::proc::{self, Proc};

const EEXIST: i64 = 17;
const ENOMEM: i64 = 12;
const EINVAL: i64 = 22;

const MAP_FIXED: u64 = 0x10;
const MAP_32BIT: u64 = 0x40;
const MAP_FIXED_NOREPLACE: u64 = 0x10_0000;
/// `MAP_32BIT` mappings lie below 2 GiB (as Linux).
const LIMIT_32BIT: u64 = 1 << 31;
/// Steps of the search for a free gap in a low window (Windows programs expect 64 KiB granules).
const GRANULE: u64 = 0x1_0000;

/// Where a new mapping of `len` bytes (page multiple) goes. With `MAP_FIXED` whatever was there is
/// removed; with `MAP_FIXED_NOREPLACE` an occupied range is `EEXIST` (also the kernel's own range).
pub fn place(p: &mut Proc, addr: u64, len: u64, flags: u64) -> Result<u64, i64> {
    if flags & MAP_FIXED != 0 {
        return place_fixed(p, addr, len);
    }
    if flags & MAP_FIXED_NOREPLACE != 0 {
        if addr & 0xfff != 0 {
            return Err(-EINVAL);
        }
        return if usable(p, addr, len) { Ok(addr) } else { Err(-EEXIST) };
    }
    let hint = addr & !0xfff;
    let below = |a: u64| flags & MAP_32BIT == 0 || a + len <= LIMIT_32BIT;
    if hint != 0 && below(hint) && usable(p, hint, len) {
        return Ok(hint);
    }
    if flags & MAP_32BIT != 0 {
        return low_gap(p, len).ok_or(-ENOMEM);
    }
    let v = p.g.mmap_next;
    if v + len + 4096 > proc::STACK_TOP - (1 << 30) {
        return Err(-ENOMEM);
    }
    p.g.mmap_next += len + 4096;
    Ok(v)
}

/// `MAP_FIXED`: whatever the range held (pages, reservations, shared mappings) is removed.
fn place_fixed(p: &mut Proc, addr: u64, len: u64) -> Result<u64, i64> {
    // outside the user areas the page-table index wraps (non-canonical addresses) or names the
    // kernel's identity map: a mapping there would replace the kernel's own pages in this address space
    if addr & 0xfff != 0 || !paging::user_range(addr, len) {
        return Err(-EINVAL);
    }
    if !p.space().unreserve(addr, len) {
        return Err(-ENOMEM);
    }
    p.space().unmap_shared(addr, len);
    p.space().unmap_range(addr, addr + len);
    Ok(addr)
}

/// Whether `[va, va + len)` may hold a new mapping without removing anything: a user area, nothing
/// mapped or reserved, not the room the stack grows into, not the break of the program.
pub fn usable(p: &Proc, va: u64, len: u64) -> bool {
    let end = va.saturating_add(len);
    let stack = (proc::STACK_TOP - proc::STACK_MAX, proc::STACK_TOP);
    let brk = (p.g.brk_start, p.g.brk_limit);
    paging::user_range(va, len) && !overlaps((va, end), stack) && !overlaps((va, end), brk) && p.space().range_free(va, len)
}

fn overlaps(a: (u64, u64), b: (u64, u64)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// First free gap of `len` bytes in a low window (both lie below 2 GiB), in 64 KiB steps.
fn low_gap(p: &Proc, len: u64) -> Option<u64> {
    let windows = [(USER_LOW, USER_LOW_END), (WIN_LOW, WIN_END)];
    for (lo, hi) in windows {
        let mut a = (lo + GRANULE - 1) & !(GRANULE - 1);
        while a + len <= hi {
            if usable(p, a, len) {
                return Some(a);
            }
            a += GRANULE;
        }
    }
    None
}
