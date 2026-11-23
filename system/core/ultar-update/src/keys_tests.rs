use super::*;
use crate::sign::public_of;
use std::fs;

const K1: [u8; 32] = [1; 32];
const K2: [u8; 32] = [2; 32];
const K3: [u8; 32] = [3; 32];

fn set(serial: u64, secrets: &[[u8; 32]]) -> KeySet {
    let mut keys: Vec<PublicKey> = secrets.iter().map(public_of).collect();
    keys.sort_unstable();
    KeySet { serial, keys }
}

fn rotation(serial: u64, secrets: &[[u8; 32]], signer: &[u8; 32]) -> Vec<u8> {
    Rotation {
        set: set(serial, secrets),
        created: 1_795_000_000 + serial,
    }
    .sign(signer)
    .unwrap()
}

fn device(secrets: &[[u8; 32]]) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let burned = dir.path().join("update-keys");
    let state = dir.path().join("state-dir");
    fs::create_dir_all(&burned).unwrap();
    for (i, secret) in secrets.iter().enumerate() {
        let text = format!("# release key {i}\n{}\n\n", hex::encode(public_of(secret)));
        fs::write(burned.join(format!("k{i}{KEY_FILE_SUFFIX}")), text).unwrap();
    }
    (dir, burned, state)
}

#[test]
fn rotation_text_round_trip_and_canonical_form() {
    let r = Rotation {
        set: set(4, &[K1, K2]),
        created: 9,
    };
    let bytes = r.encode().unwrap();
    assert_eq!(Rotation::parse(&bytes).unwrap(), r);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("ultar-update-keys 1\nserial=4\ncreated=9\nkey="));
    let k = hex::encode(r.set.keys[0]);
    for bad in [
        text.replace("serial=4", "serial=04"),
        text.replace("serial=4\n", ""),
        text.replace(&k, &k.to_uppercase()),
        format!("{text}key={k}\n"),
        format!("{text}\n"),
        text.replace("keys 1", "keys 2"),
        "ultar-update-keys 1\nserial=1\ncreated=1\n".to_string(),
    ] {
        assert!(Rotation::parse(bad.as_bytes()).is_err(), "{bad}");
    }
    let mut unsorted = r.clone();
    unsorted.set.keys.reverse();
    assert!(unsorted.encode().is_err());
    let mut many = r;
    many.set.keys = (0..=MAX_KEYS as u8).map(|i| [i; 32]).collect();
    assert!(many.encode().is_err());
}

#[test]
fn burned_keys_are_loaded_sorted_with_serial() {
    let (_dir, burned, state) = device(&[K2, K1]);
    fs::write(burned.join("notes.txt"), "not a key").unwrap();
    let trust = TrustStore::open(&burned, &state).unwrap();
    assert_eq!(trust.keys(), set(0, &[K1, K2]).keys.as_slice());
    assert_eq!(trust.serial(), 0);
    fs::write(burned.join("serial"), "5\n").unwrap();
    assert_eq!(TrustStore::open(&burned, &state).unwrap().serial(), 5);
    fs::write(burned.join("serial"), "five").unwrap();
    assert!(TrustStore::open(&burned, &state).is_err());
}

#[test]
fn missing_directory_gives_an_empty_set_and_damaged_key_file_fails() {
    let dir = tempfile::tempdir().unwrap();
    let trust = TrustStore::open(&dir.path().join("none"), dir.path()).unwrap();
    assert!(trust.keys().is_empty());
    let (_d, burned, state) = device(&[K1]);
    fs::write(burned.join("bad.ed25519"), "zz\n").unwrap();
    assert!(TrustStore::open(&burned, &state).is_err());
}

#[test]
fn rotation_replaces_the_set_and_survives_restart() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    assert!(trust.accept(&rotation(1, &[K1, K2], &K1)).unwrap());
    assert!(trust.accept(&rotation(2, &[K2], &K2)).unwrap());
    assert_eq!(trust.keys(), set(2, &[K2]).keys.as_slice());
    let again = TrustStore::open(&burned, &state).unwrap();
    assert_eq!(again.serial(), 2);
    assert_eq!(again.keys(), set(2, &[K2]).keys.as_slice());
}

#[test]
fn rotation_must_be_signed_by_the_set_in_force() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    // Signed by the new key itself: not trusted yet.
    assert!(matches!(
        trust.accept(&rotation(1, &[K3], &K3)),
        Err(UpdateError::UnknownKey)
    ));
    trust.accept(&rotation(1, &[K2], &K1)).unwrap();
    // The retired key can no longer rotate.
    assert!(matches!(
        trust.accept(&rotation(2, &[K1], &K1)),
        Err(UpdateError::UnknownKey)
    ));
    let mut tampered = rotation(2, &[K3], &K2);
    let at = tampered.len() - 70;
    tampered[at] ^= 1;
    assert!(trust.accept(&tampered).is_err());
    assert_eq!(trust.serial(), 1);
}

#[test]
fn old_rotations_are_ignored_not_replayed() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    let first = rotation(1, &[K1, K2], &K1);
    trust.accept(&first).unwrap();
    trust.accept(&rotation(2, &[K2], &K1)).unwrap();
    assert!(!trust.accept(&first).unwrap(), "replay brings K1 back?");
    assert_eq!(trust.keys(), set(2, &[K2]).keys.as_slice());
}

#[test]
fn deleting_or_truncating_the_chain_fails_closed() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    trust.accept(&rotation(1, &[K2], &K1)).unwrap();
    let chain = state.join("trust/chain");
    let saved = fs::read(&chain).unwrap();
    fs::remove_file(&chain).unwrap();
    assert!(matches!(
        TrustStore::open(&burned, &state),
        Err(UpdateError::StateCorrupt)
    ));
    fs::write(&chain, &saved[..saved.len() - 1]).unwrap();
    assert!(matches!(
        TrustStore::open(&burned, &state),
        Err(UpdateError::StateCorrupt)
    ));
    let mut forged = saved.clone();
    forged[30] ^= 1;
    fs::write(&chain, forged).unwrap();
    assert!(matches!(
        TrustStore::open(&burned, &state),
        Err(UpdateError::StateCorrupt)
    ));
    fs::write(&chain, saved).unwrap();
    assert_eq!(TrustStore::open(&burned, &state).unwrap().serial(), 1);
}

#[test]
fn newer_burned_set_supersedes_the_chain() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    trust.accept(&rotation(1, &[K2], &K1)).unwrap();
    // A new system image burns in the rotated key and its serial.
    fs::remove_file(burned.join(format!("k0{KEY_FILE_SUFFIX}"))).unwrap();
    fs::write(burned.join("new.ed25519"), hex::encode(public_of(&K3))).unwrap();
    fs::write(burned.join("serial"), "3").unwrap();
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    assert_eq!(trust.serial(), 3);
    assert_eq!(trust.keys(), set(3, &[K3]).keys.as_slice());
    assert!(trust.accept(&rotation(4, &[K1], &K3)).unwrap());
}

#[test]
fn chain_length_is_bounded() {
    let (_dir, burned, state) = device(&[K1]);
    let mut trust = TrustStore::open(&burned, &state).unwrap();
    for serial in 1..=MAX_CHAIN as u64 {
        assert!(trust.accept(&rotation(serial, &[K1], &K1)).unwrap());
    }
    assert!(matches!(
        trust.accept(&rotation(1000, &[K1], &K1)),
        Err(UpdateError::RotationRefused)
    ));
    assert_eq!(
        TrustStore::open(&burned, &state).unwrap().serial(),
        MAX_CHAIN as u64
    );
}

#[test]
fn secret_file_reading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("secret");
    fs::write(&path, format!("{}\n", hex::encode(K1))).unwrap();
    assert_eq!(read_secret_file(&path).unwrap(), K1);
    fs::write(&path, "").unwrap();
    assert!(read_secret_file(&path).is_err());
    assert!(read_secret_file(&dir.path().join("none")).is_err());
}
