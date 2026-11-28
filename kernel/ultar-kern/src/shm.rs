//! Shared memory for windowing programs: `mmap` of memory objects (`memfd_create`) and of the
//! framebuffer, `mremap`, and the simulated vertical blank of `/dev/fb0`.
//!
//! A shared mapping maps the object's own pages; every process that maps it sees the same bytes, also
//! after `fork`. A private mapping (`MAP_PRIVATE`) gets a copy of the bytes at the time of the call.

use crate::memfd::{Backing, MemObj, SEAL_FUTURE_WRITE, SEAL_WRITE};
use crate::proc::{FdKind, Proc};
use alloc::sync::Arc;

const EPERM: i64 = 1;
const ENOMEM: i64 = 12;
const EACCES: i64 = 13;
const EINVAL: i64 = 22;
const ENODEV: i64 = 19;

const PROT_WRITE: u64 = 2;
const PROT_EXEC: u64 = 4;
const MAP_SHARED: u64 = 1;
const MAP_TYPE: u64 = 0xf;
const MREMAP_MAYMOVE: u64 = 1;
const MREMAP_FIXED: u64 = 2;
const USER_TOP: u64 = 0x8000_0000_0000;

/// `FBIO_WAITFORVSYNC`
pub const FBIO_WAITFORVSYNC: u64 = 0x4004_4620;
/// Refresh rate of the simulated vertical blank (the firmware framebuffer has no interrupt).
pub const VBLANK_HZ: u64 = 60;

/// What a descriptor maps.
enum Source {
    Obj(Arc<MemObj>),
    /// The framebuffer; `true` when opened for writing.
    Fb(bool),
    /// The GPU render node: `off` names one of the opener's buffers (svcdev/dri.rs).
    Gpu(Arc<crate::svcdev::dri::DriFd>),
}

/// `mmap` of a memory object or the framebuffer; `None`: the descriptor is neither (or not open).
pub fn mmap(addr: u64, len: u64, prot: u64, flags: u64, fd: i64, off: u64) -> Option<i64> {
    let src = crate::syscall::with(|p| match p.fd(fd as u64).map(|d| (&d.kind, d.flags & 3)) {
        Some((FdKind::Mem(m, _), _)) => Some(Source::Obj(m.clone())),
        Some((FdKind::Fb(_), mode)) => Some(Source::Fb(mode != 0)),
        Some((FdKind::Dri(d), _)) => Some(Source::Gpu(d.clone())),
        _ => None,
    })
    .flatten()?;
    if off & 0xfff != 0 {
        return Some(-EINVAL);
    }
    let shared = flags & MAP_TYPE == MAP_SHARED || flags & MAP_TYPE == 3;
    let write = prot & PROT_WRITE != 0;
    let what = match src {
        Source::Obj(m) => {
            if shared && write && m.seals() & (SEAL_WRITE | SEAL_FUTURE_WRITE) != 0 {
                return Some(-EPERM);
            }
            if !shared {
                return Some(copy(addr, len, prot, flags, &m, off));
            }
            Backing::of(m, write, off)
        }
        Source::Gpu(d) => match crate::svcdev::dri::mmap_object(&d, off, len) {
            Some(m) if shared => Backing::of(m, write, 0),
            _ => return Some(-EINVAL),
        },
        Source::Fb(_) if !shared => return Some(-ENODEV),
        Source::Fb(false) if write => return Some(-EACCES),
        Source::Fb(_) => Backing::Framebuffer(off),
    };
    Some(map(addr, len, flags, what))
}

/// Map `what` (shared) at a new or fixed address.
pub(crate) fn map(addr: u64, len: u64, flags: u64, what: Backing) -> i64 {
    let Some(frames) = what.frames(len) else { return -ENOMEM };
    let w = what.writable();
    crate::syscall::with(|p| {
        let va = match crate::syscall::place(p, addr, len, flags) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if !p.space().map_shared(va, &frames, w, what) {
            return -ENOMEM;
        }
        va as i64
    })
    .unwrap_or(-ENOMEM)
}

/// `MAP_PRIVATE` of a memory object: own pages holding a copy of its bytes.
fn copy(addr: u64, len: u64, prot: u64, flags: u64, m: &MemObj, off: u64) -> i64 {
    let bytes = m.read(off, len.min(m.size().saturating_sub(off)) as usize);
    crate::syscall::with(|p| {
        let va = match crate::syscall::place(p, addr, len, flags) {
            Ok(v) => v,
            Err(e) => return e,
        };
        if !p.space().reserve(va, len, true, prot & PROT_EXEC != 0) {
            return -ENOMEM;
        }
        if !bytes.is_empty() && !crate::proc::write_user(p.space(), va, &bytes) {
            return -ENOMEM;
        }
        va as i64
    })
    .unwrap_or(-ENOMEM)
}

/// `mremap(old, old_len, new_len, flags, new_addr)`.
pub fn mremap(old: u64, old_len: u64, new_len: u64, flags: u64, new_addr: u64) -> i64 {
    let (old_len, new_len) = ((old_len + 4095) & !4095, (new_len + 4095) & !4095);
    if old & 0xfff != 0 || flags & !(MREMAP_MAYMOVE | MREMAP_FIXED) != 0 || new_len == 0 || new_len > 1 << 34 || old_len > 1 << 34 {
        return -EINVAL;
    }
    if old < crate::paging::USER_LOW || old + old_len > USER_TOP || flags & MREMAP_FIXED != 0 && flags & MREMAP_MAYMOVE == 0 {
        return -EINVAL;
    }
    crate::syscall::with(|p| {
        if p.space().has_shared(old, old_len.max(4096)) {
            return remap_shared(p, old, old_len, new_len, flags, new_addr);
        }
        remap_own(p, old, old_len, new_len, flags, new_addr)
    })
    .unwrap_or(-EINVAL)
}

/// Where a mapping that grows goes when it cannot grow in place.
fn target(p: &mut Proc, old: u64, new_len: u64, flags: u64, new_addr: u64) -> Result<u64, i64> {
    if flags & MREMAP_FIXED != 0 {
        if new_addr < old + new_len && new_addr + new_len > old {
            return Err(-EINVAL);
        }
        return crate::syscall::place(p, new_addr, new_len, 0x10);
    }
    if flags & MREMAP_MAYMOVE == 0 {
        return Err(-ENOMEM);
    }
    crate::syscall::place(p, 0, new_len, 0)
}

/// Resize a shared mapping: it is mapped anew from the same object and offset.
fn remap_shared(p: &mut Proc, old: u64, old_len: u64, new_len: u64, flags: u64, new_addr: u64) -> i64 {
    let va = if new_len <= old_len && flags & MREMAP_FIXED == 0 {
        old
    } else {
        match target(p, old, new_len, flags, new_addr) {
            Ok(v) => v,
            Err(e) => return e,
        }
    };
    let Some(what) = p.space().take_shared(old, old_len) else { return -EINVAL };
    let Some(frames) = what.frames(new_len) else { return -ENOMEM };
    let w = what.writable();
    if !p.space().map_shared(va, &frames, w, what) {
        return -ENOMEM;
    }
    va as i64
}

/// Resize a mapping of own pages: shrink in place, grow by moving the pages (no copy) to a new place
/// whose tail is reserved like the old range.
fn remap_own(p: &mut Proc, old: u64, old_len: u64, new_len: u64, flags: u64, new_addr: u64) -> i64 {
    let s = p.space();
    if new_len <= old_len && flags & MREMAP_FIXED == 0 {
        if !s.unreserve(old + new_len, old_len - new_len) {
            return -ENOMEM;
        }
        let mut a = old + new_len;
        while a < old + old_len {
            s.unmap(a);
            a += 4096;
        }
        return old as i64;
    }
    let rights = p.space().reserved_at(old).unwrap_or((true, false));
    // the space behind is free: the mapping grows where it is (glibc's realloc of large blocks)
    if flags & MREMAP_FIXED == 0 && crate::vmplace::usable(p, old + old_len, new_len - old_len) {
        return if p.space().reserve(old + old_len, new_len - old_len, rights.0, rights.1) { old as i64 } else { -ENOMEM };
    }
    let va = match target(p, old, new_len, flags, new_addr) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let s = p.space();
    if !s.reserve(va, new_len, rights.0, rights.1) {
        return -ENOMEM;
    }
    let keep = old_len.min(new_len);
    let mut i = 0;
    while i < keep {
        if !s.move_page(old + i, va + i) {
            return -ENOMEM;
        }
        i += 4096;
    }
    // what is left of the old range (and its reservation) goes away
    let _ = s.unreserve(old, old_len);
    let mut a = old;
    while a < old + old_len {
        s.unmap(a);
        a += 4096;
    }
    va as i64
}

/// `FBIO_WAITFORVSYNC` on a framebuffer descriptor: the time of the next vertical blank to sleep until.
pub fn vsync_wait(fd: u64, req: u64) -> Option<u64> {
    if req != FBIO_WAITFORVSYNC {
        return None;
    }
    let fb = crate::syscall::with(|p| matches!(p.fd(fd).map(|d| &d.kind), Some(FdKind::Fb(_)))).unwrap_or(false);
    fb.then(next_vblank)
}

/// Uptime in ms of the next simulated vertical blank (a 60 Hz grid).
pub fn next_vblank() -> u64 {
    let now = crate::arch::uptime_ms();
    let n = now * VBLANK_HZ / 1000 + 1;
    (n * 1000).div_ceil(VBLANK_HZ)
}

/// Number of vertical blanks since boot (`FBIOGET_VBLANK`).
pub fn vblank_count() -> u64 {
    crate::arch::uptime_ms() * VBLANK_HZ / 1000
}
