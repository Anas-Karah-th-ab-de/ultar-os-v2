//! Routing state of the broker: connections, services, subscriptions,
//! retained values and pending requests. Everything here runs under the one
//! state lock, which is what gives every topic a single delivery order.

use crate::error::ErrorCode;
use crate::identity::Identity;
use crate::sync::lock;
use crate::topic::Pattern;
use crate::wire::{Envelope, Kind};
use std::collections::HashMap;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Broker-internal connection number.
pub(crate) type ConnId = u64;
/// One encoded frame, shared between all receivers of an event.
pub(crate) type Frame = Arc<Vec<u8>>;

/// Number of pending requests above which expired ones are purged.
const PURGE_THRESHOLD: usize = 256;
/// Shortest time between two purges.
const PURGE_INTERVAL: Duration = Duration::from_secs(1);

/// Handle to end a connection from any thread.
pub(crate) struct LinkControl {
    stream: UnixStream,
    reason: Mutex<Option<ErrorCode>>,
    closing: AtomicBool,
}

impl LinkControl {
    pub(crate) fn new(stream: UnixStream) -> Self {
        Self {
            stream,
            reason: Mutex::new(None),
            closing: AtomicBool::new(false),
        }
    }

    /// Records why the connection ends; the first reason wins.
    pub(crate) fn set_reason(&self, code: ErrorCode) {
        lock(&self.reason).get_or_insert(code);
        self.closing.store(true, Ordering::SeqCst);
    }

    /// Ends the connection: the reader stops, the writer sends `code` as
    /// last frame if the peer still accepts data. Returns `false` when the
    /// connection was already closing.
    pub(crate) fn close_with(&self, code: ErrorCode) -> bool {
        let first = !self.closing.load(Ordering::SeqCst);
        self.set_reason(code);
        self.hang_up();
        first
    }

    /// Wakes the reader thread so that it cleans the connection up.
    pub(crate) fn hang_up(&self) {
        let _ = self.stream.shutdown(Shutdown::Read);
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    pub(crate) fn reason(&self) -> Option<ErrorCode> {
        *lock(&self.reason)
    }
}

/// Everything the broker knows about one authenticated connection.
pub(crate) struct ConnEntry {
    pub(crate) identity: Identity,
    pub(crate) queue: SyncSender<Frame>,
    pub(crate) control: Arc<LinkControl>,
    /// Listener the connection came in on (limits are per listener).
    pub(crate) listener: usize,
    /// Bytes waiting in `queue`; the writer subtracts what it has written.
    pub(crate) queued: Arc<AtomicUsize>,
    pub(crate) subscriptions: Vec<(u64, Pattern)>,
    pub(crate) services: Vec<String>,
}

/// Last value of a retained topic.
pub(crate) struct Retained {
    pub(crate) version: u16,
    pub(crate) payload: Vec<u8>,
}

/// A request that was forwarded to a service and awaits its answer.
pub(crate) struct Pending {
    pub(crate) caller: ConnId,
    pub(crate) caller_id: u64,
    pub(crate) service: ConnId,
    pub(crate) expires: Instant,
}

/// Outcome of putting a frame into a connection's queue.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Queued {
    /// The frame is in the queue.
    Yes,
    /// The queue is full.
    Full,
    /// The connection is gone.
    Gone,
}

/// The mutable routing tables.
pub(crate) struct State {
    pub(crate) conns: HashMap<ConnId, ConnEntry>,
    pub(crate) services: HashMap<String, ConnId>,
    pub(crate) retained: HashMap<String, Retained>,
    pub(crate) pending: HashMap<u64, Pending>,
    pub(crate) next_request: u64,
    pub(crate) dropped_slow: u64,
    /// Upper bound of bytes waiting for one connection.
    pub(crate) queue_bytes: usize,
    last_purge: Instant,
}

impl State {
    pub(crate) fn new() -> Self {
        Self {
            conns: HashMap::new(),
            services: HashMap::new(),
            retained: HashMap::new(),
            pending: HashMap::new(),
            next_request: 1,
            dropped_slow: 0,
            queue_bytes: usize::MAX,
            last_purge: Instant::now(),
        }
    }

    /// Tries to queue a frame without any consequence for a full queue.
    pub(crate) fn try_queue(&self, conn: ConnId, frame: Frame) -> Queued {
        let Some(entry) = self.conns.get(&conn) else {
            return Queued::Gone;
        };
        if entry.control.is_closing() {
            return Queued::Gone;
        }
        let len = frame.len();
        let waiting = entry.queued.load(Ordering::SeqCst);
        // One frame always fits, whatever its size.
        if waiting > 0 && waiting.saturating_add(len) > self.queue_bytes {
            return Queued::Full;
        }
        entry.queued.fetch_add(len, Ordering::SeqCst);
        match entry.queue.try_send(frame) {
            Ok(()) => Queued::Yes,
            Err(TrySendError::Full(_)) => {
                entry.queued.fetch_sub(len, Ordering::SeqCst);
                Queued::Full
            }
            Err(TrySendError::Disconnected(_)) => {
                entry.queued.fetch_sub(len, Ordering::SeqCst);
                entry.control.hang_up();
                Queued::Gone
            }
        }
    }

    /// Queues a frame; a receiver whose queue is full is dropped as slow.
    pub(crate) fn deliver(&mut self, conn: ConnId, frame: Frame) {
        if self.try_queue(conn, frame) != Queued::Full {
            return;
        }
        if let Some(entry) = self.conns.get(&conn) {
            if entry.control.close_with(ErrorCode::SlowConsumer) {
                self.dropped_slow += 1;
            }
        }
    }

    /// Encodes and delivers an envelope.
    pub(crate) fn send(&mut self, conn: ConnId, envelope: &Envelope) {
        if let Ok(frame) = envelope.to_frame(u32::MAX) {
            self.deliver(conn, Arc::new(frame));
        }
    }

    /// Sends a typed error for correlation `id`.
    pub(crate) fn reply_error(&mut self, conn: ConnId, id: u64, code: ErrorCode) {
        self.send(conn, &Envelope::error(id, code));
    }

    /// Acknowledges the operation with correlation `id`.
    pub(crate) fn ack(&mut self, conn: ConnId, id: u64) {
        self.send(conn, &Envelope::new(Kind::Response, id));
    }

    /// Remembers a forwarded request and occasionally purges expired ones.
    pub(crate) fn remember(&mut self, broker_id: u64, pending: Pending) {
        let now = Instant::now();
        if self.pending.len() >= PURGE_THRESHOLD
            && now.duration_since(self.last_purge) >= PURGE_INTERVAL
        {
            self.pending.retain(|_, entry| entry.expires > now);
            self.last_purge = now;
        }
        self.pending.insert(broker_id, pending);
    }

    /// Forgets a connection: its services, subscriptions and requests.
    /// Callers that wait for one of its services get `ServiceGone`.
    pub(crate) fn remove_conn(&mut self, conn: ConnId) {
        let Some(entry) = self.conns.remove(&conn) else {
            return;
        };
        for service in &entry.services {
            if self.services.get(service) == Some(&conn) {
                self.services.remove(service);
            }
        }
        let mut orphaned = Vec::new();
        self.pending.retain(|_, pending| {
            if pending.service == conn {
                orphaned.push((pending.caller, pending.caller_id));
            }
            pending.service != conn && pending.caller != conn
        });
        for (caller, caller_id) in orphaned {
            self.reply_error(caller, caller_id, ErrorCode::ServiceGone);
        }
    }

    /// Total number of subscriptions over all connections.
    pub(crate) fn subscription_count(&self) -> usize {
        self.conns
            .values()
            .map(|entry| entry.subscriptions.len())
            .sum()
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::wire::read_envelope;
    use std::sync::mpsc::{sync_channel, Receiver};

    /// A connection entry backed by a socket pair, plus its outbound queue.
    pub(crate) struct Probe {
        pub(crate) queue: Receiver<Frame>,
        pub(crate) control: Arc<LinkControl>,
        _peer: UnixStream,
    }

    impl Probe {
        /// Next queued envelope, if any.
        pub(crate) fn next(&self) -> Option<Envelope> {
            let frame = self.queue.try_recv().ok()?;
            read_envelope(&mut frame.as_slice(), u32::MAX).ok()
        }
    }

    /// Adds a connection named `identity` with a queue of `capacity` frames.
    pub(crate) fn add_conn(
        state: &mut State,
        conn: ConnId,
        identity: &str,
        capacity: usize,
    ) -> Probe {
        let (local, peer) = UnixStream::pair().unwrap();
        let (tx, rx) = sync_channel(capacity);
        let control = Arc::new(LinkControl::new(local));
        state.conns.insert(
            conn,
            ConnEntry {
                identity: Identity::new(identity).unwrap(),
                queue: tx,
                control: control.clone(),
                listener: 0,
                queued: Arc::new(AtomicUsize::new(0)),
                subscriptions: Vec::new(),
                services: Vec::new(),
            },
        );
        Probe {
            queue: rx,
            control,
            _peer: peer,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::add_conn;
    use super::*;

    fn pending(caller: ConnId, service: ConnId, ttl: Duration) -> Pending {
        Pending {
            caller,
            caller_id: 7,
            service,
            expires: Instant::now() + ttl,
        }
    }

    #[test]
    fn full_queue_drops_the_receiver_once() {
        let mut state = State::new();
        let probe = add_conn(&mut state, 1, "app.slow", 2);
        for id in 0..5 {
            state.ack(1, id);
        }
        assert_eq!(state.dropped_slow, 1);
        assert_eq!(probe.control.reason(), Some(ErrorCode::SlowConsumer));
        assert!(probe.control.is_closing());
        assert_eq!(probe.next().map(|e| e.id), Some(0));
        assert_eq!(probe.next().map(|e| e.id), Some(1));
        assert!(probe.next().is_none());
    }

    #[test]
    fn try_queue_reports_full_and_gone_without_dropping() {
        let mut state = State::new();
        let probe = add_conn(&mut state, 1, "svc", 1);
        let frame = || Arc::new(Envelope::new(Kind::Event, 0).to_frame(64).unwrap());
        assert_eq!(state.try_queue(1, frame()), Queued::Yes);
        assert_eq!(state.try_queue(1, frame()), Queued::Full);
        assert!(!probe.control.is_closing());
        assert_eq!(state.try_queue(99, frame()), Queued::Gone);
        drop(probe.queue);
        assert_eq!(state.try_queue(1, frame()), Queued::Gone);
    }

    #[test]
    fn first_close_reason_wins() {
        let mut state = State::new();
        let probe = add_conn(&mut state, 1, "a", 1);
        assert!(probe.control.close_with(ErrorCode::Malformed));
        assert!(!probe.control.close_with(ErrorCode::SlowConsumer));
        assert_eq!(probe.control.reason(), Some(ErrorCode::Malformed));
    }

    #[test]
    fn removing_a_service_fails_its_callers() {
        let mut state = State::new();
        let caller = add_conn(&mut state, 1, "app.caller", 8);
        let _service = add_conn(&mut state, 2, "system.svc", 8);
        state.services.insert("settings".into(), 2);
        if let Some(entry) = state.conns.get_mut(&2) {
            entry.services.push("settings".into());
        }
        state.remember(50, pending(1, 2, Duration::from_secs(5)));
        state.remove_conn(2);
        assert!(state.services.is_empty());
        assert!(state.pending.is_empty());
        let error = caller.next().unwrap();
        assert_eq!(
            (error.kind, error.id, error.error_code()),
            (Kind::Error, 7, ErrorCode::ServiceGone)
        );
        state.remove_conn(2);
    }

    #[test]
    fn removing_a_caller_forgets_its_requests() {
        let mut state = State::new();
        let _caller = add_conn(&mut state, 1, "app.caller", 8);
        let service = add_conn(&mut state, 2, "system.svc", 8);
        state.remember(50, pending(1, 2, Duration::from_secs(5)));
        state.remove_conn(1);
        assert!(state.pending.is_empty());
        assert!(service.next().is_none());
    }

    #[test]
    fn expired_requests_are_purged_lazily() {
        let mut state = State::new();
        for id in 0..PURGE_THRESHOLD as u64 {
            state.remember(id, pending(1, 2, Duration::ZERO));
        }
        state.last_purge = Instant::now() - PURGE_INTERVAL * 2;
        state.remember(9999, pending(1, 2, Duration::from_secs(60)));
        assert_eq!(state.pending.len(), 1);
        assert!(state.pending.contains_key(&9999));
    }

    #[test]
    fn subscriptions_are_counted() {
        let mut state = State::new();
        let _a = add_conn(&mut state, 1, "a", 1);
        let _b = add_conn(&mut state, 2, "b", 1);
        for conn in [1, 2] {
            if let Some(entry) = state.conns.get_mut(&conn) {
                entry.subscriptions.push((conn, Pattern::any()));
            }
        }
        assert_eq!(state.subscription_count(), 2);
    }
}
