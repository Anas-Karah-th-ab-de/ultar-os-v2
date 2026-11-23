//! The log service without threads and sockets: rate limit per source,
//! ring, store, queries. Everything here is deterministic – time comes
//! from an injected clock – so the bounds can be proven by tests.

use crate::chain::Chain;
use crate::clock::Clock;
use crate::record::{clean_text, SourceKind, Stored, INTERNAL_NAME, KERNEL_NAME};
use crate::ring::Ring;
use crate::storage::Storage;
use crate::store::{Notice, SegmentStore, StoreConfig};
use std::collections::BTreeMap;
use std::sync::Arc;
use ultar_bus::messages::log as contract;
use ultar_service::log::{Level, OwnedRecord, OwnedValue, RateLimit, TokenBucket};

/// Message key of the marker for records the rate limit refused.
pub const DROPPED_KEY: &str = "log.dropped";
/// Message key of a kernel line.
pub const KERNEL_KEY: &str = "kernel.line";
/// Bucket shared by sources beyond [`CoreConfig::max_sources`].
const OVERFLOW_SOURCE: &str = "(overflow)";
/// Age retention is checked every this many records.
const AGE_CHECK_EVERY: u64 = 256;

/// Limits of the log service.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreConfig {
    /// Records kept in memory.
    pub ring_records: usize,
    /// Approximate bytes kept in memory.
    pub ring_bytes: usize,
    /// Rate limit of every source.
    pub rate: RateLimit,
    /// Sources with an own rate limit and counters; further sources share
    /// one bucket.
    pub max_sources: usize,
    /// Limits of the files on disk.
    pub store: StoreConfig,
}

impl CoreConfig {
    /// 2048 records / 1 MiB in memory, 200 + 50/s per source, 256 sources,
    /// standard store limits.
    pub const STANDARD: CoreConfig = CoreConfig {
        ring_records: 2048,
        ring_bytes: 1 << 20,
        rate: RateLimit::STANDARD,
        max_sources: 256,
        store: StoreConfig::STANDARD,
    };
}

/// Whose records a reader may see.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Scope {
    /// Everything, including kernel lines (`log.read_all`).
    All,
    /// Only the records of the service with this identity.
    Only(String),
}

struct SourceState {
    bucket: TokenBucket,
    stored: u64,
    dropped: u64,
    malformed: u64,
    /// Drops reported from outside the core that no marker has named yet.
    unreported: u64,
}

/// The log service core.
pub struct Core {
    config: CoreConfig,
    clock: Arc<dyn Clock>,
    ring: Ring,
    store: SegmentStore,
    sources: BTreeMap<String, SourceState>,
    seq: u64,
    was_degraded: bool,
}

fn field(name: &str, value: OwnedValue) -> (String, OwnedValue) {
    (name.to_owned(), value)
}

fn notice_record(notice: &Notice) -> (Level, &'static str, Vec<(String, OwnedValue)>) {
    match notice {
        Notice::Unreadable => (Level::Error, "log.store_unreadable", Vec::new()),
        Notice::TailRecovered(bytes) => (
            Level::Notice,
            "log.tail_recovered",
            vec![field("bytes", OwnedValue::Count(*bytes))],
        ),
        Notice::Damaged(damage) => (
            Level::Error,
            "log.segment_damaged",
            vec![field("what", OwnedValue::Word(damage.key().to_owned()))],
        ),
        Notice::TailMissing => (Level::Error, "log.tail_missing", Vec::new()),
    }
}

impl Core {
    /// Opens the store and records what it noticed.
    pub fn open(
        config: CoreConfig,
        storage: Box<dyn Storage>,
        chain: Chain,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let (store, notices) = SegmentStore::open(storage, chain, config.store);
        let mut core = Self {
            ring: Ring::new(config.ring_records, config.ring_bytes),
            seq: store.last_seq(),
            was_degraded: false,
            store,
            sources: BTreeMap::new(),
            config,
            clock,
        };
        core.store.enforce_age(core.clock.wall_ms());
        for notice in &notices {
            let (level, key, fields) = notice_record(notice);
            core.internal(level, key, fields);
        }
        core
    }

    fn source(&mut self, name: &str) -> &mut SourceState {
        let known = self.sources.contains_key(name);
        let name = if known || self.sources.len() < self.config.max_sources {
            name
        } else {
            OVERFLOW_SOURCE
        };
        let (rate, now) = (self.config.rate, self.clock.mono_ms());
        self.sources
            .entry(name.to_owned())
            .or_insert_with(|| SourceState {
                bucket: TokenBucket::new(rate, now),
                stored: 0,
                dropped: 0,
                malformed: 0,
                unreported: 0,
            })
    }

    /// Puts a record into ring and store, bypassing the rate limit.
    fn commit(&mut self, mut record: Stored) -> u64 {
        self.seq += 1;
        record.seq = self.seq;
        record.time_ms = self.clock.wall_ms();
        let urgent = record.level >= Level::Error;
        let persisted = self.store.append(&record, record.time_ms);
        if urgent && persisted {
            self.store.sync();
        }
        self.ring.push(record);
        let seq = self.seq;
        if seq.is_multiple_of(AGE_CHECK_EVERY) {
            self.store.enforce_age(self.clock.wall_ms());
        }
        // May store a record of its own; `seq` is this record's position.
        self.note_disk_state();
        seq
    }

    /// Records the moment the disk stops or resumes taking data. The
    /// records about it are themselves only in memory while it is down.
    fn note_disk_state(&mut self) {
        let degraded = self.store.is_degraded();
        if degraded == self.was_degraded {
            return;
        }
        self.was_degraded = degraded;
        let lost = self.store.stats().not_persisted;
        if degraded {
            self.internal(Level::Error, "log.persist_failed", Vec::new());
        } else {
            let fields = vec![field("not_persisted", OwnedValue::Count(lost))];
            self.internal(Level::Notice, "log.persist_resumed", fields);
        }
    }

    /// Applies the rate limit of `name`, then commits. Returns the
    /// position, or `None` when the record was refused.
    fn limited(&mut self, name: &str, record: Stored) -> Option<u64> {
        let now = self.clock.mono_ms();
        let state = self.source(name);
        if !state.bucket.allow(now) {
            state.dropped += 1;
            return None;
        }
        let refused = state.bucket.take_dropped() + std::mem::take(&mut state.unreported);
        state.stored += 1;
        if refused > 0 {
            let marker = Stored {
                level: Level::Warn,
                key: DROPPED_KEY.to_owned(),
                fields: vec![field("server", OwnedValue::Count(refused))],
                text: String::new(),
                ..record.clone()
            };
            self.commit(marker);
        }
        Some(self.commit(record))
    }

    /// Stores a record of the service `service` (its verified identity).
    pub fn ingest(&mut self, service: &str, record: OwnedRecord) -> Option<u64> {
        self.limited(service, Stored::from_owned(0, 0, service, record))
    }

    /// Counts records of `service` that were dropped before they reached
    /// the core (rate limit at the socket, full queue); they show up in
    /// the next drop marker of that source.
    pub fn note_dropped(&mut self, service: &str, count: u64) {
        let state = self.source(service);
        state.dropped += count;
        state.unreported += count;
    }

    /// Counts a malformed record of `service`.
    pub fn note_malformed(&mut self, service: &str) {
        self.source(service).malformed += 1;
    }

    /// Stores lines of the kernel log as the separate source `kernel`.
    pub fn ingest_kernel(&mut self, lines: &[String]) {
        for line in lines {
            let record = Stored {
                seq: 0,
                time_ms: 0,
                kind: SourceKind::Kernel,
                service: KERNEL_NAME.to_owned(),
                level: Level::Info,
                key: KERNEL_KEY.to_owned(),
                fields: Vec::new(),
                text: clean_text(line),
            };
            self.limited(KERNEL_NAME, record);
        }
    }

    /// Stores a record of the log service itself.
    pub fn internal(&mut self, level: Level, key: &str, fields: Vec<(String, OwnedValue)>) -> u64 {
        self.commit(Stored {
            seq: 0,
            time_ms: 0,
            kind: SourceKind::Internal,
            service: INTERNAL_NAME.to_owned(),
            level,
            key: key.to_owned(),
            fields,
            text: String::new(),
        })
    }

    /// Position of the newest record.
    pub fn last_seq(&self) -> u64 {
        self.seq
    }

    /// The ring (for bound checks).
    pub fn ring(&self) -> &Ring {
        &self.ring
    }

    /// The store (for bound checks).
    pub fn store(&self) -> &SegmentStore {
        &self.store
    }

    /// Forces the files to stable storage.
    pub fn sync(&mut self) {
        self.store.sync();
    }

    /// Answers a query for a reader with `scope`.
    pub fn query(&self, query: &contract::Query, scope: &Scope) -> contract::QueryReply {
        let all = *scope == Scope::All;
        let limit = match query.limit {
            0 => contract::MAX_REPLY_RECORDS,
            wanted => wanted.min(contract::MAX_REPLY_RECORDS),
        } as usize;
        let matches = |record: &Stored| visible(record, scope) && wanted(record, query);
        let ring_first = self.ring.first_seq().unwrap_or(u64::MAX);
        let mut found: Vec<Stored> = Vec::new();
        if query.after_seq.saturating_add(1) < ring_first {
            // Older than memory: read from disk up to where the ring begins.
            found = self.store.read_after(query.after_seq, limit + 1, |record| {
                record.seq < ring_first && matches(record)
            });
        }
        for record in self.ring.after(query.after_seq) {
            if found.len() > limit {
                break;
            }
            if matches(record) {
                found.push(record.clone());
            }
        }
        let more = found.len() > limit;
        found.truncate(limit);
        let next_seq = match (more, found.last()) {
            (true, Some(last)) => last.seq,
            // Everything up to the newest record was examined.
            _ => self.seq.max(query.after_seq),
        };
        contract::QueryReply {
            records: found
                .into_iter()
                .map(|record| to_bus(record, all))
                .collect(),
            next_seq,
            more,
            all_sources: all,
        }
    }

    /// Counters; the per-source table only for readers who see everything.
    pub fn stats(&self, scope: &Scope) -> contract::StatsReply {
        let sources = match scope {
            Scope::All => self
                .sources
                .iter()
                .map(|(name, state)| contract::SourceStats {
                    service: name.clone(),
                    stored: state.stored,
                    dropped: state.dropped,
                    malformed: state.malformed,
                })
                .collect(),
            Scope::Only(_) => Vec::new(),
        };
        contract::StatsReply {
            last_seq: self.seq,
            ring_records: self.ring.len() as u64,
            disk_bytes: self.store.disk_bytes(),
            disk_limit: self.store.config().total_bytes,
            degraded: self.store.is_degraded(),
            not_persisted: self.store.stats().not_persisted,
            sources,
        }
    }
}

/// Whether a reader with `scope` may see `record`.
pub fn visible(record: &Stored, scope: &Scope) -> bool {
    match scope {
        Scope::All => true,
        Scope::Only(identity) => record.kind == SourceKind::Service && record.service == *identity,
    }
}

/// Whether `record` passes the filters of `query`.
pub fn wanted(record: &Stored, query: &contract::Query) -> bool {
    record.level.number() >= query.min_level
        && query
            .service
            .as_ref()
            .is_none_or(|name| record.service == *name)
        && query.since_ms.is_none_or(|since| record.time_ms >= since)
        && query.until_ms.is_none_or(|until| record.time_ms < until)
}

/// Converts a stored record for the bus. Kernel text is only included
/// for readers who see everything.
pub fn to_bus(record: Stored, with_text: bool) -> contract::LogRecord {
    let source = match record.kind {
        SourceKind::Service => contract::SourceKind::Service,
        SourceKind::Kernel => contract::SourceKind::Kernel,
        SourceKind::Internal => contract::SourceKind::Internal,
    };
    let text = (with_text && record.kind == SourceKind::Kernel).then_some(record.text);
    contract::LogRecord {
        seq: record.seq,
        time_ms: record.time_ms,
        source,
        service: record.service,
        level: record.level.number(),
        key: record.key,
        fields: record
            .fields
            .into_iter()
            .map(|(name, value)| {
                let value = match value {
                    OwnedValue::Count(n) => contract::FieldValue::Count(n),
                    OwnedValue::Signed(n) => contract::FieldValue::Signed(n),
                    OwnedValue::Flag(flag) => contract::FieldValue::Flag(flag),
                    OwnedValue::Word(word) => contract::FieldValue::Word(word),
                    OwnedValue::Id(id) => contract::FieldValue::Id(id),
                };
                (name, value)
            })
            .collect(),
        text,
    }
}
