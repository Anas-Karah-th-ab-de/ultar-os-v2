//! The file-backed store: atomic replacement, verification and recovery.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use ultar_devd::store::file::{
    decode, encode, FileStore, StoreEvent, StoreProblem, CORRUPT_FILE, MAX_EVENTS, MAX_STATE_BYTES,
    STATE_FILE, TEMP_FILE,
};
use ultar_devd::{ChoiceStore, DriverKey, FailureReason, HealthFailure, StoreError, StoredState};
use ultar_driverpkg::Version;

fn dir(tag: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let path = std::env::temp_dir().join(format!("devd-store-{}-{n}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    path
}

fn key(id: &str) -> DriverKey {
    DriverKey::new(id, Version::new(1, 0, 0))
}

fn sample() -> StoredState {
    let mut state = StoredState::default();
    state
        .choices
        .insert("pci:00:1f.6=pci:8086:0d4c".into(), key("gen-intel-eth"));
    state
        .previous
        .insert("pci:00:1f.6=pci:8086:0d4c".into(), key("old-eth"));
    state.quarantine_driver(
        "pci:01:00.0=pci:144d:a808",
        key("bad-nvme"),
        FailureReason::Health(HealthFailure::NoResponse),
    );
    state
}

fn recovered(problem: StoreProblem) -> Vec<StoreEvent> {
    vec![StoreEvent::Recovered {
        problem,
        kept: true,
    }]
}

#[test]
fn save_and_load_round_trip_across_instances() {
    let dir = dir("roundtrip").join("nested/state");
    let mut store = FileStore::new(&dir);
    assert_eq!(store.load().unwrap(), StoredState::default());
    assert!(!dir.exists(), "loading must not create anything");
    store.save(&sample()).unwrap();
    store.save(&sample()).unwrap();
    assert_eq!(store.saves(), 2);
    assert!(!dir.join(TEMP_FILE).exists());
    let text = std::fs::read_to_string(store.path()).unwrap();
    assert!(text.starts_with("# ultar-devd state v1 sha256="));
    let mut again = FileStore::new(&dir);
    assert_eq!(again.load().unwrap(), sample());
    assert!(again.events().is_empty());
    again.save(&StoredState::default()).unwrap();
    assert_eq!(FileStore::new(&dir).load().unwrap(), StoredState::default());
}

#[test]
fn every_kind_of_damage_is_detected_and_recovered() {
    let good = encode(&sample()).unwrap();
    let body_at = good.find('\n').unwrap() + 1;
    let truncated = &good[..good.len() - 30];
    // Cut at a line boundary: still valid TOML, only the header can tell.
    let line_cut = &good[..good[..good.len() - 2].rfind('\n').unwrap() + 1];
    let flipped = good.replacen("gen-intel-eth", "gen-intel-eti", 1);
    let newer = good.replacen("state v1 ", "state v7 ", 1);
    let other_body = "format = 9\n";
    let wrong_body = format!(
        "# ultar-devd state v1 sha256={} bytes={}\n{other_body}",
        ultar_driverpkg::keys::sha256_hex(other_body.as_bytes()),
        other_body.len()
    );
    let cases: Vec<(Vec<u8>, StoreProblem)> = vec![
        (Vec::new(), StoreProblem::Header),
        (b"format = 1\n".to_vec(), StoreProblem::Header),
        (good.as_bytes()[body_at..].to_vec(), StoreProblem::Header),
        (vec![0xff, 0xfe, b'\n'], StoreProblem::Header),
        (
            good.replacen(" bytes=", " bytes=x", 1).into_bytes(),
            StoreProblem::Header,
        ),
        (
            good.replacen('\n', " extra\n", 1).into_bytes(),
            StoreProblem::Header,
        ),
        (truncated.as_bytes().to_vec(), StoreProblem::Length),
        (line_cut.as_bytes().to_vec(), StoreProblem::Length),
        (format!("{good}x").into_bytes(), StoreProblem::Length),
        (flipped.into_bytes(), StoreProblem::Checksum),
        (newer.into_bytes(), StoreProblem::UnsupportedVersion(7)),
        (wrong_body.into_bytes(), StoreProblem::Body),
    ];
    for (index, (bytes, problem)) in cases.into_iter().enumerate() {
        assert_eq!(decode(&bytes), Err(problem), "case {index}");
        let dir = dir("damage");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(STATE_FILE), &bytes).unwrap();
        let mut store = FileStore::new(&dir);
        assert_eq!(
            store.load().unwrap(),
            StoredState::default(),
            "case {index}"
        );
        assert_eq!(store.take_events(), recovered(problem), "case {index}");
        assert!(store.events().is_empty());
        assert_eq!(std::fs::read(dir.join(CORRUPT_FILE)).unwrap(), bytes);
        assert!(!dir.join(STATE_FILE).exists());
        // The store works normally afterwards.
        store.save(&sample()).unwrap();
        assert_eq!(FileStore::new(&dir).load().unwrap(), sample());
        assert!(problem.key().starts_with("devd.store.problem."));
    }
}

#[test]
fn oversized_file_and_stale_temp_file() {
    let dir = dir("big");
    std::fs::create_dir_all(&dir).unwrap();
    let file = std::fs::File::create(dir.join(STATE_FILE)).unwrap();
    file.set_len(MAX_STATE_BYTES + 1).unwrap();
    drop(file);
    let mut store = FileStore::new(&dir);
    assert_eq!(store.load().unwrap(), StoredState::default());
    assert_eq!(store.take_events(), recovered(StoreProblem::TooLarge));
    assert_eq!(StoreProblem::TooLarge.key(), "devd.store.problem.too-large");
    // A temporary file left by a power loss is ignored and replaced by the next save.
    std::fs::write(dir.join(TEMP_FILE), "half written").unwrap();
    assert_eq!(store.load().unwrap(), StoredState::default());
    store.save(&sample()).unwrap();
    assert!(!dir.join(TEMP_FILE).exists());
    assert_eq!(store.load().unwrap(), sample());
}

#[test]
fn unusable_locations_give_typed_errors() {
    // The state directory is a file: nothing can be created below it.
    let base = dir("blocked");
    std::fs::create_dir_all(&base).unwrap();
    let blocker = base.join("blocker");
    std::fs::write(&blocker, "x").unwrap();
    let mut store = FileStore::new(&blocker.join("state"));
    assert_eq!(store.save(&sample()), Err(StoreError::Unavailable));
    assert_eq!(store.load(), Err(StoreError::Unavailable));
    // The state file is a directory: reading and replacing fail, nothing is destroyed.
    let dir = dir("isdir");
    std::fs::create_dir_all(dir.join(STATE_FILE).join("keep")).unwrap();
    let mut store = FileStore::new(&dir);
    assert_eq!(store.load(), Err(StoreError::Unavailable));
    assert_eq!(store.save(&sample()), Err(StoreError::WriteFailed));
    assert!(!dir.join(TEMP_FILE).exists(), "failed save must clean up");
    assert!(dir.join(STATE_FILE).join("keep").exists());
    assert_eq!(store.saves(), 0);
}

#[test]
fn events_are_bounded() {
    let dir = dir("events");
    std::fs::create_dir_all(&dir).unwrap();
    let mut store = FileStore::new(&dir);
    for _ in 0..MAX_EVENTS + 4 {
        std::fs::write(dir.join(STATE_FILE), "junk").unwrap();
        store.load().unwrap();
    }
    assert_eq!(store.events().len(), MAX_EVENTS);
}
