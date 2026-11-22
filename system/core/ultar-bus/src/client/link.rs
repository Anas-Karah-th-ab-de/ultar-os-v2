//! Connection management of the client: handshake, the reader thread,
//! dispatch of incoming envelopes and automatic reconnection.

use super::service::IncomingRequest;
use super::subscription::{RawEvent, SubscriptionItem, SubscriptionSlot};
use super::ClientOptions;
use crate::error::{BusError, ErrorCode};
use crate::message::{decode, encode};
use crate::sync::{lock, wait_timeout};
use crate::wire::{
    read_envelope, read_preamble, write_envelope, write_preamble, Envelope, Hello, Kind, Welcome,
    FLAG_RETAIN, PROTOCOL_VERSION,
};
use std::collections::HashMap;
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// Extra room for fields the broker adds to forwarded envelopes.
const INBOUND_SLACK: u32 = 4096;

/// A registered service on the client side.
pub(crate) struct ServiceSlot {
    pub(crate) registration: u64,
    pub(crate) inbox: SyncSender<IncomingRequest>,
}

/// The current connection, if any.
pub(crate) struct Link {
    stream: Option<UnixStream>,
    pub(crate) generation: u64,
    pub(crate) closed: bool,
    pub(crate) identity: String,
    max_frame: u32,
}

impl Link {
    /// Whether a connection is currently established.
    pub(crate) fn is_up(&self) -> bool {
        self.stream.is_some()
    }
}

type Waiter = SyncSender<Result<Envelope, BusError>>;

/// State shared by all handles of one client and its reader thread.
pub(crate) struct Inner {
    path: PathBuf,
    pub(crate) options: ClientOptions,
    pub(crate) link: Mutex<Link>,
    pub(crate) link_changed: Condvar,
    pending: Mutex<HashMap<u64, Waiter>>,
    pub(crate) subscriptions: Mutex<Vec<SubscriptionSlot>>,
    pub(crate) services: Mutex<HashMap<String, ServiceSlot>>,
    next_id: AtomicU64,
}

/// Connects and performs the handshake.
pub(crate) fn establish(
    path: &Path,
    options: &ClientOptions,
) -> Result<(UnixStream, Welcome), BusError> {
    let mut stream = UnixStream::connect(path).map_err(BusError::Io)?;
    stream.set_read_timeout(Some(options.io_timeout))?;
    stream.set_write_timeout(Some(options.io_timeout))?;
    write_preamble(&mut stream, PROTOCOL_VERSION)?;
    let theirs = read_preamble(&mut stream)?;
    if theirs != PROTOCOL_VERSION {
        return Err(BusError::ProtocolMismatch {
            ours: PROTOCOL_VERSION,
            theirs,
        });
    }
    let hello = Hello {
        client_name: options.name.clone(),
        token: options.token.clone(),
    };
    let hello = Envelope::new(Kind::Hello, 0).with_payload(PROTOCOL_VERSION, encode(&hello)?);
    write_envelope(&mut stream, &hello, u32::MAX)?;
    let answer = read_envelope(&mut stream, INBOUND_SLACK)?;
    let welcome = match answer.kind {
        Kind::Welcome => decode::<Welcome>(&answer.payload)?,
        Kind::Error if answer.error_code() == ErrorCode::AuthFailed => {
            return Err(BusError::AuthFailed)
        }
        Kind::Error => return Err(BusError::Remote(answer.error_code())),
        _ => return Err(BusError::Malformed),
    };
    stream.set_read_timeout(None)?;
    Ok((stream, welcome))
}

/// Whether retrying the connection can never succeed.
fn is_fatal(err: &BusError) -> bool {
    matches!(
        err,
        BusError::ProtocolMismatch { .. } | BusError::AuthFailed | BusError::BadMagic
    )
}

impl Inner {
    pub(crate) fn new(path: &Path, options: ClientOptions) -> Self {
        Self {
            path: path.to_owned(),
            options,
            link: Mutex::new(Link {
                stream: None,
                generation: 0,
                closed: false,
                identity: String::new(),
                max_frame: 0,
            }),
            link_changed: Condvar::new(),
            pending: Mutex::new(HashMap::new()),
            subscriptions: Mutex::new(Vec::new()),
            services: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// A fresh correlation id (never `0`).
    pub(crate) fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Writes one envelope on the current connection. With `generation`
    /// the envelope is only sent if that connection is still the current
    /// one. Returns the generation it was sent on.
    pub(crate) fn send(
        &self,
        envelope: &Envelope,
        generation: Option<u64>,
    ) -> Result<u64, BusError> {
        let mut link = lock(&self.link);
        if link.closed {
            return Err(BusError::Closed);
        }
        if generation.is_some_and(|wanted| wanted != link.generation) {
            return Err(BusError::NotConnected);
        }
        let frame = envelope.to_frame(link.max_frame)?;
        let Some(stream) = link.stream.as_mut() else {
            return Err(BusError::NotConnected);
        };
        if let Err(err) = std::io::Write::write_all(stream, &frame) {
            // A partial frame may be on the wire; the connection is unusable.
            let _ = stream.shutdown(Shutdown::Both);
            link.stream = None;
            return Err(BusError::from(err));
        }
        Ok(link.generation)
    }

    /// Sends `envelope` under a fresh id and waits for its answer.
    pub(crate) fn round_trip(
        &self,
        mut envelope: Envelope,
        timeout: Duration,
    ) -> Result<Envelope, BusError> {
        let id = self.next_id();
        envelope.id = id;
        self.round_trip_with_id(envelope, timeout)
    }

    /// Sends `envelope` under the id it already carries and waits.
    pub(crate) fn round_trip_with_id(
        &self,
        envelope: Envelope,
        timeout: Duration,
    ) -> Result<Envelope, BusError> {
        let id = envelope.id;
        let (waiter, answer) = sync_channel(1);
        lock(&self.pending).insert(id, waiter);
        if let Err(err) = self.send(&envelope, None) {
            lock(&self.pending).remove(&id);
            return Err(err);
        }
        let answer = match answer.recv_timeout(timeout) {
            Ok(answer) => answer?,
            Err(RecvTimeoutError::Timeout) => {
                lock(&self.pending).remove(&id);
                return Err(BusError::Timeout);
            }
            Err(RecvTimeoutError::Disconnected) => return Err(BusError::Closed),
        };
        match answer.kind {
            Kind::Error => Err(BusError::Remote(answer.error_code())),
            _ => Ok(answer),
        }
    }

    /// Makes `stream` the current connection and re-announces services and
    /// subscriptions on it.
    pub(crate) fn attach(&self, stream: &UnixStream, welcome: &Welcome) -> Result<(), BusError> {
        let mut writer = stream.try_clone()?;
        let mut link = lock(&self.link);
        if link.closed {
            return Err(BusError::Closed);
        }
        for (name, slot) in lock(&self.services).iter() {
            let register = Envelope::new(Kind::Register, slot.registration).with_target(name);
            write_envelope(&mut writer, &register, u32::MAX)?;
        }
        for slot in lock(&self.subscriptions).iter() {
            let subscribe =
                Envelope::new(Kind::Subscribe, slot.id).with_target(&slot.pattern.to_string());
            write_envelope(&mut writer, &subscribe, u32::MAX)?;
        }
        link.stream = Some(writer);
        link.generation += 1;
        link.identity = welcome.identity.clone();
        link.max_frame = welcome.max_frame;
        self.link_changed.notify_all();
        Ok(())
    }

    /// Ends the client for good.
    pub(crate) fn close(&self) {
        let mut link = lock(&self.link);
        link.closed = true;
        if let Some(stream) = link.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.link_changed.notify_all();
    }

    fn max_inbound(&self) -> u32 {
        lock(&self.link).max_frame.saturating_add(INBOUND_SLACK)
    }

    /// Cleans up after a lost connection. Returns `true` when the client
    /// must not reconnect.
    fn on_disconnect(&self, cause: &BusError) -> bool {
        let closed = {
            let mut link = lock(&self.link);
            if let Some(stream) = link.stream.take() {
                let _ = stream.shutdown(Shutdown::Both);
            }
            self.link_changed.notify_all();
            link.closed
        };
        for (_, waiter) in lock(&self.pending).drain() {
            let _ = waiter.try_send(Err(BusError::Closed));
        }
        if matches!(cause, BusError::Remote(ErrorCode::SlowConsumer)) {
            for slot in lock(&self.subscriptions).iter() {
                slot.push(SubscriptionItem::Lagged);
            }
        }
        closed || !self.options.reconnect
    }

    /// Releases everything that waits on this client.
    fn finish(&self) {
        self.close();
        lock(&self.pending).clear();
        lock(&self.subscriptions).clear();
        lock(&self.services).clear();
    }

    /// Tries to connect again with exponential back-off. `None` when the
    /// client was closed or the broker rejects it permanently.
    fn reconnect(&self) -> Option<UnixStream> {
        let mut delay = self.options.reconnect_min.max(Duration::from_millis(1));
        loop {
            {
                let link = lock(&self.link);
                if link.closed {
                    return None;
                }
                if wait_timeout(&self.link_changed, link, delay).closed {
                    return None;
                }
            }
            match establish(&self.path, &self.options) {
                Ok((stream, welcome)) => {
                    if self.attach(&stream, &welcome).is_ok() {
                        return Some(stream);
                    }
                }
                Err(err) if is_fatal(&err) => return None,
                Err(_) => {}
            }
            delay = (delay * 2).min(self.options.reconnect_max.max(delay));
        }
    }

    fn read_until_error(self: &Arc<Self>, stream: &mut UnixStream) -> BusError {
        let max_inbound = self.max_inbound();
        loop {
            let outcome =
                read_envelope(stream, max_inbound).and_then(|envelope| self.dispatch(envelope));
            if let Err(err) = outcome {
                return err;
            }
        }
    }

    fn dispatch(self: &Arc<Self>, envelope: Envelope) -> Result<(), BusError> {
        match envelope.kind {
            Kind::Error if envelope.id == 0 => Err(BusError::Remote(envelope.error_code())),
            Kind::Response | Kind::Error => {
                self.on_answer(envelope);
                Ok(())
            }
            Kind::Event => {
                self.on_event(envelope);
                Ok(())
            }
            Kind::Request => {
                self.on_request(envelope);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn on_answer(&self, envelope: Envelope) {
        let id = envelope.id;
        if let Some(waiter) = lock(&self.pending).remove(&id) {
            let _ = waiter.try_send(Ok(envelope));
            return;
        }
        if envelope.kind != Kind::Error {
            return;
        }
        // Nobody waits: the broker refused something that was re-announced
        // after a reconnect (e.g. the policy changed in between).
        let code = envelope.error_code();
        lock(&self.subscriptions).retain(|slot| {
            if slot.id == id {
                slot.push(SubscriptionItem::Failed(code));
            }
            slot.id != id
        });
        lock(&self.services).retain(|_, slot| slot.registration != id);
    }

    fn on_event(&self, envelope: Envelope) {
        let event = RawEvent {
            topic: envelope.target,
            version: envelope.version,
            retained: envelope.flags & FLAG_RETAIN != 0,
            payload: envelope.payload,
        };
        let subscriptions = lock(&self.subscriptions);
        let mut receivers = subscriptions.iter().filter(|slot| match envelope.id {
            0 => slot.pattern.matches(&event.topic),
            id => slot.id == id,
        });
        let Some(mut current) = receivers.next() else {
            return;
        };
        for next in receivers {
            current.push(SubscriptionItem::Event(event.clone()));
            current = next;
        }
        current.push(SubscriptionItem::Event(event));
    }

    fn on_request(self: &Arc<Self>, envelope: Envelope) {
        let id = envelope.id;
        let generation = lock(&self.link).generation;
        let refusal = {
            let services = lock(&self.services);
            match services.get(&envelope.target) {
                None => Some(ErrorCode::UnknownService),
                Some(slot) => {
                    let request = IncomingRequest::new(self.clone(), generation, envelope);
                    match slot.inbox.try_send(request) {
                        Ok(()) => None,
                        Err(TrySendError::Full(request)) => {
                            Some(request.disarm(ErrorCode::ServiceBusy))
                        }
                        Err(TrySendError::Disconnected(request)) => {
                            Some(request.disarm(ErrorCode::UnknownService))
                        }
                    }
                }
            }
        };
        if let Some(code) = refusal {
            let _ = self.send(&Envelope::error(id, code), Some(generation));
        }
    }
}

/// Body of the reader thread.
pub(crate) fn run(inner: &Arc<Inner>, mut stream: UnixStream) {
    loop {
        let cause = inner.read_until_error(&mut stream);
        if inner.on_disconnect(&cause) {
            break;
        }
        match inner.reconnect() {
            Some(next) => stream = next,
            None => break,
        }
    }
    inner.finish();
}
