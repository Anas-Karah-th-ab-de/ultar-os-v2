//! Review round 3 (service runtime): regression tests for confirmed defects
//! of the log service. Target path:
//! `system/services/ultar-logd/tests/review3_logd.rs`.
//!
//! Every test marked `R3-…` FAILS on commit 157060b and must pass once the
//! defect named in its comment is fixed. The test marked "verified as
//! sound" passes today and stays as a guard.

mod common;

use common::world::World;
use common::{numbered, owned, small_config, Rig};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::log::Query;
use ultar_logd::chain::Chain;
use ultar_logd::core::{CoreConfig, Scope};
use ultar_logd::record::clean_text;
use ultar_logd::storage::Storage;
use ultar_logd::store::HEAD_NAME;
use ultar_logd::verify::verify;
use ultar_service::log::wire::encode_frame;
use ultar_service::log::{Level, Record};

/// 48 characters, the longest key and word. Adapted for R3-W1: the digit
/// run is at most six long (the review's version had a run of ten, which
/// the wire now refuses as a key or word).
const LONG: &str = "abcdefghijklmnopqrstuvwxyz012345abcdefghijklmnop";

/// The largest record the wire accepts from a well-formed client.
fn fat() -> Record {
    let mut record = Record::new(Level::Info, LONG);
    for name in ["f1", "f2", "f3", "f4", "f5", "f6", "f7", "f8"] {
        record = record.word(name, LONG);
    }
    record
}

/// R3-L1: retention is one pool for all sources. A single source that
/// stays inside its rate limit (200 at once, 50 per second) pushes every
/// other source's records out of memory and off the disk within about eleven minutes;
/// the "30 days / 16 MiB" bound gives no source a share of its own.
#[test]
fn one_source_inside_its_rate_limit_must_not_evict_another_sources_records() {
    let mut rig = Rig::new(CoreConfig::STANDARD, Chain::unkeyed());
    let evidence = owned(&Record::new(Level::Warn, "bus.auth_failed").count("listener", 4));
    assert!(rig.core.ingest("system.busd", evidence).is_some());
    let fat = owned(&fat());
    // Twenty minutes of the noisy source at exactly its permitted rate.
    for _ in 0..(20 * 60 * 50) {
        rig.clock.advance(20);
        rig.core.ingest("system.devd", fat.clone());
    }
    let query = Query {
        service: Some("system.busd".into()),
        ..Query::default()
    };
    let found = rig.core.query(&query, &Scope::All).records.len();
    assert_eq!(found, 1, "the other source's record is gone after 20 min");
}

/// R3-L3: the cut tail is found "through the authenticated head file" –
/// but a missing head file is no finding. Whoever can delete files removes
/// the newest segment together with the head; `ulog verify` then calls a
/// keyed log intact, and the next start writes a fresh head.
#[test]
fn a_removed_tail_is_detected_also_when_the_head_file_is_removed_with_it() {
    let mut rig = Rig::new(small_config(), Chain::keyed(b"a key the attacker lacks"));
    for n in 0..300 {
        rig.core.ingest("app.a", numbered(n));
    }
    rig.core.sync();
    let last = rig.segments().pop().unwrap();
    let mut disk = rig.disk.clone();
    Storage::remove(&mut disk, &last).unwrap();
    Storage::remove(&mut disk, HEAD_NAME).unwrap();
    let report = verify(&rig.disk, &rig.chain, 1 << 20);
    assert!(report.last_seq < Some(300), "the tail is really gone");
    assert!(
        !report.is_intact(),
        "verify calls the log intact: {report:?}"
    );
}

/// R3-L4: kernel lines are free text that ends on the administrator's
/// terminal. Control characters are replaced, but the invisible direction
/// overrides (U+202A–U+202E, U+2066–U+2069) pass and let a line show
/// something other than it contains.
#[test]
fn direction_overrides_in_kernel_text_do_not_reach_the_terminal() {
    let cleaned = clean_text("exec /bin/\u{202e}dmr- mr\u{2066} ok");
    let visible = cleaned.chars().all(|c| !is_direction_control(c));
    assert!(visible, "stored kernel text: {cleaned:?}");
}

fn is_direction_control(c: char) -> bool {
    matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}')
}

fn flood(path: std::path::PathBuf, stop: Arc<AtomicBool>) {
    let frame = encode_frame(&fat()).unwrap();
    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };
    while !stop.load(Ordering::SeqCst) {
        if stream.write_all(&frame).is_err() {
            return;
        }
    }
}

/// Verified as sound (passes): while one source floods its socket on all its
/// connections, another source's records must still arrive.
#[test]
fn a_flooding_source_does_not_starve_another_source() {
    let world = World::new("r3-starve");
    let child = world.start(&["--rate-burst", "200", "--rate-per-second", "50"]);
    let stop = Arc::new(AtomicBool::new(false));
    let flooders: Vec<_> = (0..4)
        .map(|_| {
            let (path, stop) = (world.path("two.sock"), stop.clone());
            std::thread::spawn(move || flood(path, stop))
        })
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    let logger = world.logger("ingest.sock");
    for n in 0..100 {
        logger.emit(Record::new(Level::Info, "victim.event").count("n", n));
        std::thread::sleep(Duration::from_millis(20));
    }
    logger.flush(Duration::from_secs(5));
    std::thread::sleep(Duration::from_millis(500));
    let admin = world.client("admin.support");
    let stats = world.stats(&admin);
    stop.store(true, Ordering::SeqCst);
    for flooder in flooders {
        let _ = flooder.join();
    }
    let victim = stats.sources.iter().find(|s| s.service == "app.one");
    let stored = victim.map_or(0, |s| s.stored);
    common::world::stop(child);
    assert_eq!(stored, 100, "victim records stored: {stats:?}");
}
