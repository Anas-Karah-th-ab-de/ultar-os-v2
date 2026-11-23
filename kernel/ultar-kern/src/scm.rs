//! Messages on connected Unix sockets with ancillary data: `SCM_RIGHTS` (descriptors travel with the
//! bytes, as on Linux), `MSG_DONTWAIT` and `MSG_NOSIGNAL`.
//!
//! The descriptors of one `sendmsg` travel with its first byte. A `recvmsg` that reads that byte gets
//! them as new descriptors; it never reads past the next byte that carries descriptors, so the
//! descriptors of two messages never arrive together. A plain `read` discards them.

use crate::pipe::PushError;
use crate::proc::{Fd, FdKind, Proc};
use alloc::vec::Vec;

const EBADF: i64 = 9;
const EAGAIN: i64 = 11;
const ENOMEM: i64 = 12;
const EFAULT: i64 = 14;
const EINVAL: i64 = 22;
const EPIPE: i64 = 32;
const EOPNOTSUPP: i64 = 95;

const SOL_SOCKET: u32 = 1;
const SCM_RIGHTS: u32 = 1;
/// Most descriptors one message carries (Linux: `SCM_MAX_FD`).
pub const MAX_FDS: usize = 253;
pub const MSG_CTRUNC: u32 = 8;
pub const MSG_DONTWAIT: u64 = 0x40;
pub const MSG_NOSIGNAL: u64 = 0x4000;
pub const MSG_CMSG_CLOEXEC: u64 = 0x4000_0000;
/// Bytes of one message, at most (as `sendmsg` on other sockets).
const MAX_BYTES: usize = 1 << 20;

/// Outcome of a call: a result, or wait for the event counter to move past the value.
pub enum Step {
    Val(i64),
    Wait(u64),
}

/// Whether `fd` is a connected Unix socket (the calls here are for those only).
pub fn is_conn(fd: u64) -> bool {
    crate::syscall::with(|p| matches!(p.fd(fd).map(|d| &d.kind), Some(FdKind::UnixConn(..)))).unwrap_or(false)
}

/// The descriptor numbers in a control buffer (`SCM_RIGHTS` messages; others are ignored).
fn rights_in(ctl: &[u8]) -> Result<Vec<i32>, i64> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 16 <= ctl.len() {
        let len = u64::from_le_bytes(ctl[at..at + 8].try_into().unwrap_or([0; 8])) as usize;
        let level = u32::from_le_bytes(ctl[at + 8..at + 12].try_into().unwrap_or([0; 4]));
        let kind = u32::from_le_bytes(ctl[at + 12..at + 16].try_into().unwrap_or([0; 4]));
        if len < 16 || at + len > ctl.len() {
            return Err(-EINVAL);
        }
        if level == SOL_SOCKET && kind == SCM_RIGHTS {
            for c in ctl[at + 16..at + len].chunks_exact(4) {
                out.push(i32::from_le_bytes([c[0], c[1], c[2], c[3]]));
            }
        }
        at += len.div_ceil(8) * 8;
    }
    if out.len() > MAX_FDS {
        return Err(-EINVAL);
    }
    Ok(out)
}

/// Copies of the caller's descriptors `nums` (they stay open in the caller). Unix sockets cannot be
/// passed (`EOPNOTSUPP`): a socket in flight inside itself would never be freed (no collector).
fn copies(p: &mut Proc, nums: &[i32]) -> Result<Vec<Fd>, i64> {
    let mut out = Vec::new();
    if !crate::mem::try_reserve(&mut out, nums.len()) {
        return Err(-ENOMEM);
    }
    for &n in nums {
        let Some(d) = (n >= 0).then(|| p.fd(n as u64)).flatten() else { return Err(-EBADF) };
        if matches!(d.kind, FdKind::UnixConn(..) | FdKind::UnixSock(_)) {
            return Err(-EOPNOTSUPP);
        }
        out.push(d.dup());
    }
    Ok(out)
}

/// `sendmsg`/`sendto` on a connected Unix socket: `data` and the control buffer `ctl`.
pub fn send(fd: u64, data: &[u8], ctl: &[u8], flags: u64) -> Step {
    let nums = match rights_in(ctl) {
        Ok(n) => n,
        Err(e) => return Step::Val(e),
    };
    let seen = crate::socket::gen();
    crate::syscall::with(|p| {
        let fds = match copies(p, &nums) {
            Ok(f) => f,
            Err(e) => return Step::Val(e),
        };
        let Some(d) = p.fd(fd) else { return Step::Val(-EBADF) };
        let nonblock = d.flags & 0x800 != 0 || flags & MSG_DONTWAIT != 0;
        let FdKind::UnixConn(_, tx) = &d.kind else { return Step::Val(-EBADF) };
        let tx = tx.clone();
        if tx.read_closed() {
            if flags & MSG_NOSIGNAL == 0 {
                p.pending |= 1 << (crate::signal::SIGPIPE - 1);
            }
            return Step::Val(-EPIPE);
        }
        let r = if fds.is_empty() { tx.push(data).map_err(|e| (e, fds)) } else { tx.push_with_rights(data, fds) };
        match r {
            Ok(n) => {
                crate::socket::bump();
                Step::Val(n as i64)
            }
            Err((PushError::Full, _)) if nonblock => Step::Val(-EAGAIN),
            Err((PushError::Full, _)) => Step::Wait(seen),
            Err((PushError::NoMemory, _)) => Step::Val(-ENOMEM),
        }
    })
    .unwrap_or(Step::Val(-EBADF))
}

/// What `recvmsg` reports besides the bytes.
pub struct Received {
    pub len: i64,
    /// Bytes of the control buffer used.
    pub ctl_len: u64,
    pub msg_flags: u32,
}

/// `recvmsg`/`recvfrom` on a connected Unix socket: fill the buffers `iov`, put descriptors into the
/// control buffer at `ctl` (room `ctl_room`).
pub fn recv(fd: u64, iov: &[(u64, u64)], ctl: u64, ctl_room: u64, flags: u64) -> Result<Received, Step> {
    let total = iov.iter().map(|x| x.1).fold(0u64, |a, b| a.saturating_add(b)).min(MAX_BYTES as u64) as usize;
    let seen = crate::socket::gen();
    crate::syscall::with(|p| {
        let Some(d) = p.fd(fd) else { return Err(Step::Val(-EBADF)) };
        let nonblock = d.flags & 0x800 != 0 || flags & MSG_DONTWAIT != 0;
        let FdKind::UnixConn(rx, _) = &d.kind else { return Err(Step::Val(-EBADF)) };
        let rx = rx.clone();
        let mut b = rx.buf.lock();
        if total == 0 || b.is_empty() && rx.write_closed() {
            return Ok(Received { len: 0, ctl_len: 0, msg_flags: 0 });
        }
        if b.is_empty() {
            return Err(if nonblock { Step::Val(-EAGAIN) } else { Step::Wait(seen) });
        }
        let Some((data, fds)) = rx.take_with_rights(&mut b, total) else { return Err(Step::Val(-ENOMEM)) };
        drop(b);
        crate::socket::bump();
        if !scatter(p, iov, &data) {
            return Err(Step::Val(-EFAULT));
        }
        let (ctl_len, msg_flags) = deliver(p, fds, ctl, ctl_room, flags & MSG_CMSG_CLOEXEC != 0);
        Ok(Received { len: data.len() as i64, ctl_len, msg_flags })
    })
    .unwrap_or(Err(Step::Val(-EBADF)))
}

/// Copy `data` into the caller's buffers in order.
fn scatter(p: &mut Proc, iov: &[(u64, u64)], data: &[u8]) -> bool {
    let mut off = 0usize;
    for &(b, l) in iov {
        if off >= data.len() {
            break;
        }
        let k = (l as usize).min(data.len() - off);
        if !crate::proc::write_user(p.space(), b, &data[off..off + k]) {
            return false;
        }
        off += k;
    }
    true
}

/// Install the received descriptors and write one `SCM_RIGHTS` message for those that fit into the
/// control buffer; the rest are closed (`MSG_CTRUNC`). Returns (bytes used, message flags).
fn deliver(p: &mut Proc, fds: Vec<Fd>, ctl: u64, room: u64, cloexec: bool) -> (u64, u32) {
    if fds.is_empty() {
        return (0, 0);
    }
    let fit = (room.saturating_sub(16) / 4) as usize;
    let wanted = fds.len();
    let mut nums = Vec::new();
    for mut f in fds.into_iter().take(fit) {
        f.cloexec = cloexec;
        let n = p.add_fd(f, 0);
        if n < 0 {
            break;
        }
        nums.push(n as i32);
    }
    if nums.is_empty() {
        return (0, MSG_CTRUNC);
    }
    let len = 16 + 4 * nums.len() as u64;
    let mut msg = Vec::new();
    msg.extend_from_slice(&len.to_le_bytes());
    msg.extend_from_slice(&SOL_SOCKET.to_le_bytes());
    msg.extend_from_slice(&SCM_RIGHTS.to_le_bytes());
    nums.iter().for_each(|n| msg.extend_from_slice(&n.to_le_bytes()));
    if !crate::proc::write_user(p.space(), ctl, &msg) {
        return (0, MSG_CTRUNC);
    }
    let flags = if nums.len() < wanted { MSG_CTRUNC } else { 0 };
    ((len.div_ceil(8) * 8).min(room), flags)
}
