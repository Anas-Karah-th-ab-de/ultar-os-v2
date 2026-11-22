//! Power-loss simulation: a settings file cut off or garbled at every byte
//! of a write must never be accepted as something it is not.

use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::StoreNoticeKind;
use ultar_settingsd::clock::ManualClock;
use ultar_settingsd::seed;
use ultar_settingsd::store::{encode, FileContent, Slot, Store, StoreConfig};
use ultar_settingsd::testdir::TestDir;
use ultar_settingsd::value::Value;

const FILE: &str = "user-anas.toml";

fn open(dir: &Path) -> (Store, Vec<ultar_settingsd::store::Notice>) {
    let config = StoreConfig {
        dir: dir.to_path_buf(),
        user: "anas".into(),
        write_delay: Duration::from_millis(100),
    };
    let registry = seed::registry().unwrap();
    Store::open(config, &registry, Arc::new(ManualClock::new())).unwrap()
}

/// A small file as the store writes it: two values of the user scope.
fn small_file() -> (FileContent, String) {
    let mut content = FileContent {
        schema: seed::SCHEMA_VERSION,
        ..FileContent::default()
    };
    content
        .values
        .insert("appearance.theme".into(), Value::text("light"));
    content
        .values
        .insert("appearance.font_size".into(), Value::Int(120));
    let text = encode(&content);
    (content, text)
}

fn stored(store: &Store) -> Vec<(String, Value)> {
    store
        .keys(Slot::User)
        .map(|key| (key.to_owned(), store.get(Slot::User, key).unwrap().clone()))
        .collect()
}

/// Opens a store on `bytes` and checks the only two acceptable outcomes:
/// the complete original values, or defaults with a quarantine copy of
/// exactly these bytes and a notice.
fn check(dir: &Path, bytes: &[u8], original: &FileContent, what: &str) {
    let _ = fs::remove_dir_all(dir.join("quarantine"));
    fs::write(dir.join(FILE), bytes).unwrap();
    let (store, notices) = open(dir);
    let values = stored(&store);
    let complete: Vec<(String, Value)> = original.values.clone().into_iter().collect();
    if values == complete {
        assert!(notices.is_empty(), "{what}: {notices:?}");
        return;
    }
    assert!(
        values.is_empty(),
        "{what}: partial content accepted: {values:?}"
    );
    assert_eq!(notices.len(), 1, "{what}: {notices:?}");
    assert_eq!(notices[0].kind, StoreNoticeKind::Quarantined, "{what}");
    assert_eq!(notices[0].file, FILE);
    let copy = fs::read(dir.join(&notices[0].detail)).unwrap();
    assert_eq!(
        copy, bytes,
        "{what}: the quarantine copy must be the damaged file"
    );
}

#[test]
fn file_cut_off_at_every_byte_is_detected() {
    let dir = TestDir::new("loss-truncate");
    let (original, text) = small_file();
    let bytes = text.as_bytes();
    assert!(
        bytes.len() < 200,
        "keep the file small: {} bytes",
        bytes.len()
    );
    let mut accepted = 0;
    for len in 0..=bytes.len() {
        check(
            dir.path(),
            &bytes[..len],
            &original,
            &format!("cut at {len}"),
        );
        if stored(&open(dir.path()).0).len() == original.values.len() {
            accepted += 1;
        }
    }
    // Only the complete file is accepted; no prefix passes as valid.
    assert_eq!(accepted, 1);
}

#[test]
fn file_garbled_at_every_byte_is_detected() {
    let dir = TestDir::new("loss-garble");
    let (_, text) = small_file();
    let bytes = text.as_bytes();
    for position in 0..bytes.len() {
        for garbage in [
            0x00,
            0xff,
            bytes[position] ^ 0x01,
            bytes[position] ^ 0x20,
            b'\n',
        ] {
            if garbage == bytes[position] {
                continue;
            }
            let mut damaged = bytes.to_vec();
            damaged[position] = garbage;
            fs::write(dir.path().join(FILE), &damaged).unwrap();
            let (store, notices) = open(dir.path());
            let what = format!("byte {position} -> {garbage:#04x}");
            assert!(
                stored(&store).is_empty(),
                "{what}: damaged content accepted"
            );
            assert_eq!(notices.len(), 1, "{what}: {notices:?}");
            assert_eq!(notices[0].kind, StoreNoticeKind::Quarantined, "{what}");
        }
    }
}

#[test]
fn tail_of_the_old_file_after_a_shorter_new_one_is_detected() {
    // A write in place that stopped half way: new bytes first, old bytes after.
    let dir = TestDir::new("loss-mixed");
    let (original, new) = small_file();
    let mut old_content = original.clone();
    old_content
        .values
        .insert("appearance.accent_color".into(), Value::text("teal"));
    let old = encode(&old_content);
    for len in 1..new.len() {
        let mut mixed = new.as_bytes()[..len].to_vec();
        mixed.extend_from_slice(&old.as_bytes()[len.min(old.len())..]);
        if mixed == old.as_bytes() || mixed == new.as_bytes() {
            continue;
        }
        fs::write(dir.path().join(FILE), &mixed).unwrap();
        let (store, notices) = open(dir.path());
        assert!(stored(&store).is_empty(), "mixed at {len}");
        assert_eq!(notices[0].kind, StoreNoticeKind::Quarantined);
    }
}

#[test]
fn interrupted_atomic_write_leaves_the_old_file_intact() {
    // The store writes to a temporary file and renames it. Power loss at
    // any byte of that write leaves a partial temporary file next to the
    // complete old file: the old values must be loaded, nothing reported,
    // and the leftover removed.
    let dir = TestDir::new("loss-temp");
    let (original, old) = small_file();
    let mut newer = original.clone();
    newer
        .values
        .insert("appearance.theme".into(), Value::text("dark"));
    let new = encode(&newer);
    let temp = dir.path().join(format!("{FILE}.tmp"));
    for len in 0..=new.len() {
        fs::write(dir.path().join(FILE), &old).unwrap();
        fs::write(&temp, &new.as_bytes()[..len]).unwrap();
        let (store, notices) = open(dir.path());
        assert!(notices.is_empty(), "temp cut at {len}: {notices:?}");
        assert_eq!(
            store.get(Slot::User, "appearance.theme"),
            Some(&Value::text("light"))
        );
        assert_eq!(store.len(Slot::User), 2);
        assert!(!temp.exists());
    }
}

#[test]
fn store_recovers_and_writes_a_clean_file_after_damage() {
    let dir = TestDir::new("loss-recover");
    let (_, text) = small_file();
    fs::write(dir.path().join(FILE), &text.as_bytes()[..text.len() / 2]).unwrap();
    let (mut store, notices) = open(dir.path());
    assert_eq!(notices.len(), 1);
    assert!(store.set(Slot::User, "appearance.theme", Value::text("light")));
    store.flush().unwrap();
    let (store, notices) = open(dir.path());
    assert!(notices.is_empty());
    assert_eq!(
        store.get(Slot::User, "appearance.theme"),
        Some(&Value::text("light"))
    );
}
