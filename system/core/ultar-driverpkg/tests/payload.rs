//! Payload building and unpacking on disk.

mod common;

use common::{manifest_text, TempDir};
use flate2::write::GzEncoder;
use ultar_driverpkg::package::{self, assemble, Limits, PackageError, RawPackage};
use ultar_driverpkg::payload::{build_payload, build_payload_from, list_payload, unpack_payload};
use ultar_driverpkg::sign::{package_message, SignatureBlob};
use ultar_driverpkg::{PrivateKey, TrustedKeys};

/// A correctly signed package around an arbitrary payload.
fn signed_with_payload(payload: Vec<u8>, key: &PrivateKey) -> Vec<u8> {
    let manifest = manifest_text("drv", "1.0.0", 10, "pci = [\"8086:0d4c\"]").into_bytes();
    let blob = SignatureBlob::create(&package_message(&manifest, &payload), key);
    let signature = Some(blob.encode().into_bytes());
    assemble(&RawPackage {
        manifest,
        payload,
        signature,
    })
    .unwrap()
}

#[test]
fn builds_payload_from_directory() {
    let dir = TempDir::new("payload");
    std::fs::create_dir_all(dir.join("src/drivers")).unwrap();
    std::fs::create_dir_all(dir.join("src/firmware/intel")).unwrap();
    std::fs::write(dir.join("src/drivers/drv.uko"), b"module").unwrap();
    std::fs::write(dir.join("src/firmware/intel/fw.bin"), b"firmware").unwrap();
    let payload = build_payload(&dir.join("src")).unwrap();
    assert_eq!(payload, build_payload(&dir.join("src")).unwrap());
    let paths: Vec<String> = list_payload(&payload, &Limits::default())
        .unwrap()
        .into_iter()
        .map(|e| e.path)
        .collect();
    assert_eq!(
        paths,
        vec![
            "drivers/drv.uko".to_string(),
            "firmware/intel/fw.bin".to_string()
        ]
    );
    let out = dir.join("out");
    unpack_payload(&payload, &out, &Limits::default()).unwrap();
    assert_eq!(
        std::fs::read(out.join("firmware/intel/fw.bin")).unwrap(),
        b"firmware"
    );
    let missing = build_payload(&dir.join("does-not-exist")).unwrap_err();
    assert!(matches!(
        missing,
        PackageError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        }
    ));
    assert!(missing.to_string().starts_with("driverpkg.package.io: "));
}

#[cfg(unix)]
#[test]
fn links_are_never_followed() {
    let dir = TempDir::new("links");
    // A symlink in the source directory is refused when packing.
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", dir.join("src/link")).unwrap();
    assert_eq!(
        build_payload(&dir.join("src")).unwrap_err(),
        PackageError::UnsafeEntryType("link".into())
    );
    // A symlink already present in the target directory is not written through.
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("out/drivers")).unwrap();
    let payload = build_payload_from(&[("drivers/drv.uko".to_string(), b"x".to_vec())]).unwrap();
    assert_eq!(
        unpack_payload(&payload, &dir.join("out"), &Limits::default()).unwrap_err(),
        PackageError::UnsafeEntryType("drivers/drv.uko".into())
    );
    assert!(!dir.join("elsewhere/drv.uko").exists());
}

#[test]
fn directory_entries_are_accepted() {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Directory);
    header.set_size(0);
    header.set_mode(0o755);
    header.set_cksum();
    builder
        .append_data(&mut header, "drivers/", &[][..])
        .unwrap();
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "drivers/drv.uko", &b"x"[..])
        .unwrap();
    let payload = builder.into_inner().unwrap().finish().unwrap();
    let dir = TempDir::new("dirs");
    let entries = unpack_payload(&payload, dir.path(), &Limits::default()).unwrap();
    assert!(entries[0].is_dir && !entries[1].is_dir);
    assert!(dir.join("drivers").is_dir());
    let key = PrivateKey::generate();
    let keys = TrustedKeys::from(key.public());
    assert!(package::verify(
        &signed_with_payload(payload, &key),
        &keys,
        &Limits::default()
    )
    .is_ok());
}
