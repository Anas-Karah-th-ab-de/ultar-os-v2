//! A service's connection to the bus: connect with back-off, register with
//! retry, and bring retained topics back after a broker restart.
//!
//! The bus client reconnects by itself and announces its services and
//! subscriptions again. What it cannot know is which **retained** values a
//! service had published: the broker keeps them in memory, so a restarted
//! broker has none, and a client that subscribes afterwards would never
//! learn the device's capabilities or the store report. A [`BusSession`]
//! remembers the last value of every retained topic it published and
//! publishes all of them again whenever the connection generation changes.
//! The watcher thread sleeps on the client's connection state; nothing
//! polls.

use crate::secret::Secret;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode, ServiceEndpoint, Topic};

/// Most retained topics one session remembers.
pub const MAX_RETAINED_TOPICS: usize = 64;

/// How a session connects.
#[derive(Debug)]
pub struct SessionOptions {
    /// Diagnostic program name shown to the broker.
    pub name: String,
    /// Token for listeners that authenticate by token.
    pub token: Option<Secret>,
    /// How long to keep trying while the broker is not there (zero: one
    /// attempt).
    pub connect_wait: Duration,
    /// How long to keep trying while a service name is still held by a
    /// previous instance (zero: one attempt).
    pub register_wait: Duration,
    /// First pause between attempts; doubles up to `backoff_max`.
    pub backoff_min: Duration,
    /// Longest pause between attempts.
    pub backoff_max: Duration,
}

impl SessionOptions {
    /// Ten seconds of patience, pauses from 20 ms to 1 s.
    pub fn named(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            token: None,
            connect_wait: Duration::from_secs(10),
            register_wait: Duration::from_secs(10),
            backoff_min: Duration::from_millis(20),
            backoff_max: Duration::from_secs(1),
        }
    }
}

/// The pauses of an exponential back-off: `min, 2·min, 4·min, … max, max`.
#[derive(Clone, Copy, Debug)]
pub struct Backoff {
    next: Duration,
    max: Duration,
}

impl Backoff {
    /// A back-off from `min` (at least 1 ms) to `max` (at least `min`).
    pub fn new(min: Duration, max: Duration) -> Self {
        let next = min.max(Duration::from_millis(1));
        Self {
            next,
            max: max.max(next),
        }
    }

    /// The next pause.
    pub fn step(&mut self) -> Duration {
        let pause = self.next;
        self.next = self.next.saturating_mul(2).min(self.max);
        pause
    }
}

/// Retries `attempt` with back-off while `retry` says the error is
/// temporary and the deadline has not passed.
fn with_retry<T>(
    wait: Duration,
    mut backoff: Backoff,
    retry: impl Fn(&BusError) -> bool,
    mut attempt: impl FnMut() -> Result<T, BusError>,
) -> Result<T, BusError> {
    let deadline = Instant::now() + wait;
    loop {
        match attempt() {
            Err(error) if retry(&error) => {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    return Err(error);
                }
                thread::sleep(backoff.step().min(left));
            }
            other => return other,
        }
    }
}

#[derive(Clone)]
struct Retained {
    version: u16,
    payload: Vec<u8>,
}

struct Shared {
    client: Client,
    retained: Mutex<BTreeMap<String, Retained>>,
    republished: AtomicU64,
    failed: AtomicU64,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// Publishes every remembered value again; returns how many failed.
    fn republish(&self) -> u64 {
        let values: Vec<(String, Retained)> = lock(&self.retained)
            .iter()
            .map(|(topic, value)| (topic.clone(), value.clone()))
            .collect();
        let mut failed = 0;
        for (topic, value) in values {
            let sent = self
                .client
                .publish_raw(&topic, value.version, true, value.payload, true);
            match sent {
                Ok(()) => self.republished.fetch_add(1, Ordering::Relaxed),
                Err(_) => {
                    failed += 1;
                    self.failed.fetch_add(1, Ordering::Relaxed)
                }
            };
        }
        failed
    }
}

/// Body of the watcher thread.
fn watch(shared: &Shared, mut seen: u64) {
    while let Some(generation) = shared.client.wait_reconnected(seen) {
        // A failure here means the connection broke again in the middle:
        // the next generation repeats the whole set.
        shared.republish();
        seen = generation;
    }
}

/// A bus connection that restores what a broker restart loses.
pub struct BusSession {
    shared: Arc<Shared>,
    register_wait: Duration,
    backoff: Backoff,
}

impl BusSession {
    /// Connects to the broker at `socket`, retrying with back-off while
    /// the socket is not there or refuses connections.
    pub fn connect(socket: &Path, options: SessionOptions) -> Result<Self, BusError> {
        let backoff = Backoff::new(options.backoff_min, options.backoff_max);
        let client_options = || ClientOptions {
            token: options
                .token
                .as_ref()
                .and_then(|token| token.expose_str().map(str::to_owned)),
            ..ClientOptions::named(&options.name)
        };
        let client = with_retry(
            options.connect_wait,
            backoff,
            |error| {
                matches!(
                    error,
                    BusError::Io(_) | BusError::Closed | BusError::Timeout
                )
            },
            || Client::connect(socket, client_options()),
        )?;
        let seen = client.generation();
        let shared = Arc::new(Shared {
            client,
            retained: Mutex::new(BTreeMap::new()),
            republished: AtomicU64::new(0),
            failed: AtomicU64::new(0),
        });
        let watcher = shared.clone();
        thread::Builder::new()
            .name("ultar-bus-session".into())
            .stack_size(128 * 1024)
            .spawn(move || watch(&watcher, seen))
            .map_err(BusError::Io)?;
        Ok(Self {
            shared,
            register_wait: options.register_wait,
            backoff,
        })
    }

    /// The underlying client (cheap to clone).
    pub fn client(&self) -> &Client {
        &self.shared.client
    }

    /// Registers `service`, retrying while the name is still held by a
    /// previous instance whose connection the broker has not cleaned up.
    /// After a broker restart the client registers the name again itself.
    pub fn register(&self, service: &str) -> Result<ServiceEndpoint, BusError> {
        with_retry(
            self.register_wait,
            self.backoff,
            |error| matches!(error, BusError::Remote(ErrorCode::NameTaken)),
            || self.shared.client.register(service),
        )
    }

    /// Remembers `payload` as the last value of the retained `topic`
    /// without publishing it now (for values some other code path already
    /// published). Refused beyond [`MAX_RETAINED_TOPICS`] topics.
    pub fn remember_raw(
        &self,
        topic: &str,
        version: u16,
        payload: Vec<u8>,
    ) -> Result<(), BusError> {
        if !ultar_bus::is_valid_name(topic) {
            return Err(BusError::InvalidName);
        }
        let mut retained = lock(&self.shared.retained);
        if !retained.contains_key(topic) && retained.len() >= MAX_RETAINED_TOPICS {
            return Err(BusError::Remote(ErrorCode::LimitExceeded));
        }
        retained.insert(topic.to_owned(), Retained { version, payload });
        Ok(())
    }

    /// Typed form of [`Self::remember_raw`].
    pub fn remember<T: Topic>(&self, message: &T) -> Result<(), BusError> {
        self.remember_raw(T::TOPIC, T::VERSION, ultar_bus::encode(message)?)
    }

    /// Publishes a retained value and remembers it. The value is
    /// remembered even when publishing fails right now (broker away): it
    /// goes out with the next connection.
    pub fn publish_retained<T: Topic>(&self, message: &T) -> Result<(), BusError> {
        let payload = ultar_bus::encode(message)?;
        self.remember_raw(T::TOPIC, T::VERSION, payload.clone())?;
        self.shared
            .client
            .publish_raw(T::TOPIC, T::VERSION, true, payload, true)
    }

    /// Retained values published again after reconnections, since start.
    pub fn republished(&self) -> u64 {
        self.shared.republished.load(Ordering::Relaxed)
    }

    /// Re-publications that failed (they are repeated on the next
    /// connection).
    pub fn republish_failures(&self) -> u64 {
        self.shared.failed.load(Ordering::Relaxed)
    }

    /// Number of remembered retained topics.
    pub fn retained_topics(&self) -> usize {
        lock(&self.shared.retained).len()
    }

    /// Closes the connection; the watcher thread ends.
    pub fn close(&self) {
        self.shared.client.close();
    }
}

impl Drop for BusSession {
    fn drop(&mut self) {
        // The watcher holds a client handle; without this it would keep
        // the connection alive for ever.
        self.shared.client.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_doubles_up_to_the_maximum() {
        let mut backoff = Backoff::new(Duration::from_millis(20), Duration::from_millis(100));
        let pauses: Vec<u64> = (0..5).map(|_| backoff.step().as_millis() as u64).collect();
        assert_eq!(pauses, [20, 40, 80, 100, 100]);
        let mut odd = Backoff::new(Duration::ZERO, Duration::ZERO);
        assert_eq!(odd.step(), Duration::from_millis(1));
        assert_eq!(odd.step(), Duration::from_millis(1));
    }

    #[test]
    fn retry_stops_at_success_fatal_errors_and_the_deadline() {
        let backoff = Backoff::new(Duration::from_millis(1), Duration::from_millis(1));
        let temporary = |error: &BusError| matches!(error, BusError::Timeout);
        let mut calls = 0;
        let ok = with_retry(Duration::from_secs(30), backoff, temporary, || {
            calls += 1;
            if calls < 3 {
                Err(BusError::Timeout)
            } else {
                Ok(calls)
            }
        });
        assert_eq!(ok.unwrap(), 3);
        let mut calls = 0;
        let fatal: Result<(), _> = with_retry(Duration::from_secs(30), backoff, temporary, || {
            calls += 1;
            Err(BusError::AuthFailed)
        });
        assert!(matches!(fatal, Err(BusError::AuthFailed)));
        assert_eq!(calls, 1);
        let mut calls = 0;
        let late: Result<(), _> = with_retry(Duration::ZERO, backoff, temporary, || {
            calls += 1;
            Err(BusError::Timeout)
        });
        assert!(matches!(late, Err(BusError::Timeout)));
        assert_eq!(calls, 1);
    }

    #[test]
    fn options_do_not_print_the_token() {
        let mut options = SessionOptions::named("x");
        options.token = Some(Secret::from_bytes(b"tok-123".to_vec()).unwrap());
        assert!(!format!("{options:?}").contains("tok-123"));
    }
}
