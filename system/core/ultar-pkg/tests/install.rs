//! Signature gate of `.uap` packages: only packages signed by a trusted key are installed.

use std::path::{Path, PathBuf};

struct Env {
    dir: tempfile::TempDir,
    private_key: String,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let (private_key, public_key) = ultar_pkg::keygen();
    std::fs::create_dir_all(dir.path().join("trusted")).unwrap();
    std::fs::write(dir.path().join("trusted/publisher.ed25519"), public_key).unwrap();
    Env { dir, private_key }
}

fn build(e: &Env, id: &str, key: &str) -> PathBuf {
    let src = e.dir.path().join("src");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("hello.txt"), b"hello app").unwrap();
    let manifest = e.dir.path().join("manifest.toml");
    let text = format!("id = \"{id}\"\nname = \"Test\"\nversion = \"1.0.0\"\nkind = \"data\"\npermissions = [\"network\"]\n");
    std::fs::write(&manifest, text).unwrap();
    let out = e.dir.path().join("out.uap");
    ultar_pkg::pack_and_sign(&manifest, &src, &out, key).unwrap();
    out
}

fn trusted(e: &Env) -> PathBuf {
    e.dir.path().join("trusted")
}

fn is_empty_or_missing(dir: &Path) -> bool {
    std::fs::read_dir(dir)
        .map(|mut d| d.next().is_none())
        .unwrap_or(true)
}

#[test]
fn trusted_package_installs_under_the_given_root() {
    let e = env();
    let uap = build(&e, "com.example.app", &e.private_key);
    let root = e.dir.path().join("root");
    let m = ultar_pkg::install_at(&uap, &trusted(&e), &root).unwrap();
    assert_eq!(
        (m.id.as_str(), m.version.as_str()),
        ("com.example.app", "1.0.0")
    );
    assert_eq!(m.permissions, vec!["network"]);
    let file = root.join("data/ultar/apps/com.example.app/hello.txt");
    assert_eq!(std::fs::read(file).unwrap(), b"hello app");
    assert!(root
        .join("data/ultar/installed/com.example.app.toml")
        .exists());
    assert_eq!(
        ultar_pkg::verify(&uap, &trusted(&e)).unwrap().id,
        "com.example.app"
    );
    assert_eq!(ultar_pkg::info(&uap).unwrap().name, "Test");
}

#[test]
fn package_signed_with_an_unknown_key_is_rejected_and_nothing_is_written() {
    let e = env();
    let (other_private, _) = ultar_pkg::keygen();
    let uap = build(&e, "com.example.app", &other_private);
    let root = e.dir.path().join("root");
    assert!(ultar_pkg::install_at(&uap, &trusted(&e), &root).is_err());
    assert!(ultar_pkg::verify(&uap, &trusted(&e)).is_err());
    assert!(is_empty_or_missing(&root));
}

#[test]
fn tampered_package_is_rejected() {
    let e = env();
    let uap = build(&e, "com.example.app", &e.private_key);
    let mut bytes = std::fs::read(&uap).unwrap();
    let pos = bytes.windows(4).position(|w| w == b"1.0.").unwrap();
    bytes[pos] = b'9';
    std::fs::write(&uap, &bytes).unwrap();
    let root = e.dir.path().join("root");
    assert!(ultar_pkg::install_at(&uap, &trusted(&e), &root).is_err());
    assert!(is_empty_or_missing(&root));
}

#[test]
fn no_trusted_keys_means_no_installation() {
    let e = env();
    let uap = build(&e, "com.example.app", &e.private_key);
    let empty = e.dir.path().join("no-keys");
    std::fs::create_dir_all(&empty).unwrap();
    let root = e.dir.path().join("root");
    assert!(ultar_pkg::install_at(&uap, &empty, &root).is_err());
    assert!(ultar_pkg::install_at(&uap, &e.dir.path().join("absent"), &root).is_err());
    assert!(is_empty_or_missing(&root));
}

#[test]
fn garbage_files_and_path_like_ids_are_rejected() {
    let e = env();
    let garbage = e.dir.path().join("garbage.uap");
    std::fs::write(&garbage, b"this is not a tar archive at all").unwrap();
    let root = e.dir.path().join("root");
    assert!(ultar_pkg::install_at(&garbage, &trusted(&e), &root).is_err());
    assert!(ultar_pkg::install_at(&e.dir.path().join("missing.uap"), &trusted(&e), &root).is_err());

    // Even a correctly signed package may not choose a directory outside the app area.
    let evil = build(&e, "../../outside", &e.private_key);
    assert!(ultar_pkg::install_at(&evil, &trusted(&e), &root).is_err());
    assert!(is_empty_or_missing(&root));
    assert!(!e.dir.path().join("outside").exists());

    for good in ["com.kmcc.pos", "a", "app_1-2"] {
        assert!(ultar_pkg::valid_id(good), "{good}");
    }
    for bad in ["", "..", "a/b", "../x", ".hidden", "a b"] {
        assert!(!ultar_pkg::valid_id(bad), "{bad}");
    }
}
