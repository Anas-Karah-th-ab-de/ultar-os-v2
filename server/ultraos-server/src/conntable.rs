//! Which connections the process keeps open, and which one gives way when it is full.
//!
//! Every connection holds a slot. Two bounds apply: `max_total` slots in all, and
//! `max_per_peer` per client (IPv4 address or IPv6 /64; trusted reverse proxies are exempt –
//! behind a proxy every connection comes from it). When a new connection would exceed a bound,
//! the **oldest idle** connection within that bound is closed to make room: idle means no request
//! in flight and no response body being sent (silent connections that never sent a request, and
//! keep-alive connections between requests). Only when every connection within the bound is busy
//! is the new connection closed. So one client cannot hold the process with silent connections:
//! they are the first to go.

use crate::guard::ProxyNet;
use crate::ratelimit::ClientKey;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::Notify;

struct Conn {
    peer: Option<ClientKey>,
    busy: Arc<AtomicUsize>,
    evict: Arc<Notify>,
}

#[derive(Default)]
struct Inner {
    next_id: u64,
    conns: HashMap<u64, Conn>,
}

/// The table of open connections.
pub struct ConnTable {
    inner: Mutex<Inner>,
    max_total: usize,
    max_per_peer: usize,
    trusted: Vec<ProxyNet>,
}

/// The place of one connection. Dropping it frees the place.
pub struct Slot {
    id: u64,
    table: Arc<ConnTable>,
    /// Requests in flight (with their response bodies) on this connection.
    pub busy: Arc<AtomicUsize>,
    /// Signalled when the connection has to give way.
    pub evict: Arc<Notify>,
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.table.lock().conns.remove(&self.id);
    }
}

/// Counts a request in flight for as long as it lives.
pub struct BusyGuard(Arc<AtomicUsize>);

impl BusyGuard {
    /// Marks the connection busy until the guard is dropped.
    pub fn new(busy: Arc<AtomicUsize>) -> Self {
        busy.fetch_add(1, Ordering::SeqCst);
        Self(busy)
    }
}

impl Drop for BusyGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl ConnTable {
    /// A table with the given bounds; peers in `trusted` are exempt from the per-peer bound.
    pub fn new(max_total: usize, max_per_peer: usize, trusted: Vec<ProxyNet>) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            max_total: max_total.max(1),
            max_per_peer: max_per_peer.max(1),
            trusted,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Closes the oldest idle connection that `within` selects. False when there is none.
    fn evict_oldest_idle(inner: &mut Inner, within: impl Fn(&Conn) -> bool) -> bool {
        let victim = inner
            .conns
            .iter()
            .filter(|(_, c)| within(c) && c.busy.load(Ordering::SeqCst) == 0)
            .map(|(id, _)| *id)
            .min();
        match victim.and_then(|id| inner.conns.remove(&id)) {
            Some(conn) => {
                conn.evict.notify_one();
                true
            }
            None => false,
        }
    }

    /// A place for a new connection from `peer`, or `None` when it has to be closed.
    pub fn admit(self: &Arc<Self>, peer: IpAddr) -> Option<Slot> {
        let exempt = self.trusted.iter().any(|net| net.contains(peer));
        let key = (!exempt).then(|| ClientKey::from_ip(peer));
        let mut inner = self.lock();
        if let Some(key) = key {
            let same = |c: &Conn| c.peer == Some(key);
            let held = inner.conns.values().filter(|c| same(c)).count();
            if held >= self.max_per_peer && !Self::evict_oldest_idle(&mut inner, same) {
                return None;
            }
        }
        if inner.conns.len() >= self.max_total && !Self::evict_oldest_idle(&mut inner, |_| true) {
            return None;
        }
        let id = inner.next_id;
        inner.next_id += 1;
        let (busy, evict) = (Arc::new(AtomicUsize::new(0)), Arc::new(Notify::new()));
        let conn = Conn {
            peer: key,
            busy: busy.clone(),
            evict: evict.clone(),
        };
        inner.conns.insert(id, conn);
        Some(Slot {
            id,
            table: self.clone(),
            busy,
            evict,
        })
    }

    /// Open connections.
    pub fn len(&self) -> usize {
        self.lock().conns.len()
    }

    /// True when no connection is open.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn the_oldest_idle_connection_gives_way_busy_ones_never() {
        let table = Arc::new(ConnTable::new(2, 10, Vec::new()));
        let first = table.admit(ip("192.0.2.1")).unwrap();
        let second = table.admit(ip("192.0.2.2")).unwrap();
        let _work = BusyGuard::new(second.busy.clone());
        let third = table.admit(ip("192.0.2.3")).unwrap();
        assert_eq!(table.len(), 2);
        // the first (idle) was told to go; the busy second stays
        let told = first.evict.notified();
        tokio::pin!(told);
        assert!(futures_util::FutureExt::now_or_never(told.as_mut()).is_some());
        let _busy_third = BusyGuard::new(third.busy.clone());
        assert!(table.admit(ip("192.0.2.4")).is_none(), "everything busy");
        drop(third);
        assert!(table.admit(ip("192.0.2.4")).is_some());
    }

    #[test]
    fn one_client_cannot_take_more_than_its_share() {
        let table = Arc::new(ConnTable::new(100, 2, Vec::new()));
        let a = table.admit(ip("2001:db8::1")).unwrap();
        let b = table.admit(ip("2001:db8::2")).unwrap();
        let (_ga, _gb) = (
            BusyGuard::new(a.busy.clone()),
            BusyGuard::new(b.busy.clone()),
        );
        assert!(
            table.admit(ip("2001:db8::3")).is_none(),
            "same /64, both busy"
        );
        assert!(table.admit(ip("198.51.100.1")).is_some(), "another client");
        drop(_ga);
        assert!(
            table.admit(ip("2001:db8::4")).is_some(),
            "an idle one gave way"
        );
    }

    #[test]
    fn a_trusted_proxy_is_only_bound_by_the_total() {
        let proxy = ProxyNet::parse("127.0.0.1").unwrap();
        let table = Arc::new(ConnTable::new(3, 1, vec![proxy]));
        let slots: Vec<Slot> = (0..3)
            .map(|_| table.admit(ip("127.0.0.1")).unwrap())
            .collect();
        let guards: Vec<BusyGuard> = slots
            .iter()
            .map(|s| BusyGuard::new(s.busy.clone()))
            .collect();
        assert!(table.admit(ip("127.0.0.1")).is_none());
        drop(guards);
        drop(slots);
        assert!(table.is_empty());
    }
}
