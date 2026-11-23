//! Fair admission to the engine's queue, with the engine stalled on
//! purpose (nobody reads the queue): a source that floods takes at most its
//! share, and another source's records still fit. Deterministic – no
//! reliance on how fast the engine happens to be.

mod common;

use common::temp_dir;
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{sync_channel, Receiver};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_logd::ingest::{Delivery, Ingest, IngestLimits, ListenerSpec, SourceNames};
use ultar_service::log::wire::encode_frame;
use ultar_service::log::{Level, RateLimit, Record};

const LIMITS: IngestLimits = IngestLimits {
    max_connections: 8,
    max_per_source: 4,
    rate: RateLimit {
        burst: u32::MAX,
        per_second: 0,
    },
    share: 16,
};

fn listener(path: std::path::PathBuf, name: &str) -> ListenerSpec {
    ListenerSpec {
        path,
        names: SourceNames::Fixed(name.to_owned()),
        mode: None,
    }
}

/// Writes frames until the connection is closed by the ingestion side.
fn flood(path: std::path::PathBuf) {
    let frame = encode_frame(&Record::new(Level::Info, "flood.event")).unwrap();
    let Ok(mut stream) = UnixStream::connect(path) else {
        return;
    };
    while stream.write_all(&frame).is_ok() {}
}

/// Takes deliveries until `wanted` records of `service` came, or the time
/// is over; returns how many came and whether a drop of `service` was
/// reported.
fn collect(queue: &Receiver<Delivery>, service: &str, wanted: u64) -> (u64, bool) {
    let (mut seen, mut dropped) = (0, false);
    let deadline = Instant::now() + Duration::from_secs(10);
    while seen < wanted && Instant::now() < deadline {
        match queue.recv_timeout(Duration::from_millis(50)) {
            Ok(Delivery::Record { service: from, .. }) if from == service => seen += 1,
            Ok(Delivery::Dropped { service: from, .. }) if from == service => dropped = true,
            _ => {}
        }
    }
    (seen, dropped)
}

#[test]
fn a_flooding_source_cannot_take_the_queue_room_of_another_source() {
    assert!(LIMITS.fits(LIMITS.max_connections * LIMITS.share));
    let dir = temp_dir("fair");
    let (sink, queue) = sync_channel::<Delivery>(LIMITS.max_connections * LIMITS.share);
    let listeners = [
        listener(dir.join("flood.sock"), "app.two"),
        listener(dir.join("victim.sock"), "app.one"),
    ];
    let ingest = Ingest::start(&listeners, LIMITS, Arc::new(sink)).unwrap();
    let flooders: Vec<_> = (0..4)
        .map(|_| {
            let path = dir.join("flood.sock");
            std::thread::spawn(move || flood(path))
        })
        .collect();
    // The engine is stalled: nothing reads the queue. Give the flood time
    // to fill everything it may.
    std::thread::sleep(Duration::from_millis(300));
    let mut victim = UnixStream::connect(dir.join("victim.sock")).unwrap();
    for n in 0..10 {
        let record = Record::new(Level::Warn, "victim.event").count("n", n);
        victim.write_all(&encode_frame(&record).unwrap()).unwrap();
    }
    // Still stalled while the victim's records are offered.
    std::thread::sleep(Duration::from_millis(300));
    let (seen, dropped) = collect(&queue, "app.one", 10);
    drop(ingest);
    drop(queue);
    for flooder in flooders {
        let _ = flooder.join();
    }
    assert!(!dropped, "records of the victim were dropped");
    assert_eq!(seen, 10, "victim records that reached the engine");
}
