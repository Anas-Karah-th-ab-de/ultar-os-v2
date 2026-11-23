use super::*;
use ed25519_dalek::{Signature, VerifyingKey};
use std::os::unix::fs::PermissionsExt;

fn store() -> (tempfile::TempDir, FileKeyStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FileKeyStore::open_or_create(dir.path().join("sub/device.key")).unwrap();
    (dir, store)
}

#[test]
fn a_key_is_created_once_private_and_signs() {
    let (_dir, store) = store();
    let mode = std::fs::metadata(store.path())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let public = store.public_key().unwrap();
    let signature = store.sign(b"message").unwrap();
    let key = VerifyingKey::from_bytes(&public).unwrap();
    assert!(key
        .verify_strict(b"message", &Signature::from_bytes(&signature))
        .is_ok());
    assert!(key
        .verify_strict(b"other", &Signature::from_bytes(&signature))
        .is_err());
    // Opening again uses the same key; no temporary file is left behind.
    let again = FileKeyStore::open_or_create(store.path()).unwrap();
    assert_eq!(again.public_key().unwrap(), public);
    let names: Vec<_> = std::fs::read_dir(store.path().parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, ["device.key"]);
}

#[test]
fn two_devices_get_different_keys() {
    let (_a, a) = store();
    let (_b, b) = store();
    assert_ne!(a.public_key().unwrap(), b.public_key().unwrap());
}

#[test]
fn creation_never_replaces_an_existing_key() {
    let (_dir, store) = store();
    let public = store.public_key().unwrap();
    // A second creator (another process at the same moment) loses and uses the existing key.
    store.create().unwrap();
    store.create().unwrap();
    assert_eq!(store.public_key().unwrap(), public);
}

#[test]
fn concurrent_first_use_ends_with_one_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("device.key");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
    let threads: Vec<_> = (0..8)
        .map(|_| {
            let (path, barrier) = (path.clone(), barrier.clone());
            std::thread::spawn(move || {
                barrier.wait();
                FileKeyStore::open_or_create(path)
                    .unwrap()
                    .public_key()
                    .unwrap()
            })
        })
        .collect();
    let keys: std::collections::HashSet<[u8; 32]> =
        threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(keys.len(), 1);
}

#[test]
fn a_file_open_to_others_is_refused_and_left_alone() {
    let (_dir, store) = store();
    let before = std::fs::read(store.path()).unwrap();
    for mode in [0o640, 0o604, 0o660, 0o644, 0o602] {
        std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(mode)).unwrap();
        assert!(
            matches!(store.sign(b"m"), Err(KeyStoreError::Insecure(_))),
            "{mode:o}"
        );
        assert!(matches!(
            FileKeyStore::open_or_create(store.path()),
            Err(KeyStoreError::Insecure(_))
        ));
    }
    assert_eq!(std::fs::read(store.path()).unwrap(), before);
    std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o600)).unwrap();
    assert!(store.sign(b"m").is_ok());
    std::fs::set_permissions(store.path(), std::fs::Permissions::from_mode(0o400)).unwrap();
    assert!(store.sign(b"m").is_ok(), "read-only for the owner is fine");
}

#[test]
fn a_file_of_another_user_is_refused() {
    // Changing the owner needs root; as an ordinary user this test cannot make such a file.
    if !rustix::process::geteuid().is_root() {
        return;
    }
    let (_dir, store) = store();
    std::os::unix::fs::chown(store.path(), Some(12_345), None).unwrap();
    assert!(matches!(
        store.sign(b"m"),
        Err(KeyStoreError::Insecure("owned by another user"))
    ));
    assert!(FileKeyStore::open_or_create(store.path()).is_err());
}

#[test]
fn links_and_directories_are_refused() {
    let (dir, store) = store();
    let link = dir.path().join("link.key");
    std::os::unix::fs::symlink(store.path(), &link).unwrap();
    let via_link = FileKeyStore::new(&link);
    assert!(matches!(
        via_link.public_key(),
        Err(KeyStoreError::Insecure(_))
    ));
    assert!(matches!(
        FileKeyStore::open_or_create(&link),
        Err(KeyStoreError::Insecure(_))
    ));
    // A dangling link is "something there": nothing is created through it.
    let dangling = dir.path().join("dangling.key");
    std::os::unix::fs::symlink(dir.path().join("nowhere"), &dangling).unwrap();
    assert!(FileKeyStore::open_or_create(&dangling).is_err());
    assert!(!dir.path().join("nowhere").exists());
    let folder = dir.path().join("folder.key");
    std::fs::create_dir(&folder).unwrap();
    assert!(matches!(
        FileKeyStore::new(&folder).public_key(),
        Err(KeyStoreError::Insecure(_))
    ));
}

fn rewrite(store: &FileKeyStore, edit: impl FnOnce(String) -> String) {
    let text = std::fs::read_to_string(store.path()).unwrap();
    std::fs::write(store.path(), edit(text)).unwrap();
}

#[test]
fn damaged_files_are_detected_never_repaired_or_replaced() {
    let (_dir, store) = store();
    let good = std::fs::read_to_string(store.path()).unwrap();
    let other = encode_file(&SigningKey::from_bytes(&[3; 32]));
    let other_public = other.lines().nth(2).unwrap().to_string();
    let edits: Vec<Box<dyn Fn(String) -> String>> = vec![
        Box::new(|_| String::new()),
        Box::new(|t| t.replace("ultar-device-key-v1", "ultar-device-key-v2")),
        Box::new(|t| t.replacen("seed=", "seed=00", 1)),
        Box::new(|t| t[..t.len() / 2].to_string()),
        Box::new(|t| format!("{t}extra=1\n")),
        Box::new(|t| {
            // flip one hex digit of the seed
            let i = t.find("seed=").unwrap() + 5;
            let c = if &t[i..=i] == "0" { "1" } else { "0" };
            format!("{}{c}{}", &t[..i], &t[i + 1..])
        }),
        Box::new(move |t| {
            // a different public key with the old seed
            let line = t.lines().nth(2).unwrap().to_string();
            t.replace(&line, &other_public)
        }),
        Box::new(|_| "x".repeat(5000)),
        Box::new(|_| "\u{0}\u{1}binary".to_string()),
    ];
    for (i, edit) in edits.iter().enumerate() {
        std::fs::write(store.path(), &good).unwrap();
        rewrite(&store, edit);
        let damaged = std::fs::read(store.path()).unwrap();
        assert!(
            matches!(store.public_key(), Err(KeyStoreError::Corrupt)),
            "edit {i}"
        );
        assert!(
            matches!(store.sign(b"m"), Err(KeyStoreError::Corrupt)),
            "edit {i}"
        );
        assert!(
            FileKeyStore::open_or_create(store.path()).is_err(),
            "edit {i}"
        );
        assert_eq!(
            std::fs::read(store.path()).unwrap(),
            damaged,
            "left as found"
        );
    }
    std::fs::write(store.path(), &good).unwrap();
    assert!(store.sign(b"m").is_ok());
}

#[test]
fn destroy_removes_the_identity_and_the_next_use_makes_a_new_one() {
    let (_dir, store) = store();
    let first = store.public_key().unwrap();
    store.destroy().unwrap();
    store.destroy().unwrap();
    assert!(!store.exists());
    assert!(matches!(store.sign(b"m"), Err(KeyStoreError::Io(_))));
    let second = FileKeyStore::open_or_create(store.path()).unwrap();
    assert_ne!(second.public_key().unwrap(), first);
}

#[test]
fn errors_describe_themselves() {
    let io = KeyStoreError::from(std::io::Error::other("disk"));
    assert!(io.to_string().contains("disk"));
    assert!(std::error::Error::source(&io).is_some());
    assert!(KeyStoreError::Insecure("owned by another user")
        .to_string()
        .contains("another user"));
    assert!(std::error::Error::source(&KeyStoreError::Corrupt).is_none());
    assert!(!KeyStoreError::Random.to_string().is_empty());
}

/// A store that is not a file: the trait is all the rest of the crate needs.
struct MemoryStore(SigningKey);

impl KeyStore for MemoryStore {
    fn public_key(&self) -> Result<[u8; 32], KeyStoreError> {
        Ok(self.0.verifying_key().to_bytes())
    }
    fn sign(&self, message: &[u8]) -> Result<[u8; 64], KeyStoreError> {
        Ok(self.0.sign(message).to_bytes())
    }
}

#[test]
fn the_trait_is_object_safe_and_implementable_without_a_file() {
    let store: Box<dyn KeyStore> = Box::new(MemoryStore(SigningKey::from_bytes(&[1; 32])));
    let public = store.public_key().unwrap();
    let signature = store.sign(b"x").unwrap();
    assert!(VerifyingKey::from_bytes(&public)
        .unwrap()
        .verify_strict(b"x", &Signature::from_bytes(&signature))
        .is_ok());
}

fn age(path: &std::path::Path, secs: u64) {
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(secs);
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(past).unwrap();
}

#[test]
fn leftovers_of_an_interrupted_creation_are_removed_when_they_are_old() {
    let (_dir, store) = store();
    let public = store.public_key().unwrap();
    let parent = store.path().parent().unwrap().to_path_buf();
    // A crash between "link" and "unlink" leaves a second name of the key file behind; a crash
    // before the link leaves an unused key. Both are an hour old.
    let second_name = parent.join("device.key.new.00112233aabbccdd");
    let unused = parent.join("device.key.new.0123456789abcdef");
    let young = parent.join("device.key.new.ffeeddccbbaa0099");
    std::fs::hard_link(store.path(), &second_name).unwrap();
    std::fs::write(&unused, "another key").unwrap();
    std::fs::write(&young, "a creator at work right now").unwrap();
    age(store.path(), 3600);
    age(&unused, 3600);
    let again = FileKeyStore::open_or_create(store.path()).unwrap();
    assert_eq!(again.public_key().unwrap(), public, "the key itself stays");
    assert!(!second_name.exists() && !unused.exists());
    assert!(young.exists(), "a young temporary file may be in use");
}
