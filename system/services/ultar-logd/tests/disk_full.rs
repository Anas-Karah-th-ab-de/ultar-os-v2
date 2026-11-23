//! A disk that is full or broken: clients are never blocked, nothing
//! crashes, records stay readable from memory, the files stay valid, and
//! writing resumes with a declared gap.

mod common;

use common::{numbered, small_config, Rig};
use ultar_bus::messages::log::Query;
use ultar_logd::chain::Chain;
use ultar_logd::core::Scope;
use ultar_logd::storage::MemStorage;
use ultar_logd::verify::verify;

fn keys(rig: &Rig) -> Vec<String> {
    rig.core
        .query(&Query::default(), &Scope::All)
        .records
        .into_iter()
        .map(|record| record.key)
        .collect()
}

#[test]
fn a_full_disk_degrades_to_memory_and_recovers_with_a_declared_gap() {
    let disk = MemStorage::with_capacity(u64::MAX / 2);
    let mut rig = Rig::on(disk, small_config(), Chain::unkeyed());
    for n in 0..20 {
        rig.core.ingest("app.a", numbered(n));
    }
    assert!(!rig.core.store().is_degraded());
    // The disk is full to the byte from now on.
    rig.disk.set_capacity(rig.disk.used());
    let used = rig.disk.used();
    for n in 0..30 {
        // Every record is accepted and gets its position.
        assert_eq!(
            rig.core.ingest("app.a", numbered(n)),
            Some(21 + n + u64::from(n > 0))
        );
    }
    assert!(rig.core.store().is_degraded());
    assert_eq!(rig.disk.used(), used, "no partial record may stay on disk");
    // The files written before are still a valid log.
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert_eq!(report.last_seq, Some(20));
    // The newest records are readable from memory, with the service's own
    // note that the disk stopped taking data.
    let seen = keys(&rig);
    assert!(seen.contains(&"log.persist_failed".to_owned()));
    assert_eq!(seen.iter().filter(|key| *key == "test.event").count(), 50);
    let stats = rig.core.stats(&Scope::All);
    assert!(stats.degraded && stats.not_persisted >= 30);

    // Space returns: writing resumes by itself after a few records.
    rig.disk.set_capacity(u64::MAX / 2);
    for n in 0..10 {
        rig.core.ingest("app.a", numbered(n));
    }
    assert!(!rig.core.store().is_degraded());
    assert!(keys(&rig).contains(&"log.persist_resumed".to_owned()));
    rig.core.sync();
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert_eq!(report.declared_gaps.len(), 1);
    let (first, last) = report.declared_gaps[0];
    assert_eq!(first, 21);
    assert!(last >= 50, "{last}");
    assert_eq!(report.last_seq, Some(rig.core.last_seq()));
}

#[test]
fn a_disk_that_fills_in_the_middle_of_a_record_leaves_no_fragment() {
    let disk = MemStorage::with_capacity(u64::MAX / 2);
    let mut rig = Rig::on(disk, small_config(), Chain::unkeyed());
    for n in 0..5 {
        rig.core.ingest("app.a", numbered(n));
    }
    // Room for half a record only.
    rig.disk.set_capacity(rig.disk.used() + 40);
    rig.core.ingest("app.a", numbered(99));
    assert!(rig.core.store().is_degraded());
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(
        report.is_intact() && !report.torn_tail,
        "{:?}",
        report.problems
    );
    assert_eq!(report.last_seq, Some(5));
}

#[test]
fn a_broken_disk_from_the_start_still_gives_a_working_service() {
    let disk = MemStorage::with_capacity(u64::MAX / 2);
    disk.set_broken(true);
    let mut rig = Rig::on(disk, small_config(), Chain::unkeyed());
    for n in 0..500 {
        assert!(rig.core.ingest("app.a", numbered(n)).is_some());
    }
    rig.core.sync();
    assert!(rig.core.store().is_degraded());
    assert!(rig.core.ring().len() <= 64);
    let seen = keys(&rig);
    assert_eq!(seen.last().map(String::as_str), Some("test.event"));
    // The disk comes back: the service picks it up without a restart.
    rig.disk.set_broken(false);
    for n in 0..10 {
        rig.core.ingest("app.a", numbered(n));
    }
    assert!(!rig.core.store().is_degraded());
    rig.core.sync();
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert!(report.records >= 1);
}

#[test]
fn a_long_outage_never_grows_memory_or_disk() {
    let disk = MemStorage::with_capacity(u64::MAX / 2);
    let mut rig = Rig::on(disk, small_config(), Chain::unkeyed());
    rig.core.ingest("app.a", numbered(0));
    rig.disk.set_capacity(rig.disk.used());
    let used = rig.disk.used();
    for n in 0..200_000 {
        rig.core.ingest("app.a", numbered(n));
    }
    assert_eq!(rig.disk.used(), used);
    assert!(rig.core.ring().len() <= 64 && rig.core.ring().bytes() <= 32 * 1024);
    assert!(rig.core.store().stats().not_persisted >= 200_000);
}

#[test]
fn restart_after_an_outage_continues_with_new_positions() {
    let disk = MemStorage::with_capacity(u64::MAX / 2);
    let mut rig = Rig::on(disk, small_config(), Chain::unkeyed());
    for n in 0..10 {
        rig.core.ingest("app.a", numbered(n));
    }
    rig.disk.set_capacity(rig.disk.used());
    for n in 0..7 {
        rig.core.ingest("app.a", numbered(n));
    }
    // Power loss while degraded: the records in memory are gone.
    rig.disk.set_capacity(u64::MAX / 2);
    let mut rig = rig.reopen();
    assert_eq!(rig.core.last_seq(), 10);
    assert_eq!(rig.core.ingest("app.a", numbered(0)), Some(11));
    rig.core.sync();
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.is_intact(), "{:?}", report.problems);
    assert_eq!(report.last_seq, Some(11));
}
