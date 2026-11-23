//! Checks a log directory: every link, every position, the chain across
//! segments and the head file.
//!
//! Detected: a changed record, a removed record (anywhere, including the
//! newest ones up to the last sync), an inserted, duplicated or reordered
//! record, a removed segment in the middle or at the end, a segment from
//! another log.
//!
//! Not detectable by design: removal of the *oldest* segments looks like
//! retention. The report states the first position so that it can be
//! compared with an earlier report. And without a chain key, someone who
//! can rewrite the files can also recompute the links (see
//! [`crate::chain`]).

use crate::chain::{links_equal, Chain, Link};
use crate::segment::{parse_segment_name, scan, Damage, Scan};
use crate::storage::Storage;
use crate::store::{head_of, HEAD_NAME};

/// One thing that is wrong.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Problem {
    /// A segment file could not be read.
    Unreadable {
        /// File name.
        name: String,
    },
    /// A segment is damaged at this byte offset.
    Segment {
        /// File name.
        name: String,
        /// What is wrong.
        damage: Damage,
        /// Byte offset in the file.
        offset: usize,
    },
    /// The file name and the header disagree about the first position.
    WrongName {
        /// File name.
        name: String,
    },
    /// The segment does not continue the chain of the one before it:
    /// records at the end of the previous segment or a whole segment in
    /// between were removed, or the segment is from another log.
    BrokenChain {
        /// File name of the segment that does not fit.
        name: String,
    },
    /// The segment starts at or before the last position of its
    /// predecessor.
    Overlap {
        /// File name.
        name: String,
    },
    /// The head file says the log reached `head_seq`, the files end at
    /// `disk_seq`: the newest records are missing.
    MissingTail {
        /// Position the head file names.
        head_seq: u64,
        /// Newest position found in the segments (0: none).
        disk_seq: u64,
    },
    /// The record at the head position has another link than the head
    /// file recorded.
    HeadMismatch,
    /// The head file exists but is not authentic (damaged, or written
    /// without the chain key).
    HeadInvalid,
    /// Segments exist but no head file: the head is written before the
    /// first segment, so it was removed, possibly together with the tail.
    HeadMissing,
}

impl Problem {
    /// Stable key for reports.
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Unreadable { .. } => "unreadable",
            Self::Segment { .. } => "segment_damaged",
            Self::WrongName { .. } => "wrong_name",
            Self::BrokenChain { .. } => "broken_chain",
            Self::Overlap { .. } => "overlap",
            Self::MissingTail { .. } => "missing_tail",
            Self::HeadMismatch => "head_mismatch",
            Self::HeadInvalid => "head_invalid",
            Self::HeadMissing => "head_missing",
        }
    }
}

/// Result of a verification.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Report {
    /// Segment files found.
    pub segments: usize,
    /// Intact records found.
    pub records: u64,
    /// Position of the oldest intact record.
    pub first_seq: Option<u64>,
    /// Position of the newest intact record.
    pub last_seq: Option<u64>,
    /// Ranges of positions that were never written to disk (declared by a
    /// segment header after a disk failure): `(first, last)`.
    pub declared_gaps: Vec<(u64, u64)>,
    /// The newest segment ends in a partly written record (a crash while
    /// writing; the service cuts it off at its next start).
    pub torn_tail: bool,
    /// Whether an authentic head file was found.
    pub head: Option<u64>,
    /// Everything that is wrong; empty for an intact log.
    pub problems: Vec<Problem>,
}

impl Report {
    /// Whether nothing is wrong.
    pub fn is_intact(&self) -> bool {
        self.problems.is_empty()
    }
}

/// State carried from one segment to the next.
struct Walk<'a> {
    chain: &'a Chain,
    /// Position the head file names, if it is authentic.
    head_seq: Option<u64>,
    /// Link and last position of the previous segment, when it was intact.
    previous: Option<(Link, u64)>,
    /// Link of the record at the head position, if seen.
    head_link: Option<Link>,
}

/// Adds the intact records of a segment to the report and remembers the
/// link of the record the head file names.
fn count_records(result: &Scan, head_seq: Option<u64>, walk: &mut Walk<'_>, report: &mut Report) {
    report.records += result.records.len() as u64;
    if let Some(first) = result.records.first() {
        report.first_seq.get_or_insert(first.seq);
    }
    if let Some(seq) = result.last_seq() {
        report.last_seq = Some(seq);
    }
    if let Some(index) = head_seq.and_then(|seq| result.records.iter().position(|r| r.seq == seq)) {
        walk.head_link = result.links.get(index).copied();
    }
}

fn check_segment(
    name: &str,
    base: u64,
    data: &[u8],
    is_last: bool,
    walk: &mut Walk<'_>,
    report: &mut Report,
) {
    let result = scan(walk.chain, data);
    let head_seq = walk.head_seq;
    let Some(header) = result.header else {
        report.problems.push(Problem::Segment {
            name: name.to_owned(),
            damage: Damage::BadHeader,
            offset: 0,
        });
        walk.previous = None;
        return;
    };
    if header.base_seq != base {
        report.problems.push(Problem::WrongName {
            name: name.to_owned(),
        });
    }
    if let Some((link, last)) = walk.previous {
        if !links_equal(&link, &header.prev_link) {
            report.problems.push(Problem::BrokenChain {
                name: name.to_owned(),
            });
        } else if header.base_seq <= last {
            report.problems.push(Problem::Overlap {
                name: name.to_owned(),
            });
        } else if header.base_seq > last + 1 {
            report.declared_gaps.push((last + 1, header.base_seq - 1));
        }
    }
    count_records(&result, head_seq, walk, report);
    match result.damage {
        None => {}
        Some((Damage::TornTail, _)) if is_last => report.torn_tail = true,
        Some((damage, offset)) => report.problems.push(Problem::Segment {
            name: name.to_owned(),
            damage,
            offset,
        }),
    }
    // The chain continues from the last intact record; after damage in the
    // middle the next segment cannot be judged against this one.
    let clean = result.damage.is_none() || (is_last && report.torn_tail);
    let last = result.last_seq().unwrap_or(header.base_seq.wrapping_sub(1));
    walk.previous = clean.then_some((result.last_link, last));
}

fn check_head(storage: &dyn Storage, chain: &Chain, walk: &Walk<'_>, report: &mut Report) {
    let exists = storage
        .list()
        .is_ok_and(|files| files.iter().any(|(name, _)| name == HEAD_NAME));
    let Some((head_seq, head_link)) = head_of(storage, chain) else {
        if exists {
            report.problems.push(Problem::HeadInvalid);
        } else if report.segments > 0 {
            report.problems.push(Problem::HeadMissing);
        }
        return;
    };
    report.head = Some(head_seq);
    let disk_seq = report.last_seq.unwrap_or(0);
    if head_seq > disk_seq {
        report
            .problems
            .push(Problem::MissingTail { head_seq, disk_seq });
        return;
    }
    match walk.head_link {
        Some(link) if !links_equal(&link, &head_link) => {
            report.problems.push(Problem::HeadMismatch)
        }
        // The head record itself was removed by retention or lies in a
        // damaged part that is reported already.
        _ => {}
    }
}

/// Verifies the log in `storage`. Segment files larger than
/// `max_segment_bytes` are reported as unreadable instead of being loaded.
pub fn verify(storage: &dyn Storage, chain: &Chain, max_segment_bytes: u64) -> Report {
    let mut report = Report::default();
    let mut names: Vec<(u64, String)> = storage
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(name, _)| parse_segment_name(&name).map(|base| (base, name)))
        .collect();
    names.sort();
    report.segments = names.len();
    let mut walk = Walk {
        chain,
        head_seq: head_of(storage, chain).map(|(seq, _)| seq),
        previous: None,
        head_link: None,
    };
    let count = names.len();
    for (index, (base, name)) in names.iter().enumerate() {
        match storage.read(name, max_segment_bytes) {
            Ok(data) => {
                let is_last = index + 1 == count;
                check_segment(name, *base, &data, is_last, &mut walk, &mut report);
            }
            Err(_) => {
                report
                    .problems
                    .push(Problem::Unreadable { name: name.clone() });
                walk.previous = None;
            }
        }
    }
    check_head(storage, chain, &walk, &mut report);
    report
}
