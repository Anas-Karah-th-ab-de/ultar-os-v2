//! Settings, notices and counters of the segment store.

use crate::chain::LINK_LEN;
use crate::segment::Damage;

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
    /// Segments exist but the head file does not: it was removed (with the
    /// newest segments, perhaps).
    HeadMissing,
    /// The head file is damaged or was not written with this chain key.
    HeadInvalid,
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
    /// Records carried forward out of evicted segments since start.
    pub carried: u64,
}
