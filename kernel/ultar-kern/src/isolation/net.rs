//! Network isolation of cells (stage 3, `docs/integration/zell-netz.md`): network namespaces and the
//! "internet only" barrier.
//!
//! * A **network namespace** (`unshare`/`clone` with `CLONE_NEWNET`) gets its own loopback: its
//!   sockets on `127.0.0.0/8` see only each other (ports and abstract Unix names are its own), nothing
//!   of the host's loopback and nothing from the network card – a listening socket in it is never
//!   reachable from outside. Without the barrier a namespace has no way out at all (`ENETUNREACH`).
//! * The **barrier** ([`PR_ULTAR_NET_GUARD`]) is a per-process mark like `no_new_privs`: inherited by
//!   every child, kept across `execve`, never cleared. A marked process may only reach the internet:
//!   no host loopback, no address of this device, no broadcast or multicast, no private or link-local
//!   network – except the DNS servers the kernel's network configuration names, on port 53. In a
//!   namespace of its own the barrier opens the way out (filtered as above); in the host namespace it
//!   also forbids listening and binding a fixed port, so the process can never offer a service there.
//!
//! The host (namespace 0) without the barrier behaves as before.

use core::sync::atomic::{AtomicU64, Ordering};

/// `clone`/`unshare` flag: new network namespace.
pub const CLONE_NEWNET: u64 = 0x4000_0000;
/// `prctl` option: set the "internet only" barrier on the caller (and so on every later child). There
/// is no way to clear it. Returns 0.
pub const PR_ULTAR_NET_GUARD: u64 = 0x554c_5202;
/// `prctl` option: 1 if the caller carries the barrier, else 0.
pub const PR_ULTAR_NET_GUARD_GET: u64 = 0x554c_5203;
/// `prctl` option: the caller's network namespace number (0 = the host's).
pub const PR_ULTAR_NET_NS_GET: u64 = 0x554c_5204;

/// The host's network namespace.
pub const HOST: u64 = 0;

const EACCES: i64 = 13;
const ENETUNREACH: i64 = 101;
const DNS_PORT: u16 = 53;

static NEXT_NS: AtomicU64 = AtomicU64::new(1);

/// Number of a new, empty network namespace.
pub fn new_namespace() -> u64 {
    NEXT_NS.fetch_add(1, Ordering::Relaxed)
}

/// The network side of a process: its namespace and whether it carries the barrier.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NetView {
    /// Network namespace (0 = host).
    pub ns: u64,
    /// "Internet only" barrier.
    pub guard: bool,
}

impl NetView {
    /// The view of programs the kernel starts.
    pub const HOST: NetView = NetView { ns: HOST, guard: false };

    /// The calling process's view (the host's if there is no process).
    pub fn current() -> NetView {
        crate::syscall::with(|p| p.g.iso.net).unwrap_or(NetView::HOST)
    }

    /// Whether the view is anything but the unrestricted host.
    pub fn restricted(self) -> bool {
        self.ns != HOST || self.guard
    }

    /// May this view send to `ip:port` (connect, a datagram)? `dev` describes this device's addresses.
    pub fn may_reach(self, ip: [u8; 4], port: u16, dev: &Device) -> Result<(), i64> {
        if is_loopback(ip) {
            // the loopback of a namespace is its own; the host's is closed to the barrier
            return if self.ns == HOST && self.guard { Err(-EACCES) } else { Ok(()) };
        }
        if !self.guard {
            return if self.ns == HOST { Ok(()) } else { Err(-ENETUNREACH) };
        }
        if port == DNS_PORT && ip != [0; 4] && dev.dns.contains(&ip) {
            return Ok(());
        }
        if ip == dev.ip || ip == dev.broadcast || is_local_network(ip) {
            return Err(-EACCES);
        }
        Ok(())
    }

    /// May this view bind `ip:port`? Inside a namespace every address is its own; on the host the
    /// barrier allows only the wildcard address with a port the kernel chooses (an outgoing socket).
    pub fn may_bind(self, ip: [u8; 4], port: u16) -> Result<(), i64> {
        if self.ns == HOST && self.guard && (ip != [0; 4] || port != 0) {
            return Err(-EACCES);
        }
        Ok(())
    }

    /// May this view listen? Not on the host with the barrier (no service on the device's addresses).
    pub fn may_listen(self) -> Result<(), i64> {
        if self.ns == HOST && self.guard {
            return Err(-EACCES);
        }
        Ok(())
    }
}

/// This device's addresses as the network configuration has them (all zero before an address).
#[derive(Clone, Copy, Debug)]
pub struct Device {
    /// Own address.
    pub ip: [u8; 4],
    /// Broadcast address of the own network.
    pub broadcast: [u8; 4],
    /// The DNS servers (up to two; zero = none).
    pub dns: [[u8; 4]; 2],
}

/// `127.0.0.0/8`.
pub fn is_loopback(ip: [u8; 4]) -> bool {
    ip[0] == 127
}

/// Addresses the barrier never lets through: unspecified, private, shared (CGNAT), link-local,
/// multicast, reserved and broadcast ranges.
fn is_local_network(ip: [u8; 4]) -> bool {
    let [a, b, ..] = ip;
    a == 0
        || a == 10
        || a == 172 && (16..32).contains(&b)
        || a == 192 && b == 168
        || a == 169 && b == 254
        || a == 100 && (64..128).contains(&b)
        || a >= 224
}
