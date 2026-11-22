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
