//! Fifty clients change different settings at the same time.

mod common;

use common::{Rig, DELAY, WAIT};
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::thread;
use ultar_bus::messages::settings::{Changed, Get, ListRequest, RiskInfo, Set, SettingValue};
use ultar_bus::Client;

const CLIENTS: usize = 50;

/// One writable on/off setting per client, straight from the service,
/// topped up with per-app settings; each with its current value.
fn fifty_toggles(admin: &Client) -> Vec<(String, bool)> {
    let listed = admin.call(&ListRequest::default(), WAIT).unwrap();
    let toggles: Vec<(String, bool)> = listed
        .panels
        .into_iter()
        .flat_map(|panel| panel.items)
        .filter(|item| !item.template)
        .filter_map(|item| match (item.value, item.risk) {
            (SettingValue::Flag(now), RiskInfo::Safe) => Some((item.key, now)),
            _ => None,
        })
        .chain((0..CLIENTS).map(|n| (format!("notifications.app.app{n}.sound"), true)))
        .take(CLIENTS)
        .collect();
    assert_eq!(toggles.len(), CLIENTS);
    toggles
}

/// Every client flips its own setting at the same moment and reads it back.
fn flip_all_at_once(rig: &Rig, toggles: &[(String, bool)]) {
    let start = Arc::new(Barrier::new(CLIENTS));
    let workers: Vec<_> = toggles
        .iter()
        .cloned()
        .map(|(key, now)| {
            let client = rig.client("app.settings");
            let start = Arc::clone(&start);
            thread::spawn(move || {
                start.wait();
                let call = Set {
                    key: key.clone(),
                    value: SettingValue::Flag(!now),
                };
                assert!(client.call(&call, WAIT).unwrap().changed, "{key}");
                let read = client.call(&Get { key: key.clone() }, WAIT).unwrap();
                assert_eq!(read.value, Some(SettingValue::Flag(!now)), "{key}");
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn fifty_clients_set_fifty_different_keys() {
    let rig = Rig::new("concurrent");
    let server = rig.start();
    let watcher = rig.client("app.viewer");
    let changes = watcher.subscribe::<Changed>().unwrap();
    let admin = rig.client("app.settings");
    let toggles = fifty_toggles(&admin);
    flip_all_at_once(&rig, &toggles);

    // Exactly one event per key, each with the new value.
    let mut seen = BTreeMap::new();
    for _ in 0..CLIENTS {
        let change = changes.recv_timeout(WAIT).unwrap();
        assert!(seen.insert(change.key, change.value).is_none());
    }
    for (key, before) in &toggles {
        assert_eq!(seen.get(key), Some(&SettingValue::Flag(!before)), "{key}");
    }
    // No surplus event: the next one is the marker set now.
    let marker = Set {
        key: "appearance.font_size".into(),
        value: SettingValue::Integer(150),
    };
    admin.call(&marker, WAIT).unwrap();
    let next = changes.recv_timeout(WAIT).unwrap();
    assert_eq!(next.key, "appearance.font_size");

    // Nothing was written while the clients were busy; afterwards one
    // write per touched file covers all fifty changes.
    let store = rig.store_dir();
    assert!(!store.join("user-anas.toml").exists());
    assert!(!store.join("system.toml").exists());
    rig.clock.advance(DELAY);
    assert!(server.tick());
    assert!(store.join("user-anas.toml").exists());
    server.stop();

    // After a restart every value is still there.
    let _server = rig.start();
    for (key, before) in &toggles {
        let read = admin.call(&Get { key: key.clone() }, WAIT).unwrap();
        assert_eq!(read.value, Some(SettingValue::Flag(!before)), "{key}");
    }
    assert_eq!(rig.applier.total() as usize, CLIENTS + 1);
}

/// Measures round trips through broker and service. Run with
/// `cargo test --release --test concurrency -- --ignored --nocapture`.
#[test]
#[ignore = "measurement, not a check"]
fn measure_round_trips() {
    use std::time::Instant;
    const CALLS: u32 = 2000;
    let rig = Rig::new("measure");
    let _server = rig.start();
    let client = rig.client("app.settings");
    let get = Get {
        key: "appearance.theme".into(),
    };
    let started = Instant::now();
    for _ in 0..CALLS {
        client.call(&get, WAIT).unwrap();
    }
    let per_get = started.elapsed() / CALLS;
    let started = Instant::now();
    for round in 0..CALLS {
        let call = Set {
            key: "appearance.font_size".into(),
            value: SettingValue::Integer(if round % 2 == 0 { 110 } else { 120 }),
        };
        client.call(&call, WAIT).unwrap();
    }
    let per_set = started.elapsed() / CALLS;
    let started = Instant::now();
    let listed = client.call(&ListRequest::default(), WAIT).unwrap();
    let list = started.elapsed();
    let bytes = ultar_bus::encode(&listed).unwrap().len();
    println!("get: {per_get:?} per call, set (with event): {per_set:?} per call");
    println!("list of all panels: {list:?}, {bytes} bytes");
}
