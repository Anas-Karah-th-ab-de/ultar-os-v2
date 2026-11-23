//! Security review r3 (device identity): handling of the device key file beyond what
//! `src/keystore_tests.rs` already covers – special files, links that point nowhere, oversized
//! files, left-over temporary files, and what errors and debug output reveal. All tests pass on
//! the reviewed code.
#![cfg(unix)]

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::Path;
use std::time::{Duration, Instant};
use ultar_cloud::{FileKeyStore, KeyStore, KeyStoreError};

fn names(dir: &Path) -> Vec<String> {
    let mut all: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    all.sort();
    all
}

#[test]
fn a_link_that_points_nowhere_is_not_followed_when_the_key_is_created() {
    let dir = tempfile::tempdir().unwrap();
    let (path, target) = (dir.path().join("device.key"), dir.path().join("elsewhere"));
    symlink(&target, &path).unwrap();
    let result = FileKeyStore::open_or_create(&path);
    assert!(
        matches!(result, Err(KeyStoreError::Insecure(_))),
        "{result:?}"
    );
    assert!(!target.exists(), "a key was written through the link");
    assert_eq!(names(dir.path()), ["device.key"], "no temporary file left");
    assert!(std::fs::symlink_metadata(&path)
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn a_pipe_in_place_of_the_key_is_refused_without_waiting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.key");
    let made = std::process::Command::new("mkfifo").arg(&path).status();
    assert!(
        made.is_ok_and(|s| s.success()),
        "mkfifo is needed for this test"
    );
    let begun = Instant::now();
    let store = FileKeyStore::new(&path);
    assert!(matches!(
        store.public_key(),
        Err(KeyStoreError::Insecure(_))
    ));
    assert!(matches!(store.sign(b"x"), Err(KeyStoreError::Insecure(_))));
    assert!(matches!(
        FileKeyStore::open_or_create(&path),
        Err(KeyStoreError::Insecure(_))
    ));
    assert!(
        begun.elapsed() < Duration::from_secs(2),
        "blocked on the pipe"
    );
}

#[test]
fn an_oversized_or_binary_file_is_damaged_not_read_in() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.key");
    for content in [vec![b'a'; 4 * 1024 * 1024], vec![0xff; 200], Vec::new()] {
        std::fs::write(&path, &content).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let store = FileKeyStore::new(&path);
        assert!(matches!(store.public_key(), Err(KeyStoreError::Corrupt)));
        assert!(matches!(
            FileKeyStore::open_or_create(&path),
            Err(KeyStoreError::Corrupt)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), content, "left as it was");
    }
}

#[test]
fn creation_leaves_one_private_file_and_nothing_else() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.key");
    for _ in 0..5 {
        FileKeyStore::open_or_create(&path).unwrap();
    }
    assert_eq!(names(dir.path()), ["device.key"]);
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}

#[test]
fn neither_errors_nor_debug_output_carry_key_material() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.key");
    let store = FileKeyStore::open_or_create(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let seed = text
        .lines()
        .find_map(|l| l.strip_prefix("seed="))
        .unwrap()
        .to_string();
    assert_eq!(seed.len(), 64);
    assert!(!format!("{store:?}").contains(&seed));
    // Damage the check line: the error must not echo the file.
    std::fs::write(&path, text.replace("check=", "check=00")).unwrap();
    let error = store.sign(b"message").unwrap_err();
    for shown in [format!("{error}"), format!("{error:?}")] {
        assert!(!shown.contains(&seed[..16]), "{shown}");
    }
}
