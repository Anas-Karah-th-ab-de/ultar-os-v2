//! The client: what every service, the platform and every app links to
//! reach the bus. One connection, any number of handles and threads.

mod link;
mod service;
mod subscription;

pub use service::{IncomingRequest, ServiceEndpoint};
pub use subscription::{RawEvent, Subscription, TypedSubscription};

use crate::error::BusError;
use crate::message::{decode, decode_versioned, encode, Call, Topic};
use crate::sync::{lock, wait_timeout};
use crate::topic::{check_name, is_valid_segment_name, Pattern};
use crate::wire::{Envelope, Kind, FLAG_ACK, FLAG_RETAIN};
use link::{Inner, ServiceSlot};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::AtomicU64;
use std::sync::mpsc::sync_channel;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};
use subscription::SubscriptionSlot;

/// Stack size of the client's reader thread.
const READER_STACK: usize = 256 * 1024;
/// Pause between connection attempts while waiting for the broker to appear.
const CONNECT_RETRY: Duration = Duration::from_millis(20);

/// Settings of a client connection.
#[derive(Clone, Debug)]
pub struct ClientOptions {
    /// Diagnostic program name shown to the broker (not an identity).
    pub name: String,
    /// Secret for listeners that authenticate by token.
    pub token: Option<String>,
    /// Reconnect and re-announce services and subscriptions after a loss.
    pub reconnect: bool,
    /// First pause before a reconnection attempt; doubles up to the maximum.
    pub reconnect_min: Duration,
    /// Longest pause between reconnection attempts.
    pub reconnect_max: Duration,
    /// How long [`Client::connect`] keeps trying while the broker socket is
    /// not there yet (zero: one attempt).
    pub connect_retry_for: Duration,
    /// Limit for the handshake, for one write, and for acknowledgements.
    pub io_timeout: Duration,
    /// Events one subscription, or requests one service, may buffer locally.
    pub buffer: usize,
}

impl Default for ClientOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            token: None,
            reconnect: true,
            reconnect_min: Duration::from_millis(50),
            reconnect_max: Duration::from_secs(2),
            connect_retry_for: Duration::ZERO,
            io_timeout: Duration::from_secs(5),
            buffer: 256,
        }
    }
}

impl ClientOptions {
    /// Default options with the given diagnostic name.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            ..Self::default()
        }
    }
}

/// Closes the connection when the last [`Client`] handle is dropped.
struct CloseGuard(Arc<Inner>);

impl Drop for CloseGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// A connection to the bus. Cloning is cheap; all clones share the
/// connection, which closes when the last clone is dropped.
#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
    _guard: Arc<CloseGuard>,
}

impl Client {
    /// Connects to the broker socket at `path` and authenticates.
    pub fn connect(path: &Path, options: ClientOptions) -> Result<Self, BusError> {
        let deadline = Instant::now() + options.connect_retry_for;
        let (stream, welcome) = loop {
            match link::establish(path, &options) {
                Ok(connected) => break connected,
                Err(BusError::Io(_)) if Instant::now() < deadline => thread::sleep(CONNECT_RETRY),
                Err(err) => return Err(err),
            }
        };
        let inner = Arc::new(Inner::new(path, options));
        inner.attach(&stream, &welcome)?;
        let reader = inner.clone();
        thread::Builder::new()
            .name("ultar-bus-client".into())
            .stack_size(READER_STACK)
            .spawn(move || link::run(&reader, stream))
            .map_err(BusError::Io)?;
        Ok(Self {
            _guard: Arc::new(CloseGuard(inner.clone())),
            inner,
        })
    }

    /// Identity the broker assigned to this connection.
    pub fn identity(&self) -> String {
        lock(&self.inner.link).identity.clone()
    }

    /// Whether a connection to the broker is currently established.
    pub fn is_connected(&self) -> bool {
        lock(&self.inner.link).is_up()
    }

    /// Blocks until the client is connected (again); `false` on timeout or
    /// when the client is closed.
    pub fn wait_connected(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut link = lock(&self.inner.link);
        loop {
            if link.closed {
                return false;
            }
            if link.is_up() {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            link = wait_timeout(&self.inner.link_changed, link, deadline - now);
        }
    }

    /// Closes the connection for all handles. Pending calls fail with
    /// `Closed`, subscriptions and service endpoints end.
    pub fn close(&self) {
        self.inner.close();
    }

    /// Calls `method` of `service` with an untyped contract and waits at
    /// most `timeout` for the answer.
    pub fn request<T: Serialize, R: DeserializeOwned>(
        &self,
        service: &str,
        method: &str,
        argument: &T,
        timeout: Duration,
    ) -> Result<R, BusError> {
        let answer = self.raw_request(service, method, 0, encode(argument)?, timeout)?;
        decode(&answer.payload).map_err(|_| BusError::Malformed)
    }

    /// Makes the typed call `C` and waits at most `timeout` for its reply.
    pub fn call<C: Call>(&self, call: &C, timeout: Duration) -> Result<C::Reply, BusError> {
        let answer = self.raw_request(C::SERVICE, C::METHOD, C::VERSION, encode(call)?, timeout)?;
        decode_versioned(answer.version, &answer.payload)
    }

    fn raw_request(
        &self,
        service: &str,
        method: &str,
        version: u16,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<Envelope, BusError> {
        check_name(service)?;
        if !is_valid_segment_name(method) {
            return Err(BusError::InvalidName);
        }
        let mut request = Envelope::new(Kind::Request, 0).with_target(service);
        request.method = method.to_owned();
        self.inner
            .round_trip(request.with_payload(version, payload), timeout)
    }

    /// Publishes a typed message on its topic and waits until the broker
    /// accepted it (or refused it, e.g. `PermissionDenied`).
    pub fn publish<T: Topic>(&self, message: &T) -> Result<(), BusError> {
        self.publish_raw(T::TOPIC, T::VERSION, T::RETAINED, encode(message)?, true)
    }

    /// Publishes without waiting for the broker; refusals go unnoticed.
    pub fn publish_unconfirmed<T: Topic>(&self, message: &T) -> Result<(), BusError> {
        self.publish_raw(T::TOPIC, T::VERSION, T::RETAINED, encode(message)?, false)
    }

    /// Publishes pre-encoded bytes on any topic.
    pub fn publish_raw(
        &self,
        topic: &str,
        version: u16,
        retain: bool,
        payload: Vec<u8>,
        confirm: bool,
    ) -> Result<(), BusError> {
        check_name(topic)?;
        let mut event = Envelope::new(Kind::Event, 0).with_target(topic);
        event = event.with_payload(version, payload);
        if retain {
            event.flags |= FLAG_RETAIN;
        }
        if !confirm {
            return self.inner.send(&event, None).map(|_| ());
        }
        event.flags |= FLAG_ACK;
        self.inner
            .round_trip(event, self.inner.options.io_timeout)
            .map(|_| ())
    }

    /// Subscribes to the topic of `T`. A retained value, if any, arrives
    /// first.
    pub fn subscribe<T: Topic>(&self) -> Result<TypedSubscription<T>, BusError> {
        self.subscribe_pattern(T::TOPIC).map(TypedSubscription::new)
    }

    /// Subscribes to a topic (`power.state`), a prefix (`devices.*`) or
    /// everything (`*`).
    pub fn subscribe_pattern(&self, pattern: &str) -> Result<Subscription, BusError> {
        let pattern = Pattern::parse(pattern)?;
        let id = self.inner.next_id();
        let (sender, receiver) = sync_channel(self.inner.options.buffer.max(1));
        let lost = Arc::new(AtomicU64::new(0));
        let subscribe = Envelope::new(Kind::Subscribe, id).with_target(&pattern.to_string());
        let slot = SubscriptionSlot {
            id,
            pattern,
            sender,
            lost: lost.clone(),
        };
        // Registered before the request is sent so that retained values,
        // which follow the acknowledgement immediately, find the slot.
        lock(&self.inner.subscriptions).push(slot);
        let subscription = Subscription {
            id,
            inner: self.inner.clone(),
            receiver,
            lost,
        };
        // On failure the subscription is dropped, which removes the slot.
        self.inner
            .round_trip_with_id(subscribe, self.inner.options.io_timeout)?;
        Ok(subscription)
    }

    /// Registers `service` on this connection and returns its inbox.
    pub fn register(&self, service: &str) -> Result<ServiceEndpoint, BusError> {
        check_name(service)?;
        let registration = self.inner.next_id();
        let (inbox, requests) = sync_channel(self.inner.options.buffer.max(1));
        {
            let mut services = lock(&self.inner.services);
            if services.contains_key(service) {
                return Err(BusError::Remote(crate::ErrorCode::NameTaken));
            }
            services.insert(
                service.to_owned(),
                ServiceSlot {
                    registration,
                    inbox,
                },
            );
        }
        let endpoint = ServiceEndpoint {
            name: service.to_owned(),
            inner: self.inner.clone(),
            inbox: requests,
        };
        let register = Envelope::new(Kind::Register, registration).with_target(service);
        // On failure the endpoint is dropped, which forgets the service.
        self.inner
            .round_trip_with_id(register, self.inner.options.io_timeout)?;
        Ok(endpoint)
    }
}
