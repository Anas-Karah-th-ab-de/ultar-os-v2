//! The ingestion socket: where services deliver their records.
//!
//! One thread accepts, one small thread per connection reads. Records go
//! into a bounded queue with `try_send`, and what does not fit is counted
//! and reported as dropped. Every source has a share of that queue
//! (`share.rs`): at its share a source's reader waits for its own records
//! to be handled, so a flooding source never takes the room another source
//! needs. Connections are bounded in total and per source, so a single
//! source cannot occupy the socket.
//!
//! The **name** of a source is never taken from what it sends. It is
//! either fixed per listener (the socket file's owner and mode decide who
//! reaches it) or the identity the kernel credentials of the connection
//! map to; a peer that cannot be identified is disconnected.

pub use crate::delivery::{Counts, Delivery, Sink};
use crate::share::Share;
use std::collections::BTreeMap;
use std::io::Read;
use std::net::Shutdown;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use ultar_service::log::output;
use ultar_service::log::wire::{decode_body, FrameReader};
use ultar_service::log::{Clock, MonotonicClock, RateLimit, TokenBucket};
use ultar_service::socket::{bind_listener_for, ensure_parent, BindError};
use ultar_service::UidMap;

/// Who the peers of a listener are.
#[derive(Clone)]
pub enum SourceNames {
    /// Every peer is this source.
    Fixed(String),
    /// Kernel credentials looked up in the map; unknown users are refused.
    Credentials(Arc<UidMap>),
    /// The output channel of `ultar-init` (see
    /// `ultar_service::log::output`): the peer is `system.init` (the
    /// socket file's owner and mode decide who reaches it), its frames are
    /// lines of output of the services it started.
    Output,
}

/// Source name of the peer of an output listener.
pub const OUTPUT_PEER: &str = "system.init";

/// One ingestion socket.
#[derive(Clone)]
pub struct ListenerSpec {
    /// Path of the socket file.
    pub path: PathBuf,
    /// Who its peers are.
    pub names: SourceNames,
    /// Permission bits of the socket file.
    pub mode: Option<u32>,
    /// Group the socket file is handed to before its mode is set.
    pub group: Option<u32>,
}

/// Bounds of the ingestion side.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IngestLimits {
    /// Connections over all listeners.
    pub max_connections: usize,
    /// Connections of one source.
    pub max_per_source: usize,
    /// Rate limit of one source, applied right at the socket so that a
    /// flood of one source never fills the queue all sources share.
    pub rate: RateLimit,
    /// Records of one source that may wait in the queue at the same time.
    pub share: usize,
}

impl IngestLimits {
    /// 128 connections, 4 per source, the standard rate, 32 waiting
    /// records per source.
    pub const STANDARD: IngestLimits = IngestLimits {
        max_connections: 128,
        max_per_source: 4,
        rate: RateLimit::STANDARD,
        share: 32,
    };

    /// Whether the shares of all sources there can be (at most one per
    /// connection) and the share of rejections fit a queue of `capacity`
    /// entries.
    pub const fn fits(&self, capacity: usize) -> bool {
        self.max_connections
            .saturating_add(1)
            .saturating_mul(self.share)
            <= capacity
    }
}

#[derive(Default)]
struct Table {
    per_source: BTreeMap<String, usize>,
    /// One bucket per connected source (removed with its last connection;
    /// a source that reconnects to refill its bucket gains nothing the
    /// core's own limit would not take away again).
    buckets: BTreeMap<String, TokenBucket>,
    /// Queue share of each connected source.
    shares: BTreeMap<String, Share>,
    total: usize,
    streams: BTreeMap<u64, UnixStream>,
    next: u64,
}

struct Shared {
    limits: IngestLimits,
    /// Share of the rejections of all listeners.
    rejections: Share,
    /// Rejections that found the rejection share full; reported with the
    /// next one.
    unreported_rejections: AtomicU64,
    clock: MonotonicClock,
    stopping: AtomicBool,
    table: Mutex<Table>,
}

impl Shared {
    fn table(&self) -> MutexGuard<'_, Table> {
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether one more record of `service` may enter the queue now.
    fn allow(&self, service: &str) -> bool {
        let now = self.clock.now_ms();
        let mut table = self.table();
        match table.buckets.get_mut(service) {
            Some(bucket) => bucket.allow(now),
            None => true,
        }
    }
}

/// Frees a connection slot when the reader ends.
struct Slot {
    shared: Arc<Shared>,
    service: String,
    id: u64,
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut table = self.shared.table();
        table.total = table.total.saturating_sub(1);
        table.streams.remove(&self.id);
        if let Some(count) = table.per_source.get_mut(&self.service) {
            *count -= 1;
            if *count == 0 {
                table.per_source.remove(&self.service);
                table.buckets.remove(&self.service);
                table.shares.remove(&self.service);
            }
        }
    }
}

/// Takes a slot for `service`, or `None` when a bound is reached. The slot
/// carries the source's queue share.
fn admit(shared: &Arc<Shared>, service: &str, stream: &UnixStream) -> Option<(Slot, Share)> {
    let mut table = shared.table();
    let own = table.per_source.get(service).copied().unwrap_or(0);
    if table.total >= shared.limits.max_connections || own >= shared.limits.max_per_source {
        return None;
    }
    let id = table.next;
    table.next += 1;
    table.total += 1;
    *table.per_source.entry(service.to_owned()).or_insert(0) += 1;
    let (rate, now) = (shared.limits.rate, shared.clock.now_ms());
    table
        .buckets
        .entry(service.to_owned())
        .or_insert_with(|| TokenBucket::new(rate, now));
    let share = table.shares.entry(service.to_owned()).or_default().clone();
    if let Ok(clone) = stream.try_clone() {
        table.streams.insert(id, clone);
    }
    let slot = Slot {
        shared: shared.clone(),
        service: service.to_owned(),
        id,
    };
    Some((slot, share))
}

type Queue = Arc<dyn Sink>;

fn offer(queue: &Queue, delivery: Delivery) -> Result<bool, ()> {
    queue.offer(delivery)
}

/// Reports the counts of a connection. Like a record, the report takes a
/// place in the source's share (waiting for it on this connection only), so
/// a flooding source's counts never fill the queue. `Err(())` when the
/// engine is gone or the service stops.
fn report(
    shared: &Shared,
    (service, share, _): Source<'_>,
    counts: &mut Counts,
    queue: &Queue,
) -> Result<(), ()> {
    if counts.is_empty() {
        return Ok(());
    }
    let stopping = || shared.stopping.load(Ordering::SeqCst);
    let ticket = share.take(shared.limits.share, stopping).ok_or(())?;
    queue.deliver(Delivery::Counts {
        service: service.to_owned(),
        counts: std::mem::take(counts),
        ticket,
    })
}

/// Reports a rejected peer without waiting: beyond the rejection share it
/// is only counted and named in the next report.
fn reject(shared: &Shared, queue: &Queue) {
    let Some(ticket) = shared.rejections.try_take(shared.limits.share) else {
        shared.unreported_rejections.fetch_add(1, Ordering::SeqCst);
        return;
    };
    let count = 1 + shared.unreported_rejections.swap(0, Ordering::SeqCst);
    if let Ok(false) = offer(queue, Delivery::Rejected { count, ticket }) {
        shared
            .unreported_rejections
            .fetch_add(count, Ordering::SeqCst);
    }
}

/// Turns one frame body into a delivery. `None`: counted, nothing to
/// deliver; `Some(None)`: the service stops while the source waits for its
/// share.
fn make_delivery(
    shared: &Shared,
    (service, share, output): Source<'_>,
    body: &[u8],
    backlog: &mut Counts,
) -> Option<Option<Delivery>> {
    let content = if output {
        output::decode_body(body).ok().map(Content::Line)
    } else {
        decode_body(body).ok().map(Content::Record)
    };
    let Some(content) = content else {
        backlog.malformed += 1;
        return None;
    };
    // Output lines are limited per service in the core (one peer forwards
    // for all services; a bucket here would let one service's lines take
    // the others' turn).
    if !output && !shared.allow(service) {
        backlog.rate += 1;
        return None;
    }
    let stopping = || shared.stopping.load(Ordering::SeqCst);
    let Some(ticket) = share.take(shared.limits.share, stopping) else {
        return Some(None);
    };
    Some(Some(match content {
        Content::Line(line) => Delivery::Output { line, ticket },
        Content::Record(record) => Delivery::Record {
            service: service.to_owned(),
            record,
            ticket,
        },
    }))
}

/// The peer of a connection: its source name, its share of the queue,
/// and whether it is the output channel of `ultar-init`.
type Source<'a> = (&'a str, &'a Share, bool);

/// What a frame carried.
enum Content {
    Record(ultar_service::log::OwnedRecord),
    Line(output::OutputLine),
}

/// Reads one connection until it ends.
fn read_connection(shared: &Shared, mut stream: UnixStream, source: Source<'_>, queue: &Queue) {
    let mut frames = FrameReader::new();
    let mut backlog = Counts::default();
    let mut buffer = [0u8; 4096];
    let mut gone = false;
    while !gone {
        let read = match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        let framing = frames.push(&buffer[..read], |body| {
            let Some(made) = make_delivery(shared, source, body, &mut backlog) else {
                return;
            };
            let Some(delivery) = made else {
                gone = true;
                return;
            };
            match offer(queue, delivery) {
                Ok(true) => {}
                Ok(false) => backlog.queue += 1,
                Err(()) => gone = true,
            }
        });
        if framing.is_err() {
            // The stream lost its frame boundaries: count it and hang up.
            backlog.malformed += 1;
            let _ = report(shared, source, &mut backlog, queue);
            break;
        }
        gone |= report(shared, source, &mut backlog, queue).is_err();
    }
    let _ = report(shared, source, &mut backlog, queue);
    let _ = stream.shutdown(Shutdown::Both);
}

fn serve_peer(shared: &Arc<Shared>, stream: UnixStream, names: &SourceNames, queue: &Queue) {
    let service = match names {
        SourceNames::Fixed(name) => Some(name.clone()),
        SourceNames::Credentials(map) => map
            .identify(&stream)
            .ok()
            .map(|(identity, _)| identity.as_str().to_owned()),
        SourceNames::Output => Some(OUTPUT_PEER.to_owned()),
    };
    let slot = service
        .as_deref()
        .and_then(|service| admit(shared, service, &stream));
    let (Some(service), Some((slot, share))) = (service, slot) else {
        reject(shared, queue);
        let _ = stream.shutdown(Shutdown::Both);
        return;
    };
    let queue = queue.clone();
    let output = matches!(names, SourceNames::Output);
    // If the thread cannot be created the closure is dropped: the slot is
    // freed and the connection closed.
    let _ = thread::Builder::new()
        .name("ultar-logd-conn".into())
        .stack_size(96 * 1024)
        .spawn(move || {
            read_connection(&slot.shared, stream, (&service, &share, output), &queue);
            drop(slot);
        });
}

fn accept_loop(shared: &Arc<Shared>, listener: &UnixListener, names: &SourceNames, queue: &Queue) {
    for incoming in listener.incoming() {
        if shared.stopping.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(stream) = incoming {
            serve_peer(shared, stream, names, queue);
        }
    }
}

/// The running ingestion side. Dropping it stops it.
pub struct Ingest {
    shared: Arc<Shared>,
    paths: Vec<PathBuf>,
    acceptors: Vec<thread::JoinHandle<()>>,
}

impl Ingest {
    /// Binds every listener and starts accepting. Records go to `queue`.
    pub fn start(
        listeners: &[ListenerSpec],
        limits: IngestLimits,
        queue: Arc<dyn Sink>,
    ) -> Result<Self, BindError> {
        let queue = &queue;
        let shared = Arc::new(Shared {
            limits,
            clock: MonotonicClock::new(),
            rejections: Share::default(),
            unreported_rejections: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            table: Mutex::new(Table::default()),
        });
        let mut ingest = Self {
            shared,
            paths: Vec::new(),
            acceptors: Vec::new(),
        };
        for spec in listeners {
            // On failure `ingest` is dropped, which stops what already runs.
            ensure_parent(&spec.path, 0o755).map_err(BindError::Io)?;
            let listener = bind_listener_for(&spec.path, spec.mode, spec.group)?;
            ingest.paths.push(spec.path.clone());
            let (shared, names, queue) = (ingest.shared.clone(), spec.names.clone(), queue.clone());
            let acceptor = thread::Builder::new()
                .name("ultar-logd-accept".into())
                .spawn(move || accept_loop(&shared, &listener, &names, &queue))
                .map_err(BindError::Io)?;
            ingest.acceptors.push(acceptor);
        }
        Ok(ingest)
    }

    /// Socket paths served.
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    /// Connections currently open.
    pub fn connections(&self) -> usize {
        self.shared.table().total
    }

    fn stop(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        for path in &self.paths {
            // Wakes the acceptor out of its blocking `accept`.
            let _ = UnixStream::connect(path);
        }
        for acceptor in self.acceptors.drain(..) {
            let _ = acceptor.join();
        }
        for stream in self.shared.table().streams.values() {
            let _ = stream.shutdown(Shutdown::Both);
        }
        for path in self.paths.drain(..) {
            remove_socket(&path);
        }
    }
}

/// Removes the socket file at `path` – only if it is a socket.
fn remove_socket(path: &Path) {
    use std::os::unix::fs::FileTypeExt;
    if std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket()) {
        let _ = std::fs::remove_file(path);
    }
}

impl Drop for Ingest {
    fn drop(&mut self) {
        self.stop();
    }
}
