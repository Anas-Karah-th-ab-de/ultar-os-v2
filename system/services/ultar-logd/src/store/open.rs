//! Reading a log directory at start: which segments exist, how the newest
//! one ends, and what the head file says.

use crate::chain::{links_equal, Chain, Link, GENESIS, LINK_LEN};
use crate::record::Stored;
use crate::segment::{parse_segment_name, scan, Damage, Scan, FRAME_OVERHEAD};
use crate::storage::Storage;
use std::collections::{BTreeMap, VecDeque};

use super::{Notice, StoreConfig, HEAD_LEN, HEAD_MAGIC, HEAD_NAME};

/// What the store remembers about one segment file.
#[derive(Clone, Debug)]
pub(super) struct Meta {
    pub(super) name: String,
    pub(super) base_seq: u64,
    pub(super) bytes: u64,
    /// Newest record time in the segment (records carried forward keep
    /// their own, older time, so this is a maximum, not the last).
    pub(super) last_time: u64,
    /// Bytes of each source's records in the segment.
    pub(super) sources: BTreeMap<String, u64>,
}

/// Bytes `record` occupies in a segment.
pub(super) fn frame_bytes(record: &Stored) -> u64 {
    (record.encode().len() + FRAME_OVERHEAD) as u64
}

/// Bytes per source of `records`.
pub(super) fn source_bytes(records: &[Stored]) -> BTreeMap<String, u64> {
    let mut sources = BTreeMap::new();
    for record in records {
        *sources.entry(record.service.clone()).or_insert(0) += frame_bytes(record);
    }
    sources
}

/// What the directory says about the head file.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeadState {
    /// There is no head file.
    Missing,
    /// There is one, but it is damaged or not authentic.
    Invalid,
    /// Position and link of the last synced record.
    Valid(u64, Link),
}

/// Reads the head file.
pub fn head_state(storage: &dyn Storage, chain: &Chain) -> HeadState {
    let exists = storage
        .list()
        .is_ok_and(|files| files.iter().any(|(name, _)| name == HEAD_NAME));
    match (exists, read_head(storage, chain)) {
        (_, Some((seq, link))) => HeadState::Valid(seq, link),
        (true, None) => HeadState::Invalid,
        (false, None) => HeadState::Missing,
    }
}

/// Outcome of reading the directory at start.
pub(super) struct Opened {
    pub(super) segments: VecDeque<Meta>,
    pub(super) last: Option<Scan>,
    pub(super) notices: Vec<Notice>,
}

pub(super) fn read_head(storage: &dyn Storage, chain: &Chain) -> Option<(u64, Link)> {
    let bytes = storage.read(HEAD_NAME, HEAD_LEN as u64).ok()?;
    if bytes.len() != HEAD_LEN || &bytes[..8] != HEAD_MAGIC {
        return None;
    }
    let mut auth = [0u8; LINK_LEN];
    auth.copy_from_slice(&bytes[48..]);
    if !links_equal(&auth, &chain.link(&GENESIS, &bytes[..48])) {
        return None;
    }
    let mut seq = [0u8; 8];
    seq.copy_from_slice(&bytes[8..16]);
    let mut link = [0u8; LINK_LEN];
    link.copy_from_slice(&bytes[16..48]);
    Some((u64::from_le_bytes(seq), link))
}

/// The authenticated head: position and link of the last synced record.
pub fn head_of(storage: &dyn Storage, chain: &Chain) -> Option<(u64, Link)> {
    read_head(storage, chain)
}

pub(super) fn scan_directory(storage: &dyn Storage, chain: &Chain, config: &StoreConfig) -> Opened {
    let mut opened = Opened {
        segments: VecDeque::new(),
        last: None,
        notices: Vec::new(),
    };
    let Ok(files) = storage.list() else {
        opened.notices.push(Notice::Unreadable);
        return opened;
    };
    let mut names: Vec<(u64, String, u64)> = files
        .into_iter()
        .filter_map(|(name, size)| parse_segment_name(&name).map(|base| (base, name, size)))
        .collect();
    names.sort();
    let count = names.len();
    for (index, (base_seq, name, bytes)) in names.into_iter().enumerate() {
        // A file larger than a segment can be is not read into memory.
        let limit = config.segment_bytes.saturating_add(4096);
        let scanned = storage
            .read(&name, limit)
            .ok()
            .map(|data| scan(chain, &data));
        let records = scanned.as_ref().map_or(&[][..], |s| s.records.as_slice());
        let last_time = records
            .iter()
            .map(|record| record.time_ms)
            .max()
            .unwrap_or(0);
        opened.segments.push_back(Meta {
            name,
            base_seq,
            bytes,
            last_time,
            sources: source_bytes(records),
        });
        match scanned {
            Some(result) if index + 1 == count => opened.last = Some(result),
            Some(result) => {
                if let Some((damage, _)) = result.damage {
                    opened.notices.push(Notice::Damaged(damage));
                }
            }
            None => opened.notices.push(Notice::Damaged(Damage::BadHeader)),
        }
    }
    opened
}
