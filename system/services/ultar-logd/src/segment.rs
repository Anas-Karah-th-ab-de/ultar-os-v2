//! The segment file format and how a segment is read back.
//!
//! ```text
//! segment := header frame*
//! header  := "ULOGSEG1" base_seq:u64le created_ms:u64le prev_link:[32]
//! frame   := len:u32le payload:[len] link:[32]
//! link    := chain.link(previous link, len || payload)
//! seed    := chain.link(prev_link, first 24 bytes of the header)
//! ```
//!
//! The first record of a segment links to the segment's `seed`, every
//! later record to its predecessor. The seed ties the header (magic,
//! first position, creation time) into the chain: changing any header
//! byte breaks the first link.
//!
//! `prev_link` is the link of the last record persisted before this
//! segment, so the chain runs through all segments. `base_seq` is the
//! position of the first record; if it is more than one past the last
//! record of the previous segment, the records in between were never
//! persisted (disk full) – a *declared gap*, not tampering.

use crate::chain::{links_equal, Chain, Link, LINK_LEN};
use crate::record::{Stored, MAX_PAYLOAD};

/// First bytes of every segment file.
pub const MAGIC: &[u8; 8] = b"ULOGSEG1";
/// Length of the segment header.
pub const HEADER_LEN: usize = 8 + 8 + 8 + LINK_LEN;
/// Bytes a frame adds to its payload.
pub const FRAME_OVERHEAD: usize = 4 + LINK_LEN;
/// Smallest possible payload (fixed part of a record).
const MIN_PAYLOAD: usize = 8 + 8 + 1 + 1 + 1 + 1 + 1 + 2;

/// The head of a segment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Header {
    /// Position of the first record of the segment.
    pub base_seq: u64,
    /// Creation time, milliseconds since 1970.
    pub created_ms: u64,
    /// Link of the last record persisted before this segment.
    pub prev_link: Link,
}

impl Header {
    /// The header bytes.
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..8].copy_from_slice(MAGIC);
        out[8..16].copy_from_slice(&self.base_seq.to_le_bytes());
        out[16..24].copy_from_slice(&self.created_ms.to_le_bytes());
        out[24..].copy_from_slice(&self.prev_link);
        out
    }

    /// Parses a header; `None` if the bytes are not one.
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() < HEADER_LEN || &bytes[..8] != MAGIC {
            return None;
        }
        let number = |range: std::ops::Range<usize>| {
            let mut raw = [0u8; 8];
            raw.copy_from_slice(&bytes[range]);
            u64::from_le_bytes(raw)
        };
        let mut prev_link = [0u8; LINK_LEN];
        prev_link.copy_from_slice(&bytes[24..HEADER_LEN]);
        Some(Self {
            base_seq: number(8..16),
            created_ms: number(16..24),
            prev_link,
        })
    }
}

/// The link the first record of a segment with this header continues.
pub fn seed(chain: &Chain, header: &Header) -> Link {
    chain.link(&header.prev_link, &header.encode()[..HEADER_LEN - LINK_LEN])
}

/// File name of the segment that starts at `base_seq`.
pub fn segment_name(base_seq: u64) -> String {
    format!("seg-{base_seq:020}.ulog")
}

/// The base position encoded in a segment file name.
pub fn parse_segment_name(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("seg-")?.strip_suffix(".ulog")?;
    if digits.len() != 20 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Encodes one frame for `record` after `previous`; returns the bytes and
/// the record's link.
pub fn encode_frame(chain: &Chain, previous: &Link, record: &Stored) -> (Vec<u8>, Link) {
    let payload = record.encode();
    let mut frame = Vec::with_capacity(payload.len() + FRAME_OVERHEAD);
    frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    frame.extend_from_slice(&payload);
    let link = chain.link(previous, &frame);
    frame.extend_from_slice(&link);
    (frame, link)
}

/// What is wrong with a segment.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Damage {
    /// The file does not start with a segment header.
    BadHeader,
    /// The file ends in the middle of a frame (or in zero bytes): the
    /// usual result of a crash or power loss while writing.
    TornTail,
    /// A frame length outside the possible range.
    BadLength,
    /// A record's link does not match: the record or one before it was
    /// changed, removed or inserted.
    BadLink,
    /// A record with a valid link that cannot be decoded.
    BadRecord,
    /// A record's position is not the one after its predecessor.
    BadSequence,
}

impl Damage {
    /// Stable key for reports.
    pub const fn key(self) -> &'static str {
        match self {
            Self::BadHeader => "bad_header",
            Self::TornTail => "torn_tail",
            Self::BadLength => "bad_length",
            Self::BadLink => "bad_link",
            Self::BadRecord => "bad_record",
            Self::BadSequence => "bad_sequence",
        }
    }
}

/// Result of reading a segment.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Scan {
    /// The header, if the file has one.
    pub header: Option<Header>,
    /// The records that are intact, in order.
    pub records: Vec<Stored>,
    /// The link of each intact record (same order as `records`).
    pub links: Vec<Link>,
    /// Link of the last intact record; for a segment without records the
    /// header's `prev_link` (the link the *next* segment has to name).
    pub last_link: Link,
    /// Link the next record appended to this segment has to continue: the
    /// last record's link, or the segment's seed while it is empty.
    pub append_link: Link,
    /// Bytes of the file that are intact (header and intact frames).
    pub valid_len: usize,
    /// The first problem, with its byte offset; `None` for a clean file.
    pub damage: Option<(Damage, usize)>,
}

impl Scan {
    /// Position of the last intact record, if there is one.
    pub fn last_seq(&self) -> Option<u64> {
        self.records.last().map(|record| record.seq)
    }
}

/// How one frame at the start of `rest` reads.
enum Frame<'a> {
    Good(&'a [u8], Link, usize),
    Bad(Damage),
}

fn read_frame<'a>(chain: &Chain, previous: &Link, rest: &'a [u8]) -> Frame<'a> {
    if rest.len() < 4 {
        return Frame::Bad(Damage::TornTail);
    }
    let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
    if len == 0 && rest.iter().all(|byte| *byte == 0) {
        // Space the file system allocated but never filled.
        return Frame::Bad(Damage::TornTail);
    }
    if !(MIN_PAYLOAD..=MAX_PAYLOAD).contains(&len) {
        return Frame::Bad(Damage::BadLength);
    }
    let total = len + FRAME_OVERHEAD;
    if rest.len() < total {
        return Frame::Bad(Damage::TornTail);
    }
    let mut stored = [0u8; LINK_LEN];
    stored.copy_from_slice(&rest[4 + len..total]);
    let expected = chain.link(previous, &rest[..4 + len]);
    if !links_equal(&stored, &expected) {
        return Frame::Bad(Damage::BadLink);
    }
    Frame::Good(&rest[4..4 + len], stored, total)
}

/// Reads a segment and checks every link and position.
pub fn scan(chain: &Chain, bytes: &[u8]) -> Scan {
    let Some(header) = Header::decode(bytes) else {
        return Scan {
            header: None,
            records: Vec::new(),
            links: Vec::new(),
            last_link: [0u8; LINK_LEN],
            append_link: [0u8; LINK_LEN],
            valid_len: 0,
            damage: Some((Damage::BadHeader, 0)),
        };
    };
    let mut result = Scan {
        header: Some(header),
        records: Vec::new(),
        links: Vec::new(),
        last_link: header.prev_link,
        append_link: seed(chain, &header),
        valid_len: HEADER_LEN,
        damage: None,
    };
    let mut expected_seq = header.base_seq;
    while result.valid_len < bytes.len() {
        let offset = result.valid_len;
        let (payload, link, total) = match read_frame(chain, &result.append_link, &bytes[offset..])
        {
            Frame::Good(payload, link, total) => (payload, link, total),
            Frame::Bad(damage) => {
                result.damage = Some((damage, offset));
                break;
            }
        };
        let damage = match Stored::decode(payload) {
            Err(_) => Some(Damage::BadRecord),
            Ok(record) if record.seq != expected_seq => Some(Damage::BadSequence),
            Ok(record) => {
                result.records.push(record);
                None
            }
        };
        if let Some(damage) = damage {
            result.damage = Some((damage, offset));
            break;
        }
        expected_seq = expected_seq.wrapping_add(1);
        result.links.push(link);
        result.last_link = link;
        result.append_link = link;
        result.valid_len += total;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::GENESIS;
    use crate::testkit::sample;

    fn segment(chain: &Chain, base: u64, count: u64) -> (Vec<u8>, Link) {
        let header = Header {
            base_seq: base,
            created_ms: 5,
            prev_link: GENESIS,
        };
        let mut bytes = header.encode().to_vec();
        let mut link = seed(chain, &header);
        for seq in base..base + count {
            let (frame, next) = encode_frame(chain, &link, &sample(seq));
            bytes.extend_from_slice(&frame);
            link = next;
        }
        // An empty segment hands on the link it was given.
        (bytes, if count == 0 { GENESIS } else { link })
    }

    #[test]
    fn a_clean_segment_reads_back_completely() {
        let chain = Chain::unkeyed();
        let (bytes, link) = segment(&chain, 10, 25);
        let result = scan(&chain, &bytes);
        assert_eq!(result.damage, None);
        assert_eq!(result.records.len(), 25);
        assert_eq!(result.records[0], sample(10));
        assert_eq!(result.last_seq(), Some(34));
        assert_eq!(result.last_link, link);
        assert_eq!(result.valid_len, bytes.len());
        let empty = scan(&chain, &segment(&chain, 3, 0).0);
        assert_eq!(
            (empty.damage, empty.last_seq(), empty.valid_len),
            (None, None, HEADER_LEN)
        );
    }

    #[test]
    fn names_round_trip_and_sort_by_position() {
        assert_eq!(segment_name(42), "seg-00000000000000000042.ulog");
        assert_eq!(parse_segment_name(&segment_name(u64::MAX)), Some(u64::MAX));
        assert!(segment_name(9) < segment_name(10));
        for bad in [
            "head",
            "seg-1.ulog",
            "seg-0000000000000000004x.ulog",
            "seg-00000000000000000042.tmp",
        ] {
            assert_eq!(parse_segment_name(bad), None, "{bad}");
        }
        assert_eq!(Header::decode(b"short"), None);
        assert_eq!(Header::decode(&[0u8; HEADER_LEN]), None);
    }

    #[test]
    fn every_cut_is_a_torn_tail_and_keeps_the_complete_records() {
        let chain = Chain::unkeyed();
        let (bytes, _) = segment(&chain, 1, 4);
        let frame_len = (bytes.len() - HEADER_LEN) / 4;
        for cut in HEADER_LEN..bytes.len() {
            let result = scan(&chain, &bytes[..cut]);
            let complete = (cut - HEADER_LEN) / frame_len;
            assert_eq!(result.records.len(), complete, "cut {cut}");
            if (cut - HEADER_LEN).is_multiple_of(frame_len) {
                assert_eq!(result.damage, None);
            } else {
                assert_eq!(result.damage.unwrap().0, Damage::TornTail, "cut {cut}");
                assert_eq!(result.valid_len, HEADER_LEN + complete * frame_len);
            }
        }
        // A tail of zero bytes (allocated, never written) is torn too.
        let mut zeros = bytes.clone();
        zeros.extend_from_slice(&[0u8; 100]);
        let result = scan(&chain, &zeros);
        assert_eq!(result.damage, Some((Damage::TornTail, bytes.len())));
        assert_eq!(result.records.len(), 4);
    }

    #[test]
    fn every_single_bit_flip_in_a_record_is_detected() {
        let chain = Chain::unkeyed();
        let (bytes, _) = segment(&chain, 1, 3);
        for index in HEADER_LEN..bytes.len() {
            for bit in [0, 3, 7] {
                let mut changed = bytes.clone();
                changed[index] ^= 1 << bit;
                let result = scan(&chain, &changed);
                assert!(result.damage.is_some(), "byte {index} bit {bit}");
                assert!(result.records.len() < 3);
            }
        }
        // Every byte of the header is part of the chain as well: the
        // creation time and the previous link break the first link, the
        // magic and the first position are caught even earlier.
        for index in 0..HEADER_LEN {
            let mut changed = bytes.clone();
            changed[index] ^= 1;
            let result = scan(&chain, &changed);
            assert!(
                result.damage.is_some() && result.records.is_empty(),
                "header byte {index}"
            );
        }
        let mut changed = bytes.clone();
        changed[20] ^= 1; // creation time
        assert_eq!(scan(&chain, &changed).damage.unwrap().0, Damage::BadLink);
        changed = bytes.clone();
        changed[30] ^= 1; // previous link
        assert_eq!(scan(&chain, &changed).damage.unwrap().0, Damage::BadLink);
    }

    #[test]
    fn a_keyed_segment_is_unreadable_with_another_key() {
        let keyed = Chain::keyed(b"secret key");
        let (bytes, _) = segment(&keyed, 1, 2);
        assert_eq!(scan(&keyed, &bytes).damage, None);
        for other in [Chain::unkeyed(), Chain::keyed(b"other key")] {
            assert_eq!(
                scan(&other, &bytes).damage,
                Some((Damage::BadLink, HEADER_LEN))
            );
        }
    }

    #[test]
    fn a_wrong_position_with_a_valid_link_is_a_sequence_error() {
        let chain = Chain::unkeyed();
        let header = Header {
            base_seq: 5,
            created_ms: 0,
            prev_link: GENESIS,
        };
        let mut bytes = header.encode().to_vec();
        let (frame, _) = encode_frame(&chain, &seed(&chain, &header), &sample(9));
        bytes.extend_from_slice(&frame);
        assert_eq!(
            scan(&chain, &bytes).damage,
            Some((Damage::BadSequence, HEADER_LEN))
        );
        let keys: std::collections::BTreeSet<&str> = [
            Damage::BadHeader,
            Damage::TornTail,
            Damage::BadLength,
            Damage::BadLink,
            Damage::BadRecord,
            Damage::BadSequence,
        ]
        .iter()
        .map(|damage| damage.key())
        .collect();
        assert_eq!(keys.len(), 6);
    }
}
