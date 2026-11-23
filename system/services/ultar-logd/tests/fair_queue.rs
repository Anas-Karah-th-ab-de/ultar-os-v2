//! Fair admission to the engine's queue, with the engine stalled on
//! purpose (nobody reads the queue): whatever a flooding source puts into
//! the queue – records, the counts of its refused records, rejected
//! connections – stays within its share, and another source's records
//! still fit. Deterministic – no reliance on how fast the engine is.

mod common;

use common::temp_dir;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_logd::ingest::{Delivery, Ingest, IngestLimits, ListenerSpec, SourceNames};
use ultar_service::log::wire::encode_frame;
use ultar_service::log::{Level, RateLimit, Record};

/// Unlimited rate: the flood consists of records.
const UNLIMITED: RateLimit = RateLimit {
    burst: u32::MAX,
    per_second: 0,
};
/// 12 records, then nothing more: the flood consists of refused records
/// whose counts are reported. 12 is below the share (16), so the flooder's
/// own records do not stop it; the victim's 10 stay inside the limit.
const TWELVE: RateLimit = RateLimit {
    burst: 12,
    per_second: 0,
};

fn limits(rate: RateLimit) -> IngestLimits {
    IngestLimits {
        max_connections: 8,
        max_per_source: 4,
        rate,
        share: 16,
    }
}

fn listener(path: PathBuf, name: &str) -> ListenerSpec {
    ListenerSpec {
        path,
        names: SourceNames::Fixed(name.to_owned()),
        mode: None,
        group: None,
    }
}

/// Writes frames until the connection is closed by the ingestion side.
fn flood(path: PathBuf) {
    let frame = encode_frame(&Record::new(Level::Info, "flood.event")).unwrap();
    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };
    while stream.write_all(&frame).is_ok() {}
}

/// Connects again and again beyond the connection bound: each one is a
/// rejection.
fn reconnect(path: PathBuf, count: usize) {
    let held: Vec<_> = (0..count)
        .filter_map(|_| UnixStream::connect(&path).ok())
        .collect();
    std::thread::sleep(Duration::from_millis(500));
    drop(held);
}

/// What reached the engine for one source.
#[derive(Default, Debug)]
struct Seen {
    records: u64,
    dropped: u64,
}

/// Takes deliveries until `wanted` records of `service` came, or the time
/// is over.
fn collect(queue: &Receiver<Delivery>, service: &str, wanted: u64) -> Seen {
    let mut seen = Seen::default();
    let deadline = Instant::now() + Duration::from_secs(10);
    while seen.records < wanted && Instant::now() < deadline {
        match queue.recv_timeout(Duration::from_millis(50)) {
            Ok(Delivery::Record { service: from, .. }) if from == service => seen.records += 1,
            Ok(Delivery::Counts {
                service: from,
                counts,
                ..
            }) if from == service => seen.dropped += counts.rate + counts.queue,
            _ => {}
        }
    }
    seen
}

/// Starts ingestion on a stalled queue, lets `attack` run, then sends ten
/// records as the victim; returns what reached the engine of the victim.
fn victim_under(rate: RateLimit, attack: impl Fn(&Path) + Send + Sync + 'static) -> Seen {
    let limits = limits(rate);
    let capacity = (limits.max_connections + 1) * limits.share;
    assert!(limits.fits(capacity));
    let dir = temp_dir("fair");
    let (sink, queue) = sync_channel::<Delivery>(capacity);
    let listeners = [
        listener(dir.join("flood.sock"), "app.two"),
        listener(dir.join("victim.sock"), "app.one"),
    ];
    let ingest = Ingest::start(&listeners, limits, Arc::new(sink)).unwrap();
    let attack = Arc::new(attack);
    let attackers: Vec<_> = (0..4)
        .map(|_| {
            let (attack, dir) = (attack.clone(), dir.clone());
            std::thread::spawn(move || attack(&dir))
        })
        .collect();
    // Nothing reads the queue: give the attack time to fill all it may.
    std::thread::sleep(Duration::from_millis(300));
    let mut victim = UnixStream::connect(dir.join("victim.sock")).unwrap();
    for n in 0..10 {
        let record = Record::new(Level::Warn, "victim.event").count("n", n);
        victim.write_all(&encode_frame(&record).unwrap()).unwrap();
    }
    // Still stalled while the victim's records are offered.
    std::thread::sleep(Duration::from_millis(300));
    let seen = collect(&queue, "app.one", 10);
    drop(ingest);
    drop(queue);
    for attacker in attackers {
        let _ = attacker.join();
    }
    seen
}

#[test]
fn a_flood_of_records_cannot_take_the_queue_room_of_another_source() {
    let seen = victim_under(UNLIMITED, |dir| flood(dir.join("flood.sock")));
    assert_eq!((seen.records, seen.dropped), (10, 0), "{seen:?}");
}

#[test]
fn the_drop_counts_of_a_flood_over_its_rate_cannot_take_that_room_either() {
    let seen = victim_under(TWELVE, |dir| flood(dir.join("flood.sock")));
    assert_eq!((seen.records, seen.dropped), (10, 0), "{seen:?}");
}

#[test]
fn rejected_connections_cannot_take_that_room_either() {
    // 4 × 200 connections against a bound of 8.
    let seen = victim_under(UNLIMITED, |dir| reconnect(dir.join("flood.sock"), 200));
    assert_eq!((seen.records, seen.dropped), (10, 0), "{seen:?}");
}
