//! Calls programs of other systems need (dpkg, apt, perl – docs/integration/ultar-foreign.md):
//! `close_range` (436), `sync_file_range` (277), `msync` (26) and `fchdir` (81).

use crate::isolation::path::View;
use crate::proc::FdKind;
use crate::syscall::{sys_close, with};

const EBADF: i64 = 9;
const EACCES: i64 = 13;
const ENOTDIR: i64 = 20;
const EINVAL: i64 = 22;
const ESPIPE: i64 = 29;
const EXDEV: i64 = 18;

/// `close_range` flag: only mark the descriptors close-on-exec.
const CLOSE_RANGE_CLOEXEC: u64 = 1 << 2;
/// `close_range` flag: unshare the table first – each process already has its own (clone never shares it).
const CLOSE_RANGE_UNSHARE: u64 = 1 << 1;

/// `close_range(first, last, flags)`: close (or mark close-on-exec) every open descriptor in
/// `first..=last`; `last` may be `~0` ("all above").
pub fn close_range(first: u64, last: u64, flags: u64) -> i64 {
    let (first, last) = (first as u32 as usize, last as u32 as usize);
    if flags & !(CLOSE_RANGE_CLOEXEC | CLOSE_RANGE_UNSHARE) != 0 || first > last {
        return -EINVAL;
    }
    let Some(open) = with(|p| p.g.fds.len()) else { return -EBADF };
    let end = last.min(open.saturating_sub(1));
    if first > end {
        return 0;
    }
    if flags & CLOSE_RANGE_CLOEXEC != 0 {
        with(|p| p.g.fds[first..=end].iter_mut().flatten().for_each(|d| d.cloexec = true));
        return 0;
    }
    for fd in first..=end {
        // a write-back error of one file does not stop the rest (Linux ignores them here)
        let _ = sys_close(fd as u64);
    }
    0
}

/// `sync_file_range(fd, offset, nbytes, flags)`: a file's dirty data is stored at its `fsync`/`close`
/// (whole-file writes, fs.rs), so starting a write-back early has nothing to do; the descriptor and the
/// flags are checked as Linux does.
pub fn sync_file_range(fd: u64, flags: u64) -> i64 {
    if flags & !7 != 0 {
        return -EINVAL;
    }
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::File { .. }) | Some(FdKind::Mem(..)) => 0,
        Some(FdKind::Dir { .. }) => -EINVAL,
        Some(_) => -ESPIPE,
        None => -EBADF,
    })
    .unwrap_or(-EBADF)
}

/// `msync(addr, len, flags)`: mappings of files are private copies on this kernel (sys_mmap) and shared
/// memory objects are the same pages for everyone, so there is nothing to write back. Checked like Linux:
/// page-aligned start, exactly one of `MS_ASYNC`/`MS_SYNC`, no unknown flag.
pub fn msync(addr: u64, flags: u64) -> i64 {
    const MS_ASYNC: u64 = 1;
    const MS_INVALIDATE: u64 = 2;
    const MS_SYNC: u64 = 4;
    let both = flags & MS_ASYNC != 0 && flags & MS_SYNC != 0;
    if addr & 0xfff != 0 || flags & !(MS_ASYNC | MS_INVALIDATE | MS_SYNC) != 0 || both {
        return -EINVAL;
    }
    0
}

/// `fchdir(fd)`: the working directory becomes the directory the descriptor was opened as – only if it
/// was opened in the caller's current view (like a relative lookup through it, syscall::locate) and the
/// caller may search it.
pub fn fchdir(fd: u64) -> i64 {
    let target = with(|p| {
        let tag = View::of(&p.g.iso).tag();
        match p.fd(fd).map(|d| &d.kind) {
            Some(FdKind::Dir { view, path, tag: t, .. }) if *t == tag => Ok((view.clone(), path.clone())),
            Some(FdKind::Dir { .. }) => Err(-EXDEV),
            Some(_) => Err(-ENOTDIR),
            None => Err(-EBADF),
        }
    })
    .unwrap_or(Err(-EBADF));
    let (view, host) = match target {
        Ok(t) => t,
        Err(e) => return e,
    };
    if crate::perm::check(&host, crate::perm::X).is_err() {
        return -EACCES;
    }
    with(|p| p.g.cwd = view);
    0
}
