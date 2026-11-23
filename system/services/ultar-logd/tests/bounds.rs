//! The log never grows beyond its configured bounds – in memory and on
//! disk – whatever arrives, and the rate limit per source holds.

mod common;

use common::{numbered, small_config, Rig, START_MS};
use ultar_bus::messages::log::{FieldValue, Query};
use ultar_logd::chain::Chain;
use ultar_logd::core::{CoreConfig, Scope, DROPPED_KEY};
use ultar_logd::storage::{DirStorage, Storage};
use ultar_logd::store::{FsyncPolicy, StoreConfig, HEAD_LEN};
use ultar_logd::verify::verify;
use ultar_service::log::RateLimit;

#[test]
fn a_flood_of_a_million_records_stays_inside_every_bound() {
    // No rate limit at all: every one of the 1e6 records is stored, so the
    // bounds are held by rotation and retention alone.
    let config = CoreConfig {
        ring_records: 500,
        ring_bytes: 64 * 1024,
        store: StoreConfig {
            segment_bytes: 64 * 1024,
            total_bytes: 512 * 1024,
            max_age_ms: 0,
            fsync: FsyncPolicy::Batch(1024),
            retry_every: 64,
        },
        ..small_config()
    };
    let mut rig = Rig::new(config, Chain::unkeyed());
    for n in 0..1_000_000u64 {
        assert!(rig.core.ingest("app.flood", numbered(n)).is_some());
        if n % 4096 == 0 {
            // Checked all along the way, not only at the end.
            assert!(rig.disk.used() <= 512 * 1024 + HEAD_LEN as u64, "at {n}");
            assert!(rig.core.ring().len() <= 500);
            assert!(rig.core.ring().bytes() <= 64 * 1024);
            rig.clock.advance(1);
        }
    }
    // The high-water mark covers every moment of the run.
    assert!(rig.disk.high_water() <= 512 * 1024 + HEAD_LEN as u64);
    assert_eq!(rig.core.store().disk_bytes(), rig.segment_bytes());
    assert!(rig.core.store().disk_bytes() <= 512 * 1024);
    assert!(rig.core.store().stats().removed_segments > 100);
    assert_eq!(rig.core.store().stats().persisted, 1_000_000);
    assert_eq!(rig.core.last_seq(), 1_000_000);
    for name in rig.segments() {
        assert!(rig.disk.file(&name).unwrap().len() <= 64 * 1024, "{name}");
    }
    // What is left is the newest part, intact and in order.
    rig.core.sync();
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert_eq!(report.last_seq, Some(1_000_000));
    assert!(report.records > 1_000 && report.records < 20_000);
    assert_eq!(
        report.first_seq.unwrap() + report.records - 1,
        1_000_000,
        "no gap inside the kept part"
    );
}

#[test]
fn the_rate_limit_is_per_source_and_leaves_a_marker() {
    let config = CoreConfig {
        rate: RateLimit {
            burst: 10,
            per_second: 5,
        },
        ..small_config()
    };
    let mut rig = Rig::new(config, Chain::unkeyed());
    let stored = (0..1_000)
        .filter(|n| rig.core.ingest("app.noisy", numbered(*n)).is_some())
        .count();
    assert_eq!(stored, 10);
    // The noisy source does not take anything from a quiet one.
    assert!(rig.core.ingest("app.quiet", numbered(0)).is_some());
    // One second later five more pass, led by the marker.
    rig.clock.advance(1_000);
    let stored = (0..100)
        .filter(|n| rig.core.ingest("app.noisy", numbered(*n)).is_some())
        .count();
    assert_eq!(stored, 5);
    let all = rig.core.query(&Query::default(), &Scope::All);
    let markers: Vec<_> = all
        .records
        .iter()
        .filter(|r| r.key == DROPPED_KEY)
        .collect();
    assert_eq!(markers.len(), 1);
    assert_eq!(markers[0].service, "app.noisy");
    assert_eq!(
        markers[0].fields,
        vec![("server".to_owned(), FieldValue::Count(990))]
    );
    assert_eq!(markers[0].level, 3);
    // 10 + quiet + marker + 5.
    assert_eq!(all.records.len(), 17);
    let stats = rig.core.stats(&Scope::All);
    let noisy = stats
        .sources
        .iter()
        .find(|s| s.service == "app.noisy")
        .unwrap();
    assert_eq!((noisy.stored, noisy.dropped), (15, 990 + 95));
    // An ordinary reader gets no per-source table.
    assert!(rig
        .core
        .stats(&Scope::Only("app.quiet".into()))
        .sources
        .is_empty());
}

#[test]
fn more_sources_than_the_table_holds_share_one_bucket() {
    let config = CoreConfig {
        max_sources: 4,
        rate: RateLimit {
            burst: 2,
            per_second: 0,
        },
        ..small_config()
    };
    let mut rig = Rig::new(config, Chain::unkeyed());
    for index in 0..100 {
        let name = format!("app.s{index}");
        rig.core.ingest(&name, numbered(0));
        rig.core.ingest(&name, numbered(1));
        rig.core.ingest(&name, numbered(2));
    }
    let stats = rig.core.stats(&Scope::All);
    assert!(stats.sources.len() <= 5, "{}", stats.sources.len());
    // Four sources with two records each, and two from the shared bucket.
    assert_eq!(rig.core.last_seq(), 4 * 2 + 2);
}

#[test]
fn old_segments_leave_by_age() {
    let hour = 3_600_000;
    let config = CoreConfig {
        store: StoreConfig {
            max_age_ms: 24 * hour,
            total_bytes: 1 << 20,
            ..small_config().store
        },
        ..small_config()
    };
    let mut rig = Rig::new(config, Chain::unkeyed());
    for n in 0..2_000 {
        rig.core.ingest("app.a", numbered(n));
    }
    let before = rig.segments().len();
    assert!(before > 5);
    rig.clock.advance(25 * hour);
    // The age check runs every 256 records.
    for n in 0..600 {
        rig.core.ingest("app.a", numbered(n));
    }
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    // Everything older than a day is gone except the segment that was
    // still being written when time jumped.
    let oldest_kept = rig
        .core
        .query(
            &Query {
                after_seq: 0,
                limit: 1,
                ..Query::default()
            },
            &Scope::All,
        )
        .records[0]
        .time_ms;
    assert!(rig.core.store().stats().removed_segments as usize >= before - 1);
    assert!(oldest_kept >= START_MS);
    assert!(report.first_seq.unwrap() > 1_500);
}

#[test]
fn a_real_directory_obeys_the_same_bounds_and_survives_a_restart() {
    let dir = common::temp_dir("real");
    let chain = Chain::keyed(b"directory test key");
    let clock = std::sync::Arc::new(ultar_logd::clock::ManualClock::at(START_MS));
    let config = small_config();
    let open = || {
        ultar_logd::core::Core::open(
            config,
            Box::new(DirStorage::open(&dir).unwrap()),
            chain.clone(),
            clock.clone(),
        )
    };
    let mut core = open();
    for n in 0..5_000 {
        core.ingest("app.a", numbered(n));
    }
    let size = |dir: &std::path::Path| -> u64 {
        std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().metadata().unwrap().len())
            .sum()
    };
    assert!(size(&dir) <= 32 * 1024 + HEAD_LEN as u64, "{}", size(&dir));
    drop(core);
    let mut core = open();
    assert_eq!(core.last_seq(), 5_000);
    assert_eq!(core.ingest("app.a", numbered(0)), Some(5_001));
    core.sync();
    assert!(size(&dir) <= 32 * 1024 + HEAD_LEN as u64);
    let storage = DirStorage::open(&dir).unwrap();
    let report = verify(&storage, &chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert_eq!((report.last_seq, report.head), (Some(5_001), Some(5_001)));
    assert!(report.declared_gaps.is_empty());
    // The wrong key (or none) does not verify.
    assert!(!verify(&storage, &Chain::unkeyed(), 1 << 20).is_intact());
    assert!(storage.list().unwrap().len() >= 3);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn invalid_limits_are_recognised() {
    assert!(StoreConfig::STANDARD.is_valid());
    let base = StoreConfig::STANDARD;
    for bad in [
        StoreConfig {
            segment_bytes: 100,
            ..base
        },
        StoreConfig {
            total_bytes: base.segment_bytes,
            ..base
        },
        StoreConfig {
            retry_every: 0,
            ..base
        },
    ] {
        assert!(!bad.is_valid(), "{bad:?}");
    }
}
