//! What a program needs to build a cell **around itself** with a private network (the page cell
//! of the browser's engine host, `external/servo-host`): a user namespace when it is not root, the
//! loopback device of a fresh network namespace brought up, and the capabilities given up.
//!
//! Linux only. The Ultar kernel has no network namespace (`unshare(CLONE_NEWNET)` answers
//! `EINVAL`, see `docs/integration/servo-cell.md`), so callers there stop at the namespaces and
//! never get as far as these calls.

use crate::{raw, Errno};
use rustix::net::netlink::SocketAddrNetlink;
use rustix::net::{AddressFamily, RecvFlags, SendFlags, SocketFlags, SocketType};
use rustix::thread::{CapabilitySet, CapabilitySets};

/// `RTM_NEWLINK`.
const RTM_NEWLINK: u16 = 16;
/// `NLMSG_ERROR` (also the acknowledgement: error 0).
const NLMSG_ERROR: u16 = 2;
/// `NLM_F_REQUEST | NLM_F_ACK`.
const REQUEST_ACK: u16 = 1 | 4;
/// `IFF_UP`.
const IFF_UP: u32 = 1;
/// The loopback device of a new network namespace is always interface 1.
const LOOPBACK_INDEX: i32 = 1;

/// The `RTM_NEWLINK` request that sets interface `index` up (header and `ifinfomsg`).
pub fn link_up_request(index: i32) -> Vec<u8> {
    let mut m = Vec::with_capacity(32);
    m.extend_from_slice(&32u32.to_ne_bytes()); // nlmsg_len
    m.extend_from_slice(&RTM_NEWLINK.to_ne_bytes());
    m.extend_from_slice(&REQUEST_ACK.to_ne_bytes());
    m.extend_from_slice(&1u32.to_ne_bytes()); // sequence
    m.extend_from_slice(&0u32.to_ne_bytes()); // port id: the kernel
    m.extend_from_slice(&[0, 0]); // ifi_family AF_UNSPEC, pad
    m.extend_from_slice(&0u16.to_ne_bytes()); // ifi_type
    m.extend_from_slice(&index.to_ne_bytes());
    m.extend_from_slice(&IFF_UP.to_ne_bytes()); // ifi_flags
    m.extend_from_slice(&IFF_UP.to_ne_bytes()); // ifi_change: only this bit
    m
}

/// Reads the kernel's answer to a request with `NLM_F_ACK`: `Ok` for the acknowledgement,
/// the error number otherwise.
pub fn ack_status(answer: &[u8]) -> Result<(), Errno> {
    let word = |at: usize, n: usize| answer.get(at..at + n);
    let kind = word(4, 2).map(|b| u16::from_ne_bytes([b[0], b[1]]));
    if kind != Some(NLMSG_ERROR) {
        return Err(Errno::EINVAL);
    }
    let code = word(16, 4)
        .map(|b| i32::from_ne_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or(Errno::EINVAL)?;
    match code {
        0 => Ok(()),
        c if c < 0 => Err(Errno(c.saturating_neg())),
        _ => Err(Errno::EINVAL),
    }
}

/// Brings up `lo` of the network namespace this thread is in (needs the right to administer
/// that namespace: root, or the owner of its user namespace). Without it the namespace has no
/// `127.0.0.1` at all.
pub fn loopback_up() -> Result<(), Errno> {
    let fd = rustix::net::socket_with(
        AddressFamily::NETLINK,
        SocketType::RAW,
        SocketFlags::CLOEXEC,
        None, // NETLINK_ROUTE is the default protocol of an AF_NETLINK socket
    )?;
    rustix::net::sendto(
        &fd,
        &link_up_request(LOOPBACK_INDEX),
        SendFlags::empty(),
        &SocketAddrNetlink::new(0, 0),
    )?;
    let mut buf = [0u8; 256];
    let (n, _) = rustix::net::recv(&fd, &mut buf[..], RecvFlags::empty())?;
    ack_status(&buf[..n.min(buf.len())])
}

fn write_map(file: &str, text: &str) -> Result<(), Errno> {
    std::fs::write(file, text).map_err(|e| Errno(e.raw_os_error().unwrap_or(Errno::EINVAL.0)))
}

/// Moves this single-threaded process into a user namespace of its own where it keeps its
/// ids (user and group map to themselves), so a process that is not root may then create the
/// other namespaces and mounts. Linux with unprivileged user namespaces only.
pub fn enter_user_namespace() -> Result<(), Errno> {
    let (uid, gid) = (crate::uid(), crate::gid());
    raw::unshare_user()?;
    write_map("/proc/self/setgroups", "deny")?;
    write_map("/proc/self/uid_map", &format!("{uid} {uid} 1"))?;
    write_map("/proc/self/gid_map", &format!("{gid} {gid} 1"))
}

/// Gives up every capability of the calling thread (effective, permitted, inheritable and
/// ambient) and checks that none is left. Call it before other threads are started: the
/// kernel keeps capabilities per thread, and new threads inherit the caller's.
pub fn drop_capabilities() -> Result<(), Errno> {
    // ambient capabilities: absent on old kernels (EINVAL): nothing to clear there
    match rustix::thread::clear_ambient_capability_set() {
        Ok(()) => {}
        Err(rustix::io::Errno::INVAL) => {}
        Err(errno) => return Err(errno.into()),
    }
    let none = CapabilitySets {
        effective: CapabilitySet::empty(),
        permitted: CapabilitySet::empty(),
        inheritable: CapabilitySet::empty(),
    };
    rustix::thread::set_capabilities(None, none)?;
    let left = rustix::thread::capabilities(None)?;
    if left.effective.is_empty() && left.permitted.is_empty() && left.inheritable.is_empty() {
        Ok(())
    } else {
        Err(Errno::EPERM)
    }
}

/// The text after `name` (for example `CapEff:`) in this process's status file; `None` if the
/// line is missing.
pub fn own_status_field(name: &str) -> Option<String> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    status
        .lines()
        .find_map(|l| l.strip_prefix(name))
        .map(|v| v.trim().to_string())
}

/// The names of the network devices this process's namespace has (`lo`, `eth0`, …).
pub fn network_devices() -> Vec<String> {
    let text = std::fs::read_to_string("/proc/net/dev").unwrap_or_default();
    text.lines()
        .skip(2)
        .filter_map(|l| l.split(':').next())
        .map(|n| n.trim().to_string())
        .collect()
}

/// The group (cgroup v2) this process is in, as a path below the group root (`/user.slice/…`,
/// `/` for the root group).
pub fn own_group() -> String {
    let text = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    text.lines()
        .find_map(|l| l.strip_prefix("0::"))
        .unwrap_or("/")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_status_the_devices_and_the_group_of_this_process_are_readable() {
        assert!(own_status_field("CapEff:").is_some_and(|v| !v.is_empty()));
        assert_eq!(own_status_field("NoSuchField:"), None);
        assert!(network_devices().iter().any(|d| d == "lo"));
        assert!(own_group().starts_with('/'));
    }

    #[test]
    fn the_link_request_is_a_32_byte_newlink_with_the_up_bit() {
        let m = link_up_request(1);
        assert_eq!(m.len(), 32);
        assert_eq!(u32::from_ne_bytes([m[0], m[1], m[2], m[3]]), 32);
        assert_eq!(u16::from_ne_bytes([m[4], m[5]]), 16);
        assert_eq!(u16::from_ne_bytes([m[6], m[7]]), 5);
        assert_eq!(i32::from_ne_bytes([m[20], m[21], m[22], m[23]]), 1);
        assert_eq!(&m[24..32], &[1, 0, 0, 0, 1, 0, 0, 0]);
    }

    #[test]
    fn the_answer_is_read_as_ok_or_errno() {
        let mut answer = vec![0u8; 36];
        answer[4..6].copy_from_slice(&2u16.to_ne_bytes());
        assert_eq!(ack_status(&answer), Ok(()));
        answer[16..20].copy_from_slice(&(-1i32).to_ne_bytes());
        assert_eq!(ack_status(&answer), Err(Errno::EPERM));
        assert_eq!(ack_status(&[0; 4]), Err(Errno::EINVAL));
        answer[4..6].copy_from_slice(&3u16.to_ne_bytes());
        assert_eq!(ack_status(&answer), Err(Errno::EINVAL));
    }
}
