//! The persistent part of the log: segment files with a size cap,
//! rotation, retention by total size and by age, an fsync policy, and a
//! full or broken disk that never stops the service.
//!
//! # Bounds
//!
//! * One segment never exceeds [`StoreConfig::segment_bytes`].
//! * All segments together never exceed [`StoreConfig::total_bytes`]: the
//!   oldest segments are removed **before** a new one is started.
//! * Segments whose newest record is older than
//!   [`StoreConfig::max_age_ms`] are removed.
//!
//! # A disk that does not take data
//!
//! A failed write is undone (the partly written frame is cut off again)
//! and the store becomes *degraded*: records are counted as not persisted
//! and stay in memory only. Every [`StoreConfig::retry_every`] records the
//! store tries again, in a **new segment** whose header declares the gap.
//! Nothing here ever waits for the disk to recover.

use crate::chain::{Chain, Link, GENESIS, LINK_LEN};
use crate::record::Stored;
use crate::segment::{
    encode_frame, scan, seed, segment_name, Damage, Header, Scan, FRAME_OVERHEAD, HEADER_LEN,
};
use crate::storage::{SegmentFile, Storage};
use open::{read_head, scan_directory, Meta};
use std::collections::VecDeque;

mod open;
pub use open::head_of;

/// Name of the head file.
pub const HEAD_NAME: &str = "head";
/// First bytes of the head file.
pub(crate) const HEAD_MAGIC: &[u8; 8] = b"ULOGHEAD";
/// Length of the head file.
pub const HEAD_LEN: usize = 8 + 8 + LINK_LEN + LINK_LEN;

/// When data is forced to stable storage.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsyncPolicy {
    /// After every record (slow, loses nothing).
    Always,
    /// After this many records, at rotation, and whenever the service asks
    /// (it asks for every record of level `error` and above).
    Batch(u32),
    /// Only at rotation and when the service asks.
    OnRotate,
}

/// Limits of the store.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StoreConfig {
    /// Largest size of one segment file in bytes.
    pub segment_bytes: u64,
    /// Largest size of all segment files together.
    pub total_bytes: u64,
    /// Oldest age of records in milliseconds; 0 = no age limit.
    pub max_age_ms: u64,
    /// The fsync policy.
    pub fsync: FsyncPolicy,
    /// While degraded: try the disk again after this many records.
    pub retry_every: u32,
}

impl StoreConfig {
    /// 1 MiB segments, 16 MiB in total, 30 days, sync every 64 records.
    pub const STANDARD: StoreConfig = StoreConfig {
        segment_bytes: 1 << 20,
        total_bytes: 16 << 20,
        max_age_ms: 30 * 24 * 3600 * 1000,
        fsync: FsyncPolicy::Batch(64),
        retry_every: 64,
    };

    /// Smallest accepted segment size: a header and one largest record.
    pub const MIN_SEGMENT: u64 = 8 * 1024;

    /// Whether the limits make sense: a segment holds at least one record
    /// and the total holds at least two segments (one can then always be
    /// removed to make room).
    pub fn is_valid(&self) -> bool {
        self.segment_bytes >= Self::MIN_SEGMENT
            && self.total_bytes >= self.segment_bytes.saturating_mul(2)
            && self.retry_every >= 1
    }
}

/// What the store noticed while opening the directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Notice {
    /// The directory could not be read; the service runs without disk.
    Unreadable,
    /// A torn tail of this many bytes was cut off the newest segment.
    TailRecovered(u64),
    /// A segment is damaged (see `ulog verify`); it is kept as it is and
    /// a new segment is started.
    Damaged(Damage),
    /// The newest record on disk is older than the head file says: the
    /// tail of the log is missing.
    TailMissing,
}

/// Counters of the store.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct StoreStats {
    /// Records written to disk since start.
    pub persisted: u64,
    /// Records that did not reach the disk since start.
    pub not_persisted: u64,
    /// Segments started since start.
    pub rotations: u64,
    /// Segments removed by the size or age limit since start.
    pub removed_segments: u64,
    /// Syncs since start.
    pub syncs: u64,
}

struct Active {
    file: Box<dyn SegmentFile>,
    len: u64,
    /// Position the next record of this segment must have.
    next: u64,
    /// Link the next record of this segment continues: the segment's seed
    /// while it is empty, then the link of its last record.
    link: Link,
}

/// The segment files of the log.
pub struct SegmentStore {
    storage: Box<dyn Storage>,
    chain: Chain,
    config: StoreConfig,
    /// Oldest first; the last one is the active segment when `active` is set.
    segments: VecDeque<Meta>,
    active: Option<Active>,
    last_link: Link,
    last_seq: u64,
    unsynced: u32,
    /// Records lost since the store became degraded (0: healthy).
    degraded: u64,
    stats: StoreStats,
}

impl SegmentStore {
    /// Opens the directory. Never fails: what is wrong becomes a notice,
    /// and a directory that cannot be used at all leaves the store
    /// degraded from the start.
    pub fn open(
        storage: Box<dyn Storage>,
        chain: Chain,
        config: StoreConfig,
    ) -> (Self, Vec<Notice>) {
        let mut opened = scan_directory(storage.as_ref(), &chain, &config);
        let head = read_head(storage.as_ref(), &chain);
        let mut store = Self {
            storage,
            chain,
            config,
            segments: std::mem::take(&mut opened.segments),
            active: None,
            last_link: GENESIS,
            last_seq: head.map_or(0, |(seq, _)| seq),
            unsynced: 0,
            degraded: 0,
            stats: StoreStats::default(),
        };
        if opened.notices.contains(&Notice::Unreadable) {
            store.degraded = 1;
        }
        if let Some(last) = opened.last {
            store.adopt_last(&last, &mut opened.notices);
            let disk_last = last.last_seq().unwrap_or(0);
            if head.is_some_and(|(seq, _)| seq > disk_last) && last.damage.is_none() {
                opened.notices.push(Notice::TailMissing);
            }
            store.last_seq = store.last_seq.max(disk_last);
        }
        // A smaller limit than last time: make the directory fit now.
        while store.disk_bytes() > store.config.total_bytes && store.remove_oldest() {}
        (store, opened.notices)
    }

    /// Takes over the newest segment found at start: continue it when it
    /// is clean, cut a torn tail, leave a damaged one alone.
    fn adopt_last(&mut self, last: &Scan, notices: &mut Vec<Notice>) {
        self.last_link = last.last_link;
        let Some(meta) = self.segments.back_mut() else {
            return;
        };
        let valid = last.valid_len as u64;
        match last.damage {
            None => {}
            Some((Damage::TornTail, _)) => {
                let cut = self
                    .storage
                    .open_append(&meta.name)
                    .and_then(|mut file| file.truncate(valid).and_then(|()| file.sync()));
                if cut.is_err() {
                    // Cannot repair: leave the file, start a new segment.
                    notices.push(Notice::Damaged(Damage::TornTail));
                    return;
                }
                notices.push(Notice::TailRecovered(meta.bytes.saturating_sub(valid)));
                meta.bytes = valid;
            }
            Some((damage, _)) => {
                notices.push(Notice::Damaged(damage));
                return;
            }
        }
        // Continue the segment; `write` starts a new one by itself when
        // the next record does not follow directly.
        let next = last
            .last_seq()
            .map_or(meta.base_seq, |seq| seq.wrapping_add(1));
        if let Ok(file) = self.storage.open_append(&meta.name) {
            self.active = Some(Active {
                file,
                len: valid,
                next,
                link: last.append_link,
            });
        }
    }

    /// Position of the newest record known to the store (0: none).
    pub fn last_seq(&self) -> u64 {
        self.last_seq
    }

    /// Bytes of all segment files.
    pub fn disk_bytes(&self) -> u64 {
        self.segments.iter().map(|meta| meta.bytes).sum()
    }

    /// Number of segment files.
    pub fn segment_count(&self) -> usize {
        self.segments.len()
    }

    /// Whether records currently do not reach the disk.
    pub fn is_degraded(&self) -> bool {
        self.degraded > 0
    }

    /// Counters.
    pub fn stats(&self) -> StoreStats {
        self.stats
    }

    /// The limits in effect.
    pub fn config(&self) -> &StoreConfig {
        &self.config
    }

    fn remove_oldest(&mut self) -> bool {
        // The active segment (the last one) is never removed.
        if self.segments.len() <= usize::from(self.active.is_some()) {
            return false;
        }
        let Some(meta) = self.segments.pop_front() else {
            return false;
        };
        if self.storage.remove(&meta.name).is_err() {
            self.segments.push_front(meta);
            return false;
        }
        self.stats.removed_segments += 1;
        true
    }

    /// Removes segments that are too old at wall time `now_ms`.
    pub fn enforce_age(&mut self, now_ms: u64) {
        if self.config.max_age_ms == 0 {
            return;
        }
        let limit = now_ms.saturating_sub(self.config.max_age_ms);
        while self
            .segments
            .front()
            .is_some_and(|meta| meta.last_time < limit && meta.last_time > 0)
        {
            if !self.remove_oldest() {
                break;
            }
        }
    }

    /// Starts a new segment for a record at `seq`, after making room for
    /// a **whole** segment: the new one may grow to `segment_bytes`, and
    /// the total must hold even then.
    fn rotate(&mut self, seq: u64, now_ms: u64) -> std::io::Result<()> {
        self.sync();
        self.active = None;
        while self.disk_bytes() + self.config.segment_bytes > self.config.total_bytes {
            if !self.remove_oldest() {
                return Err(std::io::ErrorKind::StorageFull.into());
            }
        }
        let header = Header {
            base_seq: seq,
            created_ms: now_ms,
            prev_link: self.last_link,
        };
        let name = segment_name(seq);
        let mut file = self.storage.create(&name)?;
        if let Err(error) = file.append(&header.encode()) {
            drop(file);
            let _ = self.storage.remove(&name);
            return Err(error);
        }
        self.segments.push_back(Meta {
            name,
            base_seq: seq,
            bytes: HEADER_LEN as u64,
            last_time: 0,
        });
        self.active = Some(Active {
            file,
            len: HEADER_LEN as u64,
            next: seq,
            link: seed(&self.chain, &header),
        });
        self.stats.rotations += 1;
        Ok(())
    }

    fn write(&mut self, record: &Stored, now_ms: u64) -> std::io::Result<()> {
        let needed = Self::frame_len(record);
        let fits = self.active.as_ref().is_some_and(|active| {
            active.len + needed <= self.config.segment_bytes && record.seq == active.next
        });
        // A gap in the positions (records lost while degraded, or a first
        // record after a damaged segment) always starts a new segment,
        // whose header declares where it begins.
        if !fits {
            self.rotate(record.seq, now_ms)?;
        }
        let Some(active) = self.active.as_mut() else {
            return Err(std::io::ErrorKind::NotFound.into());
        };
        let (frame, link) = encode_frame(&self.chain, &active.link, record);
        if let Err(error) = active.file.append(&frame) {
            // Undo the partial frame; a file that cannot even be cut is
            // abandoned (the next attempt starts a new segment).
            if active.file.truncate(active.len).is_err() {
                self.active = None;
            }
            return Err(error);
        }
        active.len += needed;
        active.next = record.seq.wrapping_add(1);
        active.link = link;
        if let Some(meta) = self.segments.back_mut() {
            meta.bytes = active.len;
            meta.last_time = record.time_ms;
        }
        self.last_link = link;
        Ok(())
    }

    /// Stores `record`; returns whether it reached the disk. Positions
    /// must increase.
    pub fn append(&mut self, record: &Stored, now_ms: u64) -> bool {
        let retry_every = u64::from(self.config.retry_every);
        if self.degraded > 0 && !self.degraded.is_multiple_of(retry_every) {
            self.degraded += 1;
            self.stats.not_persisted += 1;
            self.last_seq = record.seq;
            return false;
        }
        let written = self.write(record, now_ms).is_ok();
        self.last_seq = record.seq;
        if !written {
            self.degraded += 1;
            self.stats.not_persisted += 1;
            return false;
        }
        self.degraded = 0;
        self.stats.persisted += 1;
        self.unsynced += 1;
        let due = match self.config.fsync {
            FsyncPolicy::Always => true,
            FsyncPolicy::Batch(count) => self.unsynced >= count.max(1),
            FsyncPolicy::OnRotate => false,
        };
        if due {
            self.sync();
        }
        true
    }

    /// Forces everything written so far to stable storage and records the
    /// position in the head file. Failures are ignored here: the next
    /// write will notice a disk that is gone.
    pub fn sync(&mut self) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if self.unsynced == 0 {
            return;
        }
        if active.file.sync().is_ok() {
            self.stats.syncs += 1;
            self.unsynced = 0;
            let mut head = Vec::with_capacity(HEAD_LEN);
            head.extend_from_slice(HEAD_MAGIC);
            head.extend_from_slice(&self.last_seq.to_le_bytes());
            head.extend_from_slice(&self.last_link);
            let auth = self.chain.link(&GENESIS, &head);
            head.extend_from_slice(&auth);
            let _ = self.storage.write_atomic(HEAD_NAME, &head);
        }
    }

    /// Reads intact records with a position above `after_seq` from disk,
    /// oldest first, at most `limit`, for which `keep` says yes.
    pub fn read_after(
        &self,
        after_seq: u64,
        limit: usize,
        mut keep: impl FnMut(&Stored) -> bool,
    ) -> Vec<Stored> {
        let mut found = Vec::new();
        let count = self.segments.len();
        for (index, meta) in self.segments.iter().enumerate() {
            // Skip segments that end before the wanted position.
            let next_base = self.segments.get(index + 1).map(|next| next.base_seq);
            if index + 1 < count
                && next_base.is_some_and(|base| base <= after_seq.saturating_add(1))
            {
                continue;
            }
            let limit_bytes = self.config.segment_bytes.saturating_add(4096);
            let Ok(data) = self.storage.read(&meta.name, limit_bytes) else {
                continue;
            };
            for record in scan(&self.chain, &data).records {
                if record.seq > after_seq && keep(&record) {
                    found.push(record);
                    if found.len() >= limit {
                        return found;
                    }
                }
            }
        }
        found
    }

    /// Bytes one record adds to a segment (for size estimates in tests).
    pub fn frame_len(record: &Stored) -> u64 {
        (record.encode().len() + FRAME_OVERHEAD) as u64
    }
}
