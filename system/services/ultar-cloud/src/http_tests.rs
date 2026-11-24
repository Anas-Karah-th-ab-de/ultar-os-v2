//! Unit tests of the transport helpers (no network).

use super::*;

const HELLO_SHA: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn resolve_marks_only_the_configured_server_as_ours() {
    let t = Transport::new("https://ultar.example/", Some("tok".into()));
    assert_eq!(t.base_url(), "https://ultar.example");
    assert_eq!(
        t.resolve("/v1/x"),
        ("https://ultar.example/v1/x".to_string(), true)
    );
    assert_eq!(
        t.resolve("v1/x"),
        ("https://ultar.example/v1/x".to_string(), true)
    );
    assert!(t.resolve("https://ultar.example/v1/x").1);
    assert!(t.resolve("https://ultar.example").1);
    for foreign in [
        "https://cdn.example/x",
        "https://ultar.example.evil.test/x",
        "https://ultar.example@evil.test/x",
        "https://ultar.example:8443/x",
        "http://ultar.example/x",
    ] {
        assert!(!t.resolve(foreign).1, "{foreign}");
    }
}

#[test]
fn blank_tokens_count_as_signed_out() {
    assert!(Transport::new("http://x", Some("tok".into())).has_token());
    assert!(!Transport::new("http://x", Some("  ".into())).has_token());
    assert!(!Transport::new("http://x", None).has_token());
}

#[test]
fn verified_download_appears_only_when_it_matches() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.uap");
    write_verified(
        &b"hello"[..],
        Expected {
            size: 5,
            sha256: HELLO_SHA,
        },
        &dest,
    )
    .unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"hello");
    assert_eq!(names(dir.path()), vec!["a.uap"], "no .part file is left");

    let upper = HELLO_SHA.to_uppercase();
    let unknown = dir.path().join("b.uap");
    write_verified(
        &b"hello"[..],
        Expected {
            size: 0,
            sha256: &upper,
        },
        &unknown,
    )
    .unwrap();
    write_verified(
        &b"hello"[..],
        Expected {
            size: 0,
            sha256: "",
        },
        &unknown,
    )
    .unwrap();
}

#[test]
fn mismatches_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.uap");
    let cases = [
        (
            &b"hello"[..],
            Expected {
                size: 4,
                sha256: HELLO_SHA,
            },
        ),
        (
            &b"hello"[..],
            Expected {
                size: 6,
                sha256: HELLO_SHA,
            },
        ),
        (
            &b"hellO"[..],
            Expected {
                size: 5,
                sha256: HELLO_SHA,
            },
        ),
        (
            &b"hello"[..],
            Expected {
                size: 5,
                sha256: "00",
            },
        ),
    ];
    for (bytes, expected) in cases {
        let err = write_verified(bytes, expected, &dest).unwrap_err();
        assert!(matches!(err, CloudError::Integrity), "{expected:?}");
    }
    assert!(names(dir.path()).is_empty());
}

#[test]
fn a_failed_download_does_not_replace_an_existing_good_file() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("a.uap");
    std::fs::write(&dest, b"previous").unwrap();
    let err = write_verified(
        &b"evil"[..],
        Expected {
            size: 5,
            sha256: HELLO_SHA,
        },
        &dest,
    );
    assert!(err.is_err());
    assert_eq!(std::fs::read(&dest).unwrap(), b"previous");
    let missing_dir = dir.path().join("absent/a.uap");
    let err = write_verified(
        &b"hello"[..],
        Expected {
            size: 5,
            sha256: HELLO_SHA,
        },
        &missing_dir,
    );
    assert!(matches!(err, Err(CloudError::Io(_))));
}

#[test]
fn atomic_write_creates_parents_and_private_files() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/token");
    write_atomic(&path, b"secret", true).unwrap();
    write_atomic(&path, b"secret-2", true).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"secret-2");
    assert_eq!(names(&dir.path().join("nested")), vec!["token"]);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let public = dir.path().join("number");
    write_atomic(&public, b"UOS", false).unwrap();
    assert_eq!(std::fs::read(&public).unwrap(), b"UOS");
    assert!(write_atomic(&dir.path().join("number/below-a-file"), b"x", false).is_err());
}

#[test]
fn unreachable_server_is_a_transport_error() {
    // Reserve a free port, then close it again: connecting to it is refused.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let t = Transport::new(&format!("http://{addr}"), None);
    let err = t.get_json("/v1/store/catalog", &[]).unwrap_err();
    assert!(matches!(err, CloudError::Transport(_)), "{err}");
}

#[cfg(unix)]
#[test]
fn private_files_are_never_written_through_planted_names_or_into_open_directories() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let loot = outside.path().join("loot");
    std::fs::write(&loot, "").unwrap();
    let dest = dir.path().join("state/cloud.token");
    // A missing directory of a private file is created for the owner only.
    write_atomic(&dest, b"one", true).unwrap();
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(dest.parent().unwrap()), 0o700);
    assert_eq!(mode(&dest), 0o600);
    // The final name is a link out of the directory: the link is replaced, its target untouched.
    std::fs::remove_file(&dest).unwrap();
    symlink(&loot, &dest).unwrap();
    symlink(&loot, dest.parent().unwrap().join("cloud.token.part")).unwrap();
    write_atomic(&dest, b"two", true).unwrap();
    assert_eq!(std::fs::read_to_string(&loot).unwrap(), "");
    assert!(std::fs::symlink_metadata(&dest)
        .unwrap()
        .file_type()
        .is_file());
    assert_eq!(std::fs::read_to_string(&dest).unwrap(), "two");
    // A directory that others may write to is refused for private files, not for public ones.
    let open = dir.path().join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();
    let refused = write_atomic(&open.join("cloud.token"), b"x", true).unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(!open.join("cloud.token").exists());
    write_atomic(&open.join("number"), b"UOS", false).unwrap();
    let left: Vec<_> = std::fs::read_dir(&open).unwrap().flatten().collect();
    assert_eq!(left.len(), 1, "no temporary file left");
}

#[cfg(unix)]
#[test]
fn old_leftovers_of_interrupted_writes_are_removed_young_ones_kept() {
    // Private directory: `tempdir()` follows the umask (0775 under umask 002), and a private
    // write rightly refuses a group-writable directory.
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .tempdir()
        .unwrap();
    let dest = dir.path().join("device.state");
    let old = dir.path().join("device.state.new.0011223344556677");
    let young = dir.path().join("device.state.new.8899aabbccddeeff");
    let other = dir.path().join("device.state.new.notatemp");
    for p in [&old, &young, &other] {
        std::fs::write(p, "x").unwrap();
    }
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(past)
        .unwrap();
    write_atomic(&dest, b"state", true).unwrap();
    assert!(!old.exists(), "stale temporary file removed");
    assert!(young.exists() && other.exists());
    assert!(fresh_temp_path(&dest).unwrap() != fresh_temp_path(&dest).unwrap());
}

#[cfg(unix)]
#[test]
fn a_download_is_not_written_through_a_planted_part_link() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let loot = outside.path().join("loot");
    std::fs::write(&loot, "").unwrap();
    let dest = dir.path().join("a.uap");
    symlink(&loot, dir.path().join("a.uap.part")).unwrap();
    let body = b"package bytes";
    let expected = Expected {
        size: body.len() as u64,
        sha256: "",
    };
    write_verified(&body[..], expected, &dest).unwrap();
    assert_eq!(std::fs::read_to_string(&loot).unwrap(), "");
    assert_eq!(std::fs::read(&dest).unwrap(), body);
}
