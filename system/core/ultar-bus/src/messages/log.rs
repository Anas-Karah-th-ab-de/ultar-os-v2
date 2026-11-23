//! Contract of the log service: reading records over the bus.
//!
//! | Name | Kind | Who may |
//! |---|---|---|
//! | `log.query` | call | any identity the policy grants `log.query`; it sees **its own** records only |
//! | `log.read_all` | permission (never called) | an identity granted this call rule sees the records of all sources |
//! | `log.stats` | call | as `log.query`; the per-source table only with `log.read_all` |
//!
//! "Follow" is a query with `wait_ms > 0`: the service answers as soon as a
//! matching record newer than `after_seq` exists, or with an empty reply
//! when the time is over. The reader then asks again with `next_seq`.
//! Records never contain free text of services (see `ultar_service::log`);
//! kernel records carry the kernel's line and are only given to readers
//! with `log.read_all`.

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the log service on the bus.
pub const SERVICE: &str = "log";
/// Pseudo method: the permission to read the records of every source.
pub const READ_ALL: &str = "read_all";
/// Most records one reply carries.
pub const MAX_REPLY_RECORDS: u32 = 256;
/// Longest wait of a following query in milliseconds.
pub const MAX_WAIT_MS: u32 = 30_000;

/// Where a record came from.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum SourceKind {
    /// A service or app, named by its kernel-verified identity.
    Service,
    /// The kernel log.
    Kernel,
    /// The log service itself (drop markers, gaps, rotation).
    Internal,
}

/// The value of a record field.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum FieldValue {
    /// Unsigned quantity.
    Count(u64),
    /// Signed quantity or code.
    Signed(i64),
    /// Yes or no.
    Flag(bool),
    /// One word of the writing program's vocabulary.
    Word(String),
    /// A short technical identifier.
    Id(String),
}

/// One stored record.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct LogRecord {
    /// Position in the log; strictly increasing, never reused.
    pub seq: u64,
    /// Wall-clock time of arrival at the log service, milliseconds since
    /// 1970-01-01 UTC.
    pub time_ms: u64,
    /// Kind of source.
    pub source: SourceKind,
    /// Name of the source (the identity for services, `kernel`, `logd`).
    pub service: String,
    /// Severity, 0 (debug) to 5 (critical).
    pub level: u8,
    /// Message key.
    pub key: String,
    /// Typed fields.
    pub fields: Vec<(String, FieldValue)>,
    /// The kernel's own line; only for [`SourceKind::Kernel`].
    pub text: Option<String>,
}

/// `log.query` – read records.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct Query {
    /// Only this source; `None`: every source the caller may see.
    pub service: Option<String>,
    /// Lowest level, 0 (debug) to 5 (critical).
    pub min_level: u8,
    /// Only records at or after this time (milliseconds since 1970).
    pub since_ms: Option<u64>,
    /// Only records before this time.
    pub until_ms: Option<u64>,
    /// Only records after this position (0: from the oldest).
    pub after_seq: u64,
    /// Most records wanted; capped at [`MAX_REPLY_RECORDS`], 0 = the cap.
    pub limit: u32,
    /// Follow: wait up to this long for a first matching record; capped
    /// at [`MAX_WAIT_MS`].
    pub wait_ms: u32,
}

/// Answer to [`Query`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct QueryReply {
    /// Matching records, oldest first.
    pub records: Vec<LogRecord>,
    /// Position to continue from (`after_seq` of the next query).
    pub next_seq: u64,
    /// More matching records exist beyond `limit`.
    pub more: bool,
    /// The caller sees all sources (`log.read_all`).
    pub all_sources: bool,
}

impl Message for Query {
    const VERSION: u16 = 1;
}

impl Message for QueryReply {
    const VERSION: u16 = 1;
}

impl Call for Query {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "query";
    type Reply = QueryReply;
}

/// `log.stats` – state of the log service.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {}

/// Counters of one source.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct SourceStats {
    /// Name of the source.
    pub service: String,
    /// Records stored.
    pub stored: u64,
    /// Records refused by the rate limit or a full queue.
    pub dropped: u64,
    /// Records refused as malformed.
    pub malformed: u64,
}

/// Answer to [`Stats`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct StatsReply {
    /// Position of the newest record (0: none yet).
    pub last_seq: u64,
    /// Records in memory.
    pub ring_records: u64,
    /// Bytes of the segment files on disk.
    pub disk_bytes: u64,
    /// Configured upper bound of `disk_bytes`.
    pub disk_limit: u64,
    /// Whether records are currently not reaching the disk (disk full,
    /// I/O error); they are kept in memory only.
    pub degraded: bool,
    /// Records that never reached the disk since start.
    pub not_persisted: u64,
    /// Per source; only for callers with `log.read_all`.
    pub sources: Vec<SourceStats>,
}

impl Message for Stats {
    const VERSION: u16 = 1;
}

impl Message for StatsReply {
    const VERSION: u16 = 1;
}

impl Call for Stats {
    const SERVICE: &'static str = SERVICE;
    const METHOD: &'static str = "stats";
    type Reply = StatsReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode_versioned, encode};
    use crate::topic::{is_valid_name, is_valid_segment_name};

    #[test]
    fn names_are_valid_and_messages_round_trip() {
        assert!(is_valid_name(SERVICE));
        for method in [Query::METHOD, Stats::METHOD, READ_ALL] {
            assert!(is_valid_segment_name(method));
        }
        let reply = QueryReply {
            records: vec![LogRecord {
                seq: 7,
                time_ms: 1_800_000_000_000,
                source: SourceKind::Service,
                service: "system.devd".into(),
                level: 3,
                key: "devd.driver.failed".into(),
                fields: vec![
                    ("n".into(), FieldValue::Count(2)),
                    ("device".into(), FieldValue::Id("pci:01:00.0".into())),
                ],
                text: None,
            }],
            next_seq: 7,
            more: false,
            all_sources: true,
        };
        let bytes = encode(&reply).unwrap();
        let back: QueryReply = decode_versioned(QueryReply::VERSION, &bytes).unwrap();
        assert_eq!(back, reply);
        let query = Query {
            service: Some("app.phone".into()),
            wait_ms: 500,
            ..Query::default()
        };
        let back: Query = decode_versioned(1, &encode(&query).unwrap()).unwrap();
        assert_eq!(back, query);
        let stats: StatsReply =
            decode_versioned(1, &encode(&StatsReply::default()).unwrap()).unwrap();
        assert_eq!(stats, StatsReply::default());
    }
}
