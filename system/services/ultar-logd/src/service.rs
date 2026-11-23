//! The engine thread: owns the [`Core`], takes deliveries from the
//! ingestion side and requests from the bus, and answers queries
//! according to the policy.
//!
//! One thread, one queue, arrival order. Without work the thread blocks;
//! it only uses a timeout while a reader is following (waiting for the
//! next record with a deadline).

use crate::clock::Clock;
use crate::core::{Core, Scope};
use crate::ingest::{Delivery, Sink};
use crate::kernel::{read_snapshot, KernelTail};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::log::{Query, QueryReply, Stats, MAX_WAIT_MS, READ_ALL, SERVICE};
use ultar_bus::{BusError, ErrorCode, Identity, IncomingRequest, Policy};
use ultar_service::log::{Level, OwnedValue};

/// Readers that may wait for new records at the same time.
pub const MAX_FOLLOWERS: usize = 16;
/// Shortest time between two reads of the kernel log.
const KERNEL_INTERVAL_MS: u64 = 1_000;
/// Rejected peers are reported at most once per this many (the report
/// carries the count).
const REJECT_REPORT_EVERY: u64 = 64;

/// What a caller may read.
pub trait Access: Send {
    /// The scope of `identity`.
    fn scope(&self, identity: &str) -> Scope;
}

/// Scopes from the bus policy: `log.read_all` shows everything, everybody
/// else sees the records written under the own identity.
#[derive(Clone, Debug)]
pub struct PolicyAccess(pub Policy);

impl Access for PolicyAccess {
    fn scope(&self, identity: &str) -> Scope {
        let all = Identity::new(identity)
            .is_ok_and(|identity| self.0.may_call(&identity, SERVICE, READ_ALL));
        if all {
            Scope::All
        } else {
            Scope::Only(identity.to_owned())
        }
    }
}

/// Everybody reads everything (development only).
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowAll;

impl Access for AllowAll {
    fn scope(&self, _identity: &str) -> Scope {
        Scope::All
    }
}

/// Everything the engine thread reacts to.
pub enum Input {
    /// From the ingestion side.
    Delivery(Delivery),
    /// A bus request to the service `log`.
    Request(Box<IncomingRequest>),
    /// Read the kernel log now (start, reload).
    ReadKernel,
    /// Force the files to stable storage and answer when done.
    Sync(SyncSender<()>),
    /// A record of the daemon itself.
    Internal(Level, &'static str),
    /// A policy read again after `SIGHUP`: replaces who reads what.
    Access(Box<dyn Access>),
    /// End the thread.
    Stop,
}

/// Feeds the ingestion side into the engine's queue without blocking.
#[derive(Clone)]
pub struct InputSink(pub SyncSender<Input>);

impl Sink for InputSink {
    fn offer(&self, delivery: Delivery) -> Result<bool, ()> {
        match self.0.try_send(Input::Delivery(delivery)) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err(()),
        }
    }

    fn deliver(&self, delivery: Delivery) -> Result<(), ()> {
        self.0.send(Input::Delivery(delivery)).map_err(|_| ())
    }
}

struct Follower {
    request: IncomingRequest,
    query: Query,
    scope: Scope,
    deadline_ms: u64,
}

struct KernelSource {
    path: PathBuf,
    tail: KernelTail,
    last_read_ms: Option<u64>,
}

/// The engine: the core plus everything around it that needs its thread.
pub struct Engine {
    core: Core,
    access: Box<dyn Access>,
    clock: Arc<dyn Clock>,
    kernel: Option<KernelSource>,
    followers: Vec<Follower>,
    rejected: u64,
}

fn decode<C: ultar_bus::Call>(request: &IncomingRequest) -> Result<C, ErrorCode> {
    request.decode::<C>().map_err(|error| match error {
        BusError::Malformed => ErrorCode::BadPayload,
        other => other.code(),
    })
}

impl Engine {
    /// An engine around `core`. `kernel_log` names the kernel's log file,
    /// if the kernel log is to be ingested.
    pub fn new(
        core: Core,
        access: Box<dyn Access>,
        clock: Arc<dyn Clock>,
        kernel_log: Option<PathBuf>,
    ) -> Self {
        Self {
            core,
            access,
            clock,
            kernel: kernel_log.map(|path| KernelSource {
                path,
                tail: KernelTail::new(),
                last_read_ms: None,
            }),
            followers: Vec::new(),
            rejected: 0,
        }
    }

    /// The core (for inspection in tests).
    pub fn core(&self) -> &Core {
        &self.core
    }

    fn read_kernel(&mut self, force: bool) {
        let now = self.clock.mono_ms();
        let Some(source) = self.kernel.as_mut() else {
            return;
        };
        let recent = source
            .last_read_ms
            .is_some_and(|last| now.saturating_sub(last) < KERNEL_INTERVAL_MS);
        if recent && !force {
            return;
        }
        source.last_read_ms = Some(now);
        if let Some(snapshot) = read_snapshot(&source.path) {
            let lines = source.tail.update(&snapshot);
            self.core.ingest_kernel(&lines);
        }
    }

    fn deliver(&mut self, delivery: Delivery) {
        match delivery {
            Delivery::Record {
                service, record, ..
            } => {
                self.core.ingest(&service, record);
            }
            Delivery::Counts {
                service, counts, ..
            } => self.core.note_counts(&service, counts),
            Delivery::Rejected { count, .. } => {
                let before = self.rejected;
                self.rejected += count;
                if before / REJECT_REPORT_EVERY != self.rejected / REJECT_REPORT_EVERY
                    || before == 0
                {
                    let fields = vec![("total".to_owned(), OwnedValue::Count(self.rejected))];
                    self.core.internal(Level::Warn, "log.peer_rejected", fields);
                }
            }
        }
    }

    fn handle_query(&mut self, request: IncomingRequest) {
        let query = match decode::<Query>(&request) {
            Ok(query) => query,
            Err(code) => {
                let _ = request.fail(code);
                return;
            }
        };
        let scope = self.access.scope(request.caller());
        if scope == Scope::All {
            self.read_kernel(false);
        }
        let reply = self.core.query(&query, &scope);
        let wait = query.wait_ms.min(MAX_WAIT_MS);
        if reply.records.is_empty() && wait > 0 && self.followers.len() < MAX_FOLLOWERS {
            // Continue from where this look ended, so that nothing is
            // examined twice.
            let query = Query {
                after_seq: reply.next_seq,
                ..query
            };
            self.followers.push(Follower {
                request,
                query,
                scope,
                deadline_ms: self.clock.mono_ms() + u64::from(wait),
            });
            return;
        }
        let _ = request.reply(&reply);
    }

    fn handle_request(&mut self, request: IncomingRequest) {
        match request.method() {
            "query" => self.handle_query(request),
            "stats" => match decode::<Stats>(&request) {
                Ok(_) => {
                    let scope = self.access.scope(request.caller());
                    let _ = request.reply(&self.core.stats(&scope));
                }
                Err(code) => {
                    let _ = request.fail(code);
                }
            },
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
            }
        }
    }

    /// Takes a policy read again; waiting readers get their new scope at
    /// once, so a revoked `log.read_all` ends with the reload.
    fn replace_access(&mut self, access: Box<dyn Access>) {
        self.access = access;
        for follower in &mut self.followers {
            follower.scope = self.access.scope(follower.request.caller());
        }
        self.core
            .internal(Level::Info, "log.policy_reloaded", Vec::new());
    }

    /// Answers followers that have a record now or whose time is over.
    fn serve_followers(&mut self) {
        let now = self.clock.mono_ms();
        let mut waiting = Vec::with_capacity(self.followers.len());
        for follower in self.followers.drain(..) {
            let reply = self.core.query(&follower.query, &follower.scope);
            if reply.records.is_empty() && now < follower.deadline_ms {
                waiting.push(follower);
            } else {
                let _ = follower.request.reply(&reply);
            }
        }
        self.followers = waiting;
    }

    /// Time until the next follower deadline, if anybody follows.
    fn next_wait(&self) -> Option<Duration> {
        let now = self.clock.mono_ms();
        self.followers
            .iter()
            .map(|follower| follower.deadline_ms.saturating_sub(now))
            .min()
            .map(|ms| Duration::from_millis(ms.max(1)))
    }

    /// Handles one input; returns `false` for [`Input::Stop`].
    pub fn handle(&mut self, input: Input) -> bool {
        match input {
            Input::Delivery(delivery) => self.deliver(delivery),
            Input::Request(request) => self.handle_request(*request),
            Input::ReadKernel => self.read_kernel(true),
            Input::Sync(done) => {
                self.core.sync();
                let _ = done.send(());
            }
            Input::Internal(level, key) => {
                self.core.internal(level, key, Vec::new());
            }
            Input::Access(access) => self.replace_access(access),
            Input::Stop => return false,
        }
        self.serve_followers();
        true
    }

    /// Runs until [`Input::Stop`] or until every sender is gone, then
    /// answers the waiting readers, syncs and returns the core.
    pub fn run(mut self, inputs: &Receiver<Input>) -> Core {
        self.read_kernel(true);
        loop {
            let input = match self.next_wait() {
                None => inputs.recv().ok(),
                Some(wait) => match inputs.recv_timeout(wait) {
                    Ok(input) => Some(input),
                    Err(RecvTimeoutError::Timeout) => {
                        self.serve_followers();
                        continue;
                    }
                    Err(RecvTimeoutError::Disconnected) => None,
                },
            };
            let Some(input) = input else { break };
            if !self.handle(input) {
                break;
            }
        }
        for follower in self.followers.drain(..) {
            let reply = QueryReply {
                next_seq: follower.query.after_seq,
                ..QueryReply::default()
            };
            let _ = follower.request.reply(&reply);
        }
        self.core.internal(Level::Info, "log.stopped", Vec::new());
        self.core.sync();
        self.core
    }
}
