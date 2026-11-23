//! The logging API of every Ultar service.
//!
//! ```no_run
//! use ultar_service::log::{Level, LogTarget, Logger, LoggerConfig, Record};
//! let logger = Logger::start(LoggerConfig::to(LogTarget::Socket("/run/ultar/log/ingest.sock".into())));
//! logger.emit(Record::new(Level::Info, "service.ready").count("startup_ms", 12));
//! ```
//!
//! # Guarantees
//!
//! * **Never blocks, never fails the caller.** [`Logger::emit`] takes a
//!   short lock, appends to a bounded queue and returns. A separate thread
//!   talks to the log service. While the log service is away the queue
//!   keeps the newest [`LoggerConfig::buffer_records`] records (bounded by
//!   count and by `buffer_records · (MAX_BODY + 2)` bytes) and the thread
//!   retries with back-off; the service itself is not affected.
//! * **Rate limited per service** by a token bucket. What it refuses is
//!   counted and reported by one `log.dropped` record with the fields
//!   `rate` and `buffer` as soon as records pass again. (The log service
//!   enforces its own limit per source as well; this one keeps a service
//!   from spending its time on logging.)
//! * **No personal data by construction**, see [`record`].
//! * **Bounded record size**: [`wire::MAX_BODY`] bytes.
//!
//! The service name is not sent: the log service takes it from the kernel
//! credentials of the connection.

pub mod limit;
pub mod record;
pub mod wire;

pub use limit::{Clock, MonotonicClock, RateLimit, TokenBucket};
pub use record::{InvalidId, Level, Record, ShortId, Value, MAX_FIELDS, MAX_KEY_LEN};
pub use wire::{OwnedRecord, OwnedValue, WireError};

use std::collections::VecDeque;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Message key of the marker that reports dropped records.
pub const DROPPED_KEY: &str = "log.dropped";

/// Where records go.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogTarget {
    /// Nowhere: records are counted and discarded (tools, `--once` runs,
    /// tests without a log service).
    Discard,
    /// The ingestion socket of the log service.
    Socket(PathBuf),
}

/// Settings of a logger.
#[derive(Clone, Debug)]
pub struct LoggerConfig {
    /// Where records go.
    pub target: LogTarget,
    /// Records below this level are discarded.
    pub min_level: Level,
    /// Rate limit of this service.
    pub rate: RateLimit,
    /// Records kept while the log service does not take them.
    pub buffer_records: usize,
    /// Longest time one write to the log service may take.
    pub write_timeout: Duration,
    /// First pause before the next connection attempt; doubles up to
    /// `retry_max`.
    pub retry_min: Duration,
    /// Longest pause between connection attempts.
    pub retry_max: Duration,
}

impl LoggerConfig {
    /// Standard settings for `target`.
    pub fn to(target: LogTarget) -> Self {
        Self {
            target,
            min_level: Level::Info,
            rate: RateLimit::STANDARD,
            buffer_records: 256,
            write_timeout: Duration::from_secs(2),
            retry_min: Duration::from_millis(100),
            retry_max: Duration::from_secs(5),
        }
    }
}

/// Counters of a logger.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct LogStats {
    /// Records accepted into the queue (or counted, for `Discard`).
    pub accepted: u64,
    /// Records below the minimum level.
    pub filtered: u64,
    /// Records refused by the rate limit.
    pub rate_dropped: u64,
    /// Records pushed out of the full queue.
    pub buffer_dropped: u64,
    /// Records the encoder refused (bad key, too many fields, too large).
    pub rejected: u64,
    /// `log.dropped` markers generated.
    pub markers: u64,
    /// Records and markers handed to the log service.
    pub sent: u64,
    /// Records currently waiting.
    pub queued: usize,
}

struct State {
    bucket: TokenBucket,
    queue: VecDeque<Vec<u8>>,
    stats: LogStats,
    /// Buffer drops not yet reported by a marker.
    unreported_buffer: u64,
    /// A frame is being written right now.
    writing: bool,
    closed: bool,
}

struct Inner {
    config: LoggerConfig,
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
    wake: Condvar,
    idle: Condvar,
}

/// Handle of the logger of a service. Cloning is cheap.
#[derive(Clone)]
pub struct Logger {
    inner: Arc<Inner>,
}

fn lock(inner: &Inner) -> MutexGuard<'_, State> {
    inner.state.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Logger {
    /// Starts a logger with the machine's monotonic clock.
    pub fn start(config: LoggerConfig) -> Self {
        Self::start_with_clock(config, Arc::new(MonotonicClock::new()))
    }

    /// A logger that discards everything (and counts it).
    pub fn discard() -> Self {
        Self::start(LoggerConfig::to(LogTarget::Discard))
    }

    /// Starts a logger with an injected clock (tests).
    pub fn start_with_clock(config: LoggerConfig, clock: Arc<dyn Clock>) -> Self {
        let now = clock.now_ms();
        let inner = Arc::new(Inner {
            state: Mutex::new(State {
                bucket: TokenBucket::new(config.rate, now),
                queue: VecDeque::new(),
                stats: LogStats::default(),
                unreported_buffer: 0,
                writing: false,
                closed: false,
            }),
            config,
            clock,
            wake: Condvar::new(),
            idle: Condvar::new(),
        });
        if let LogTarget::Socket(path) = &inner.config.target {
            let (worker, path) = (inner.clone(), path.clone());
            // Without the thread the queue simply fills and drops: the
            // service still runs.
            let _ = thread::Builder::new()
                .name("ultar-log".into())
                .stack_size(128 * 1024)
                .spawn(move || flush_loop(&worker, &path));
        }
        Self { inner }
    }

    /// Queues `record`. Never blocks on I/O and never fails; see the
    /// module documentation for what happens to records that cannot pass.
    pub fn emit(&self, record: Record) {
        let inner = &*self.inner;
        if record.level() < inner.config.min_level {
            lock(inner).stats.filtered += 1;
            return;
        }
        let frame = wire::encode_frame(&record);
        let now = inner.clock.now_ms();
        let mut state = lock(inner);
        let Ok(frame) = frame else {
            state.stats.rejected += 1;
            return;
        };
        if state.closed {
            return;
        }
        if !state.bucket.allow(now) {
            state.stats.rate_dropped += 1;
            return;
        }
        self.push_marker(&mut state);
        state.stats.accepted += 1;
        self.push(&mut state, frame);
        drop(state);
        inner.wake.notify_one();
    }

    /// Reports what was dropped since the last marker – but only while the
    /// queue has room for the marker and the record that follows it. In a
    /// flood against a full queue a marker per record would itself push
    /// records out; the counts simply keep adding up until there is room.
    fn push_marker(&self, state: &mut State) {
        let capacity = self.inner.config.buffer_records.max(1);
        let discard = self.inner.config.target == LogTarget::Discard;
        if state.bucket.dropped() == 0 && state.unreported_buffer == 0 {
            return;
        }
        if !discard && state.queue.len() + 2 > capacity {
            return;
        }
        let rate = state.bucket.take_dropped();
        let buffer = std::mem::take(&mut state.unreported_buffer);
        let marker = Record::new(Level::Warn, DROPPED_KEY)
            .count("rate", rate)
            .count("buffer", buffer);
        if let Ok(marker) = wire::encode_frame(&marker) {
            state.stats.markers += 1;
            self.push(state, marker);
        }
    }

    fn push(&self, state: &mut State, frame: Vec<u8>) {
        if self.inner.config.target == LogTarget::Discard {
            return;
        }
        while state.queue.len() >= self.inner.config.buffer_records.max(1) {
            state.queue.pop_front();
            state.stats.buffer_dropped += 1;
            state.unreported_buffer += 1;
        }
        state.queue.push_back(frame);
        state.stats.queued = state.queue.len();
    }

    /// Shorthand for a record without fields.
    pub fn log(&self, level: Level, key: &'static str) {
        self.emit(Record::new(level, key));
    }

    /// Current counters.
    pub fn stats(&self) -> LogStats {
        let state = lock(&self.inner);
        LogStats {
            queued: state.queue.len(),
            ..state.stats
        }
    }

    /// Waits at most `timeout` until every queued record was handed to the
    /// log service. Returns whether the queue is empty. Used at shutdown
    /// and in the panic hook; with `Discard` it returns at once.
    pub fn flush(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut state = lock(&self.inner);
        loop {
            if state.queue.is_empty() && !state.writing {
                return true;
            }
            let now = Instant::now();
            if now >= deadline || state.closed {
                return false;
            }
            state = match self.inner.idle.wait_timeout(state, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    /// Stops the logger thread; later records are discarded.
    pub fn close(&self) {
        lock(&self.inner).closed = true;
        self.inner.wake.notify_all();
        self.inner.idle.notify_all();
    }
}

/// Waits for the next frame; `None` when the logger is closed.
fn next_frame(inner: &Inner) -> Option<Vec<u8>> {
    let mut state = lock(inner);
    loop {
        if state.closed {
            return None;
        }
        if let Some(frame) = state.queue.pop_front() {
            state.writing = true;
            return Some(frame);
        }
        inner.idle.notify_all();
        state = inner
            .wake
            .wait(state)
            .unwrap_or_else(PoisonError::into_inner);
    }
}

/// Puts an unsent frame back at the front, unless the queue filled up in
/// the meantime (then it is the oldest record and is the one to drop).
fn put_back(inner: &Inner, frame: Vec<u8>) {
    let mut state = lock(inner);
    state.writing = false;
    if state.queue.len() >= inner.config.buffer_records.max(1) {
        state.stats.buffer_dropped += 1;
        state.unreported_buffer += 1;
    } else {
        state.queue.push_front(frame);
    }
}

/// Sleeps for `pause` or until the logger is closed; returns `false` when
/// it was closed.
fn pause(inner: &Inner, pause: Duration) -> bool {
    let deadline = Instant::now() + pause;
    let mut state = lock(inner);
    loop {
        if state.closed {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        state = match inner.wake.wait_timeout(state, deadline - now) {
            Ok((guard, _)) => guard,
            Err(poisoned) => poisoned.into_inner().0,
        };
    }
}

fn connect(inner: &Inner, path: &std::path::Path) -> Option<UnixStream> {
    let stream = UnixStream::connect(path).ok()?;
    stream
        .set_write_timeout(Some(inner.config.write_timeout))
        .ok()?;
    Some(stream)
}

/// Body of the logger thread: sends queued frames, reconnects with
/// back-off. It only wakes for records and, while the log service is away
/// **and** records wait, for the next connection attempt.
fn flush_loop(inner: &Inner, path: &std::path::Path) {
    let mut stream: Option<UnixStream> = None;
    let mut delay = inner.config.retry_min;
    while let Some(frame) = next_frame(inner) {
        if stream.is_none() {
            stream = connect(inner, path);
        }
        let sent = stream
            .as_mut()
            .is_some_and(|stream| stream.write_all(&frame).is_ok());
        if sent {
            delay = inner.config.retry_min;
            let mut state = lock(inner);
            state.writing = false;
            state.stats.sent += 1;
            continue;
        }
        // A partial frame may be on the wire: this connection is done.
        stream = None;
        put_back(inner, frame);
        if !pause(inner, delay) {
            return;
        }
        delay = delay.saturating_mul(2).min(inner.config.retry_max);
    }
}

#[cfg(test)]
mod tests;
