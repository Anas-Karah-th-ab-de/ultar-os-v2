//! Queries that reach back to the disk, and a fuzz of the segment reader
//! (segment files are input from outside: the disk may be damaged or
//! edited).

mod common;

use common::{numbered, small_config, Rig};
use ultar_bus::messages::log::{Query, SourceKind, MAX_REPLY_RECORDS};
use ultar_logd::chain::{Chain, GENESIS};
use ultar_logd::core::Scope;
use ultar_logd::record::Stored;
use ultar_logd::segment::{encode_frame, scan, seed, Header, HEADER_LEN};
use ultar_service::log::{Level, OwnedValue};

fn all(rig: &Rig, mut query: Query, scope: &Scope) -> Vec<u64> {
    let mut seen = Vec::new();
    loop {
        let reply = rig.core.query(&query, scope);
        seen.extend(reply.records.iter().map(|record| record.seq));
        if !reply.more {
            return seen;
        }
        query.after_seq = reply.next_seq;
    }
}

#[test]
fn a_query_pages_through_disk_and_memory_without_gap_or_repeat() {
    let mut rig = Rig::new(small_config(), Chain::unkeyed());
    for n in 0..400 {
        rig.core
            .ingest(if n % 4 == 0 { "app.b" } else { "app.a" }, numbered(n));
    }
    // The ring holds 64 records; everything else comes from the segments
    // (the oldest of which retention has removed).
    let seen = all(&rig, Query::default(), &Scope::All);
    assert!(seen.len() > 64 && seen.len() < 400, "{}", seen.len());
    assert!(seen.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert_eq!(seen.last(), Some(&400));
    // One page never exceeds its limit.
    let page = rig.core.query(
        &Query {
            limit: 10,
            ..Query::default()
        },
        &Scope::All,
    );
    assert_eq!((page.records.len(), page.more), (10, true));
    assert_eq!(page.next_seq, page.records[9].seq);
    let huge = rig.core.query(
        &Query {
            limit: u32::MAX,
            ..Query::default()
        },
        &Scope::All,
    );
    assert!(huge.records.len() <= MAX_REPLY_RECORDS as usize);
    // The scope holds for records read from disk as well.
    let own = all(&rig, Query::default(), &Scope::Only("app.b".into()));
    assert!(!own.is_empty() && own.iter().all(|seq| (seq - 1) % 4 == 0));
    let reply = rig
        .core
        .query(&Query::default(), &Scope::Only("app.b".into()));
    assert!(reply
        .records
        .iter()
        .all(|r| r.service == "app.b" && r.source == SourceKind::Service));
    assert!(all(&rig, Query::default(), &Scope::Only("app.none".into())).is_empty());
    // After the newest record there is nothing, and the answer says where
    // to continue.
    let end = rig.core.query(
        &Query {
            after_seq: 400,
            ..Query::default()
        },
        &Scope::All,
    );
    assert!(end.records.is_empty() && !end.more);
    assert_eq!(end.next_seq, 400);
}

#[test]
fn time_and_level_filters_apply_to_both_sources() {
    let mut rig = Rig::new(small_config(), Chain::unkeyed());
    for n in 0..200 {
        rig.core.ingest("app.a", numbered(n));
        rig.clock.advance(1_000);
    }
    rig.core.internal(
        Level::Error,
        "test.problem",
        vec![("n".into(), OwnedValue::Count(1))],
    );
    let start = common::START_MS;
    let window = Query {
        since_ms: Some(start + 150_000),
        until_ms: Some(start + 160_000),
        ..Query::default()
    };
    let reply = rig.core.query(&window, &Scope::All);
    assert_eq!(reply.records.len(), 10);
    assert!(reply
        .records
        .iter()
        .all(|r| (start + 150_000..start + 160_000).contains(&r.time_ms)));
    let errors = Query {
        min_level: Level::Error.number(),
        ..Query::default()
    };
    let reply = rig.core.query(&errors, &Scope::All);
    assert_eq!(reply.records.len(), 1);
    assert_eq!(reply.records[0].key, "test.problem");
    assert_eq!(reply.records[0].source, SourceKind::Internal);
}

#[test]
fn kernel_lines_are_a_separate_source_and_their_text_is_for_administrators() {
    let mut rig = Rig::new(small_config(), Chain::unkeyed());
    rig.core
        .ingest_kernel(&["[kern] bereit".to_owned(), "tab\there".to_owned()]);
    rig.core.ingest("kernel", numbered(1)); // a service that calls itself "kernel"
    let admin = rig.core.query(&Query::default(), &Scope::All);
    let kernel: Vec<_> = admin
        .records
        .iter()
        .filter(|r| r.source == SourceKind::Kernel)
        .collect();
    assert_eq!(kernel.len(), 2);
    assert_eq!(kernel[1].text.as_deref(), Some("tab here"));
    // An identity named like the kernel source sees its own service
    // record, never the kernel's lines.
    let impostor = rig
        .core
        .query(&Query::default(), &Scope::Only("kernel".into()));
    assert_eq!(impostor.records.len(), 1);
    assert_eq!(impostor.records[0].source, SourceKind::Service);
    assert!(impostor.records[0].text.is_none());
}

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn segment(chain: &Chain, count: u64) -> Vec<u8> {
    let header = Header {
        base_seq: 1,
        created_ms: 0,
        prev_link: GENESIS,
    };
    let mut bytes = header.encode().to_vec();
    let mut link = seed(chain, &header);
    for seq in 1..=count {
        let record = Stored::from_owned(seq, seq, "app.fuzz", numbered(seq));
        let (frame, next) = encode_frame(chain, &link, &record);
        bytes.extend_from_slice(&frame);
        link = next;
    }
    bytes
}

#[test]
fn the_segment_reader_survives_arbitrary_and_mutated_files() {
    let chain = Chain::unkeyed();
    let clean = segment(&chain, 40);
    assert_eq!(scan(&chain, &clean).records.len(), 40);
    let mut rng = Rng(0x5EED_1001);
    for _ in 0..20_000 {
        let mut data = clean.clone();
        match rng.below(5) {
            0 => {
                let index = rng.below(data.len());
                data[index] ^= 1 << rng.below(8);
            }
            1 => data.truncate(rng.below(data.len())),
            2 => {
                let at = rng.below(data.len());
                let len = rng.below(64);
                let junk: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
                data.splice(at..at, junk);
            }
            3 => {
                let start = rng.below(data.len());
                let end = (start + rng.below(200)).min(data.len());
                data.drain(start..end);
            }
            _ => {
                // A length field that claims the rest of the address space.
                let at = HEADER_LEN + rng.below(data.len() - HEADER_LEN - 4);
                data[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            }
        }
        let result = scan(&chain, &data);
        // Whatever happened: no panic, what is returned is a clean prefix.
        assert!(result.valid_len <= data.len());
        assert!(result.records.len() <= 40);
        assert_eq!(result.records.len(), result.links.len());
        for (index, record) in result.records.iter().enumerate() {
            assert_eq!(record.seq, index as u64 + 1);
        }
        if data != clean {
            assert!(
                result.damage.is_some() || result.records.len() < 40 || data.len() < clean.len(),
                "an edit went unnoticed"
            );
        }
    }
    // Pure noise.
    for _ in 0..5_000 {
        let len = rng.below(600);
        let noise: Vec<u8> = (0..len).map(|_| rng.next() as u8).collect();
        let result = scan(&chain, &noise);
        assert!(result.records.is_empty());
    }
}
