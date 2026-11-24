use super::*;
use ed25519_dalek::SigningKey;
use std::path::PathBuf;

fn unique_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static N: AtomicU32 = AtomicU32::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "ultar-store-sig-{}-{}-{}",
        tag,
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&p);
    p
}

/// A key directory with the public key of `signing`, mode 0755/0644.
fn key_dir(signing: &SigningKey) -> PathBuf {
    let dir = unique_dir("keys");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("vendor.ed25519"),
        hex::encode(signing.verifying_key().to_bytes()) + "\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        let file = dir.join("vendor.ed25519");
        std::fs::set_permissions(file, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    dir
}

#[test]
fn message_binds_the_domain() {
    let m = catalog_message(b"body");
    let text = String::from_utf8(m.clone()).unwrap();
    assert!(text.starts_with("ultar-catalog-v1\n"));
    assert_eq!(text.lines().count(), 2);
    let mut h = Sha256::new();
    h.update(b"body");
    let pkg = format!("ultar-pkg-v1\n{}\n", hex::encode(h.finalize())).into_bytes();
    assert_ne!(m, pkg);
}

#[test]
fn sign_verify_round_trip_and_tamper() {
    let signing = SigningKey::from_bytes(&[7u8; 32]);
    let body = b"format_version = 1\n";
    let blob = sign_catalog(body, &signing);
    let trusted = key_dir(&signing);
    assert_eq!(
        verify_signature(body, &blob, &trusted).unwrap(),
        Some(key_id(&signing.verifying_key()))
    );
    assert_eq!(
        verify_signature(b"format_version = 2\n", &blob, &trusted),
        Err(SourceError::UnknownSigner)
    );
    let other = unique_dir("empty");
    std::fs::create_dir_all(&other).unwrap();
    assert_eq!(
        verify_signature(body, &blob, &other),
        Err(SourceError::UnknownSigner)
    );
    std::fs::remove_dir_all(&trusted).unwrap();
    std::fs::remove_dir_all(&other).unwrap();
}

#[test]
fn signer_is_the_verifying_key_not_the_claimed_id() {
    let signing = SigningKey::from_bytes(&[3u8; 32]);
    let body = b"x = 1\n";
    let blob = sign_catalog(body, &signing);
    let sig_line = blob.lines().nth(1).unwrap();
    let forged = format!("ffffffffffffffff\n{sig_line}");
    let trusted = key_dir(&signing);
    assert_eq!(
        verify_signature(body, &forged, &trusted).unwrap(),
        Some(key_id(&signing.verifying_key()))
    );
    std::fs::remove_dir_all(&trusted).unwrap();
}

#[test]
fn package_domain_signature_is_refused() {
    use ed25519_dalek::Signer;
    let signing = SigningKey::from_bytes(&[5u8; 32]);
    let body = b"x = 1\n";
    let mut h = Sha256::new();
    h.update(body);
    let pkg_msg = format!("ultar-pkg-v1\n{}\n", hex::encode(h.finalize()));
    let blob = signature_blob(&signing.verifying_key(), &signing.sign(pkg_msg.as_bytes()));
    let trusted = key_dir(&signing);
    assert_eq!(
        verify_signature(body, &blob, &trusted),
        Err(SourceError::UnknownSigner)
    );
    std::fs::remove_dir_all(&trusted).unwrap();
}

#[cfg(unix)]
#[test]
fn writable_key_files_and_dirs_are_not_trusted() {
    use std::os::unix::fs::PermissionsExt;
    let signing = SigningKey::from_bytes(&[11u8; 32]);
    let body = b"x = 1\n";
    let blob = sign_catalog(body, &signing);
    let trusted = key_dir(&signing);
    let file = trusted.join("vendor.ed25519");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o666)).unwrap();
    assert_eq!(
        verify_signature(body, &blob, &trusted),
        Err(SourceError::UnknownSigner)
    );
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(&trusted, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert_eq!(
        verify_signature(body, &blob, &trusted),
        Err(SourceError::UnknownSigner)
    );
    std::fs::set_permissions(&trusted, std::fs::Permissions::from_mode(0o755)).unwrap();
    // A symlinked key file is skipped too.
    let elsewhere = unique_dir("link-target");
    std::fs::create_dir_all(&elsewhere).unwrap();
    std::fs::rename(&file, elsewhere.join("k")).unwrap();
    std::os::unix::fs::symlink(elsewhere.join("k"), &file).unwrap();
    assert_eq!(
        verify_signature(body, &blob, &trusted),
        Err(SourceError::UnknownSigner)
    );
    std::fs::remove_dir_all(&trusted).unwrap();
    std::fs::remove_dir_all(&elsewhere).unwrap();
}

#[test]
fn revoked_key_is_no_longer_trusted() {
    let signing = SigningKey::from_bytes(&[11u8; 32]);
    let dir = key_dir(&signing);
    let blob = sign_catalog(b"body", &signing);
    assert!(verify_signature(b"body", &blob, &dir).unwrap().is_some());
    let line = format!(
        "revoke {}\n",
        hex::encode(signing.verifying_key().to_bytes())
    );
    std::fs::write(dir.join("revoked.list"), &line).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let f = dir.join("revoked.list");
        std::fs::set_permissions(f, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    assert_eq!(
        verify_signature(b"body", &blob, &dir),
        Err(SourceError::UnknownSigner)
    );
    // A revoke line is never a key by itself.
    assert!(trusted_keys(&dir).is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
