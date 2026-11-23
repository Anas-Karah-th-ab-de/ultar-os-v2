//! The hash chain: `verify` detects every single-record change, removal,
//! insertion and reordering, removed segments and a cut-off tail – with
//! and without a chain key.

mod common;

use common::{numbered, small_config, Rig};
use ultar_logd::chain::Chain;
use ultar_logd::segment::{Damage, HEADER_LEN};
use ultar_logd::store::HEAD_NAME;
use ultar_logd::verify::{verify, Problem, Report};

const MAX: u64 = 1 << 20;

/// A log of `count` records, synced, with its segment names.
fn log(chain: Chain, count: u64) -> (Rig, Vec<String>) {
    let mut rig = Rig::new(small_config(), chain);
    for n in 0..count {
        rig.core.ingest("app.a", numbered(n));
    }
    rig.core.sync();
    let segments = rig.segments();
    (rig, segments)
}

fn check(rig: &Rig) -> Report {
    verify(&rig.disk, &rig.chain, MAX)
}

/// Byte ranges `(start, end)` of the frames of a segment file.
fn frames(data: &[u8]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut at = HEADER_LEN;
    while at + 4 <= data.len() {
        let len = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        let end = at + 4 + len + 32;
        ranges.push((at, end));
        at = end;
    }
    assert_eq!(at, data.len());
    ranges
}

fn chains() -> [Chain; 2] {
    [
        Chain::unkeyed(),
        Chain::keyed(b"0123456789abcdef0123456789abcdef"),
    ]
}

#[test]
fn an_untouched_log_verifies() {
    for chain in chains() {
        let (rig, segments) = log(chain, 300);
        assert!(segments.len() >= 3, "several segments are needed");
        let report = check(&rig);
        assert!(report.is_intact(), "{:?}", report.problems);
        assert_eq!(report.segments, segments.len());
        assert_eq!((report.first_seq, report.last_seq), (Some(1), Some(300)));
        assert_eq!((report.records, report.head), (300, Some(300)));
        assert!(!report.torn_tail && report.declared_gaps.is_empty());
    }
}

#[test]
fn changing_any_single_record_is_detected() {
    for chain in chains() {
        let (rig, segments) = log(chain, 120);
        let mut tried = 0;
        for name in &segments {
            let original = rig.disk.file(name).unwrap();
            for (start, end) in frames(&original) {
                // One byte in the middle of the record's payload.
                let mut changed = original.clone();
                changed[start + (end - start) / 3] ^= 0x01;
                rig.disk.put(name, changed);
                let report = check(&rig);
                assert!(!report.is_intact(), "{name} frame at {start}");
                tried += 1;
            }
            rig.disk.put(name, original);
        }
        assert_eq!(tried, 120);
        assert!(check(&rig).is_intact());
    }
}

#[test]
fn removing_any_single_record_is_detected() {
    for chain in chains() {
        let (rig, segments) = log(chain, 120);
        let mut tried = 0;
        for name in &segments {
            let original = rig.disk.file(name).unwrap();
            for (start, end) in frames(&original) {
                let mut cut = original[..start].to_vec();
                cut.extend_from_slice(&original[end..]);
                rig.disk.put(name, cut);
                let report = check(&rig);
                assert!(!report.is_intact(), "{name} frame at {start}");
                tried += 1;
            }
            rig.disk.put(name, original);
        }
        assert_eq!(tried, 120);
    }
}

#[test]
fn duplicating_or_swapping_records_is_detected() {
    let (rig, segments) = log(Chain::unkeyed(), 60);
    let name = &segments[0];
    let original = rig.disk.file(name).unwrap();
    let ranges = frames(&original);
    let (a, b) = (ranges[2], ranges[3]);

    let mut doubled = original[..a.1].to_vec();
    doubled.extend_from_slice(&original[a.0..a.1]);
    doubled.extend_from_slice(&original[a.1..]);
    rig.disk.put(name, doubled);
    assert!(!check(&rig).is_intact());

    let mut swapped = original[..a.0].to_vec();
    swapped.extend_from_slice(&original[b.0..b.1]);
    swapped.extend_from_slice(&original[a.0..a.1]);
    swapped.extend_from_slice(&original[b.1..]);
    rig.disk.put(name, swapped);
    assert!(!check(&rig).is_intact());
}

#[test]
fn a_cut_off_tail_and_a_removed_last_segment_are_detected_through_the_head() {
    for chain in chains() {
        let (rig, segments) = log(chain, 300);
        let last = segments.last().unwrap();
        let original = rig.disk.file(last).unwrap();
        let ranges = frames(&original);
        // Remove the newest record cleanly (a complete frame).
        rig.disk
            .put(last, original[..ranges.last().unwrap().0].to_vec());
        let report = check(&rig);
        assert_eq!(
            report.problems,
            vec![Problem::MissingTail {
                head_seq: 300,
                disk_seq: 299
            }]
        );
        // Remove the whole newest segment.
        let mut disk = rig.disk.clone();
        ultar_logd::storage::Storage::remove(&mut disk, last).unwrap();
        let report = check(&rig);
        assert!(matches!(
            report.problems[..],
            [Problem::MissingTail { head_seq: 300, .. }]
        ));
    }
}

#[test]
fn a_removed_middle_segment_breaks_the_chain() {
    let (rig, segments) = log(Chain::unkeyed(), 300);
    let mut disk = rig.disk.clone();
    ultar_logd::storage::Storage::remove(&mut disk, &segments[1]).unwrap();
    let report = check(&rig);
    assert_eq!(
        report.problems,
        vec![Problem::BrokenChain {
            name: segments[2].clone()
        }]
    );
}

#[test]
fn removing_the_oldest_segment_looks_like_retention_and_moves_the_first_position() {
    let (rig, segments) = log(Chain::unkeyed(), 300);
    let before = check(&rig).first_seq;
    let mut disk = rig.disk.clone();
    ultar_logd::storage::Storage::remove(&mut disk, &segments[0]).unwrap();
    let report = check(&rig);
    // Documented limit: not a problem, but visible in the report.
    assert!(report.is_intact());
    assert!(report.first_seq > before);
}

#[test]
fn a_recomputed_history_passes_without_key_and_fails_with_key() {
    // What an attacker with write access can do: build a new, shorter log
    // with correct links. Only the keyed chain exposes it.
    let forged_by = |attacker: Chain, verifier: Chain| -> bool {
        let mut forged = Rig::new(small_config(), attacker);
        for n in 0..50 {
            forged.core.ingest("app.a", numbered(n));
        }
        forged.core.sync();
        verify(&forged.disk, &verifier, MAX).is_intact()
    };
    assert!(forged_by(Chain::unkeyed(), Chain::unkeyed()));
    let key = Chain::keyed(b"only the log service knows this");
    assert!(!forged_by(Chain::unkeyed(), key.clone()));
    assert!(!forged_by(Chain::keyed(b"a guessed key"), key));
}

#[test]
fn a_damaged_or_forged_head_file_is_reported() {
    let key = Chain::keyed(b"head key");
    let (rig, _) = log(key, 50);
    let mut head = rig.disk.file(HEAD_NAME).unwrap();
    // Claim a later position without being able to authenticate it.
    head[8] = head[8].wrapping_add(1);
    rig.disk.put(HEAD_NAME, head);
    assert_eq!(check(&rig).problems, vec![Problem::HeadInvalid]);
    rig.disk.put(HEAD_NAME, b"garbage".to_vec());
    assert_eq!(check(&rig).problems, vec![Problem::HeadInvalid]);
}

#[test]
fn a_torn_tail_is_reported_as_such_and_repaired_at_the_next_start() {
    let (rig, segments) = log(Chain::unkeyed(), 100);
    let last = segments.last().unwrap().clone();
    let original = rig.disk.file(&last).unwrap();
    // A crash in the middle of writing record 101.
    let mut torn = original.clone();
    torn.extend_from_slice(&original[HEADER_LEN..HEADER_LEN + 20]);
    rig.disk.put(&last, torn);
    let report = check(&rig);
    assert!(report.is_intact() && report.torn_tail);
    let mut rig = rig.reopen();
    // The fragment is gone; what follows the intact part is the service's
    // own note about the repair (record 101).
    let repaired = rig.disk.file(&last).unwrap();
    assert!(repaired.starts_with(&original) && repaired.len() > original.len());
    assert_eq!(frames(&repaired).len(), frames(&original).len() + 1);
    assert_eq!(rig.core.ingest("app.a", numbered(0)), Some(102));
    rig.core.sync();
    let report = check(&rig);
    assert!(
        report.is_intact() && !report.torn_tail,
        "{:?}",
        report.problems
    );
    assert_eq!(report.last_seq, Some(102));
}

#[test]
fn damage_in_the_middle_is_kept_as_evidence_and_a_new_segment_is_started() {
    let (rig, segments) = log(Chain::unkeyed(), 100);
    let last = segments.last().unwrap().clone();
    let mut data = rig.disk.file(&last).unwrap();
    let first_frame = frames(&data)[0];
    data[first_frame.0 + 10] ^= 0xff;
    rig.disk.put(&last, data.clone());
    let mut rig = rig.reopen();
    // The damaged file is not touched, not truncated, not appended to.
    assert!(rig.disk.file(&last).unwrap() == data);
    let seq = rig.core.ingest("app.a", numbered(0)).unwrap();
    assert!(seq > 100, "positions are never reused: {seq}");
    rig.core.sync();
    let report = check(&rig);
    assert!(report.problems.iter().any(|problem| matches!(
        problem,
        Problem::Segment { name, damage: Damage::BadLink, .. } if *name == last
    )));
    assert_eq!(rig.segments().len(), segments.len() + 1);
}

#[test]
fn problem_keys_are_distinct() {
    let problems = [
        Problem::Unreadable {
            name: String::new(),
        },
        Problem::Segment {
            name: String::new(),
            damage: Damage::BadLink,
            offset: 0,
        },
        Problem::WrongName {
            name: String::new(),
        },
        Problem::BrokenChain {
            name: String::new(),
        },
        Problem::Overlap {
            name: String::new(),
        },
        Problem::MissingTail {
            head_seq: 1,
            disk_seq: 0,
        },
        Problem::HeadMismatch,
        Problem::HeadInvalid,
    ];
    let keys: std::collections::BTreeSet<&str> = problems.iter().map(Problem::key).collect();
    assert_eq!(keys.len(), problems.len());
}
