//! Unix sockets and the file system: the node a bound socket has in the file tree, who may connect,
//! and who the peer is (`SO_PEERCRED`).
//!
//! * `bind` to a path creates a node of type socket there (mode `0777 & ~umask`, owned by the
//!   binder); a path that exists is `EADDRINUSE`. The node stays when the socket closes – as on
//!   Linux a server removes a stale node with `unlink` before it binds again.
//! * `connect` needs the node to exist (`ENOENT`), to be a socket with a listener
//!   (`ECONNREFUSED`), and write permission on it (`EACCES`, see `perm.rs`).
//! * Each end of a connection knows the process id, effective user id and group id its peer had
//!   when the connection was made: the connector's at `connect`, the listener's at `bind`.
//! * Abstract names (leading NUL) have no node and no permission check; they are resolved in the
//!   caller's file tree view as before, so a cell cannot reach the abstract sockets of its host.

use crate::fs;
use crate::isolation::path::{parent, Loc};
use crate::perm;
use crate::pipe::Pipe;
use crate::proc::{FdKind, UnixState};
use crate::syscall::with;
use alloc::sync::Arc;
use alloc::vec::Vec;

const EPERM: i64 = 1;
const ENOENT: i64 = 2;
const EFAULT: i64 = 14;
const EINVAL: i64 = 22;
const ENOSPC: i64 = 28;
const EROFS: i64 = 30;
const ENOTSOCK: i64 = 88;
const ENOPROTOOPT: i64 = 92;
const EADDRINUSE: i64 = 98;
const ECONNREFUSED: i64 = 111;
const SOL_SOCKET: u64 = 1;
/// Size of `struct ucred`.
const UCRED: usize = 12;

/// Identity of the process at one end of a connection.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Peer {
    /// Kernel-wide number of the thread group.
    pub pid: u64,
    /// Effective user id.
    pub uid: u32,
    /// Effective group id.
    pub gid: u32,
}

/// Identity of the calling process.
pub fn me() -> Peer {
    with(|p| Peer { pid: p.tgid, uid: p.g.iso.cred.euid(), gid: p.g.iso.cred.egid() }).unwrap_or(Peer { pid: 0, uid: 0, gid: 0 })
}

/// Mark both directions of a fresh connection: `ours` is what the caller writes to, `theirs` what
/// the other end (identity `other`) writes to.
pub fn mark(ours: &Pipe, theirs: &Pipe, other: Peer) {
    ours.set_writer(me());
    theirs.set_writer(other);
}

/// `bind` to a path: create the socket node. Nothing to do for an abstract name.
pub fn bind_node(loc: &Loc, abstract_name: bool) -> Result<(), i64> {
    if abstract_name {
        return Ok(());
    }
    if perm::of(&loc.host).is_some() {
        return Err(-EADDRINUSE);
    }
    if loc.read_only {
        return Err(-EROFS);
    }
    // a cell must not add entries to the kernel's own trees (they are shared by every view)
    if loc.confined && ["/proc", "/dev"].iter().any(|d| crate::isolation::mount::below(&loc.host, d).is_some()) {
        return Err(-EPERM);
    }
    if !matches!(fs::stat(&parent(&loc.host)), Some((1, _))) {
        return Err(-ENOENT);
    }
    perm::may_create(&loc.host)?;
    // the node is an empty entry of the RAM layer (also below a directory on disk: it is not
    // meant to outlive a restart), subject to the quota of its tree
    if !crate::isolation::tmpfs::store(&loc.host, Arc::new(Vec::new())) {
        return Err(-ENOSPC);
    }
    perm::created(&loc.host, perm::S_IFSOCK, 0o777);
    Ok(())
}

/// `connect` to a path: the node must be a socket the caller may write to.
pub fn connect_node(loc: &Loc, abstract_name: bool) -> Result<(), i64> {
    if abstract_name {
        return Ok(());
    }
    match perm::of(&loc.host) {
        None => Err(-ENOENT),
        Some(a) if a.mode & perm::S_IFMT != perm::S_IFSOCK => Err(-ECONNREFUSED),
        Some(_) => perm::check(&loc.host, perm::W),
    }
}

/// `SO_RCVTIMEO`.
const SO_RCVTIMEO: u64 = 20;

/// The direction a connected Unix socket descriptor reads from.
fn read_pipe(fd: u64) -> Option<Arc<Pipe>> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::UnixConn(rx, _)) => Some(rx.clone()),
        _ => None,
    })
    .flatten()
}

/// `setsockopt` on a connected Unix socket. `SO_RCVTIMEO` (a `timeval`, rounded up to whole
/// milliseconds; 0 = wait without limit): a blocking read that waited that long ends with `EAGAIN`,
/// as on Linux – the bus client relies on it while it greets a broker that may be going away.
/// `None`: not handled here (the call keeps its old answer, success without effect).
pub fn setsockopt(fd: u64, level: u64, opt: u64, val: u64, len: u64) -> Option<i64> {
    if level != SOL_SOCKET || opt != SO_RCVTIMEO {
        return None;
    }
    let rx = read_pipe(fd)?;
    if len < 16 {
        return Some(-EINVAL);
    }
    let Some(tv) = crate::syscall::get(val, 16) else { return Some(-EFAULT) };
    let sec = i64::from_le_bytes(tv[0..8].try_into().unwrap_or([0; 8]));
    let usec = i64::from_le_bytes(tv[8..16].try_into().unwrap_or([0; 8]));
    if sec < 0 || !(0..1_000_000).contains(&usec) {
        return Some(-33); // EDOM, as Linux
    }
    let ms = (sec as u64).saturating_mul(1000).saturating_add((usec as u64).div_ceil(1000));
    rx.read_timeout_ms.store(ms, core::sync::atomic::Ordering::Relaxed);
    Some(0)
}

/// What `getsockopt` can tell about a Unix socket descriptor.
enum Kind {
    Connected(Option<Peer>),
    Listening,
    Unbound,
}

fn kind_of(fd: u64) -> Option<Kind> {
    with(|p| match p.fd(fd).map(|d| &d.kind) {
        Some(FdKind::UnixConn(rx, _)) => Some(Kind::Connected(rx.writer())),
        Some(FdKind::UnixSock(s)) => Some(if matches!(&*s.lock(), UnixState::Listening(..)) { Kind::Listening } else { Kind::Unbound }),
        _ => None,
    })
    .flatten()
}

/// `struct ucred` of `peer` as the caller sees it: the pid in the caller's pid namespace (0 if the
/// peer is not visible there); "nobody" (`-1`) for a socket that is not connected.
fn ucred(peer: Option<Peer>) -> [u8; UCRED] {
    let (pid, uid, gid) = match peer {
        Some(p) => (with(|me| me.link.ns().local(p.pid)).flatten().unwrap_or(0) as u32, p.uid, p.gid),
        None => (0, u32::MAX, u32::MAX),
    };
    let mut out = [0u8; UCRED];
    out[0..4].copy_from_slice(&pid.to_le_bytes());
    out[4..8].copy_from_slice(&uid.to_le_bytes());
    out[8..12].copy_from_slice(&gid.to_le_bytes());
    out
}

fn user_len(lenp: u64) -> Option<usize> {
    let b = with(|p| crate::proc::read_user(p.space(), lenp, 4)).flatten()?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

fn reply(val: u64, lenp: u64, data: &[u8]) -> i64 {
    let Some(room) = user_len(lenp) else { return -EFAULT };
    if room < data.len() {
        return -EINVAL;
    }
    let ok = with(|p| crate::proc::write_user(p.space(), val, data) && crate::proc::write_user(p.space(), lenp, &(data.len() as u32).to_le_bytes())).unwrap_or(false);
    if ok {
        0
    } else {
        -EFAULT
    }
}

/// `getsockopt` on a Unix socket: `SO_PEERCRED`, and the few integer options programs ask for.
pub fn getsockopt(fd: u64, level: u64, opt: u64, val: u64, lenp: u64) -> i64 {
    let Some(kind) = kind_of(fd) else { return -ENOTSOCK };
    if level != SOL_SOCKET {
        return -ENOPROTOOPT;
    }
    if opt == SO_RCVTIMEO {
        let ms = read_pipe(fd).map_or(0, |r| r.read_timeout_ms.load(core::sync::atomic::Ordering::Relaxed));
        let mut tv = [0u8; 16];
        tv[0..8].copy_from_slice(&(ms / 1000).to_le_bytes());
        tv[8..16].copy_from_slice(&((ms % 1000) * 1000).to_le_bytes());
        return reply(val, lenp, &tv);
    }
    let number: i32 = match (opt, &kind) {
        // SO_PEERCRED
        (17, Kind::Connected(peer)) => return reply(val, lenp, &ucred(*peer)),
        (17, _) => return reply(val, lenp, &ucred(None)),
        // SO_TYPE (stream), SO_ERROR, SO_DOMAIN (AF_UNIX), SO_PASSCRED (never on)
        (3, _) | (39, _) => 1,
        (4, _) | (16, _) => 0,
        // SO_ACCEPTCONN
        (30, Kind::Listening) => 1,
        (30, _) => 0,
        // SO_SNDBUF, SO_RCVBUF: the bounded buffer of one direction
        (7, _) | (8, _) => crate::pipe::UNIX_DEFAULT as i32,
        _ => return -ENOPROTOOPT,
    };
    reply(val, lenp, &number.to_le_bytes())
}
