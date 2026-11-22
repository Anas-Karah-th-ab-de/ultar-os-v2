//! Package container: signing, verification and hostile payloads.

mod common;

use common::{manifest_text, pack, payload_for, TempDir};
use flate2::write::GzEncoder;
use ultar_driverpkg::package::{self, assemble, Limits, PackageError, Part, RawPackage};
use ultar_driverpkg::payload::{build_payload_from, list_payload, unpack_payload};
use ultar_driverpkg::sign::{package_message, SignatureBlob, SignatureError};
use ultar_driverpkg::{Coded, PrivateKey, TrustedKeys};

fn manifest() -> String {
    manifest_text("drv", "1.0.0", 10, "pci = [\"8086:0d4c\"]")
}

fn trusted(key: &PrivateKey) -> TrustedKeys {
    TrustedKeys::from(key.public())
}

fn verify(bytes: &[u8], keys: &TrustedKeys) -> Result<package::VerifiedPackage, PackageError> {
    package::verify(bytes, keys, &Limits::default())
}

/// A correctly signed package around an arbitrary (possibly hostile) payload.
fn signed_with_payload(payload: Vec<u8>, key: &PrivateKey) -> Vec<u8> {
    let manifest = manifest().into_bytes();
    let blob = SignatureBlob::create(&package_message(&manifest, &payload), key);
    assemble(&RawPackage {
        manifest,
        payload,
        signature: Some(blob.encode().into_bytes()),
    })
    .unwrap()
}

/// A tar.gz with one entry whose name and type are written without the builder's safety checks.
fn hostile_payload(name: &[u8], kind: tar::EntryType, data: &[u8]) -> Vec<u8> {
    let mut builder = tar::Builder::new(GzEncoder::new(Vec::new(), flate2::Compression::default()));
    let mut header = tar::Header::new_gnu();
    header.as_old_mut().name[..name.len()].copy_from_slice(name);
    header.set_entry_type(kind);
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder.append(&header, data).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

#[test]
fn round_trip_and_unpack() {
    let key = PrivateKey::generate();
    let bytes = pack(&manifest(), &key);
    let verified = verify(&bytes, &trusted(&key)).unwrap();
    assert_eq!(verified.manifest.id, "drv");
    assert_eq!(verified.key_id, key.public().key_id());
    assert_eq!(verified.entries.len(), 1);
    assert_eq!(verified.payload(), payload_for(&manifest()).as_slice());
    let dir = TempDir::new("unpack");
    let written = verified.unpack(dir.path()).unwrap();
    assert_eq!(written.len(), 1);
    assert_eq!(
        std::fs::read(dir.join("drivers/drv.uko")).unwrap(),
        b"dummy drivers/drv.uko"
    );
    // Packing is reproducible: fixed metadata and deterministic Ed25519 signatures.
    assert_eq!(bytes, pack(&manifest(), &key));
}

#[test]
fn info_needs_no_trust() {
    let key = PrivateKey::generate();
    let info = package::info(&pack(&manifest(), &key), &Limits::default()).unwrap();
    assert_eq!(info.manifest.id, "drv");
    assert_eq!(info.key_id, Some(key.public().key_id()));
    assert_eq!(info.payload_sha256.len(), 64);
    assert_eq!(info.entries[0].path, "drivers/drv.uko");
    let mut raw = package::read_package(&pack(&manifest(), &key), &Limits::default()).unwrap();
    raw.signature = None;
    assert_eq!(
        package::info(&assemble(&raw).unwrap(), &Limits::default())
            .unwrap()
            .key_id,
        None
    );
}

#[test]
fn unsigned_is_rejected() {
    let key = PrivateKey::generate();
    let mut raw = package::read_package(&pack(&manifest(), &key), &Limits::default()).unwrap();
    raw.signature = None;
    let err = verify(&assemble(&raw).unwrap(), &trusted(&key)).unwrap_err();
    assert_eq!(err, PackageError::Unsigned);
    assert_eq!(err.code(), "driverpkg.package.unsigned");
}

#[test]
fn foreign_key_is_rejected() {
    let (publisher, stranger) = (PrivateKey::generate(), PrivateKey::generate());
    let err = verify(&pack(&manifest(), &stranger), &trusted(&publisher)).unwrap_err();
    assert_eq!(
        err,
        PackageError::Signature(SignatureError::UntrustedKey {
            key_id: stranger.public().key_id()
        })
    );
    assert_eq!(err.code(), "driverpkg.signature.untrusted-key");
}

#[test]
fn tampering_is_rejected() {
    let key = PrivateKey::generate();
    let good = package::read_package(&pack(&manifest(), &key), &Limits::default()).unwrap();
    let invalid = PackageError::Signature(SignatureError::Invalid);

    let mut changed_manifest = good.clone();
    changed_manifest.manifest = manifest().replace("rank = 10", "rank = 99").into_bytes();
    assert_eq!(
        verify(&assemble(&changed_manifest).unwrap(), &trusted(&key)).unwrap_err(),
        invalid
    );

    let mut changed_payload = good.clone();
    changed_payload.payload =
        build_payload_from(&[("drivers/drv.uko".to_string(), b"evil".to_vec())]).unwrap();
    assert_eq!(
        verify(&assemble(&changed_payload).unwrap(), &trusted(&key)).unwrap_err(),
        invalid
    );

    // A foreign signature relabelled with the trusted key id is still "tampered", not accepted.
    let stranger = PrivateKey::generate();
    let mut forged =
        SignatureBlob::create(&package_message(&good.manifest, &good.payload), &stranger);
    forged.key_id = key.public().key_id();
    let mut relabelled = good.clone();
    relabelled.signature = Some(forged.encode().into_bytes());
    assert_eq!(
        verify(&assemble(&relabelled).unwrap(), &trusted(&key)).unwrap_err(),
        invalid
    );
    assert_eq!(invalid.code(), "driverpkg.signature.invalid");

    // A single flipped bit anywhere in the container never verifies.
    let bytes = pack(&manifest(), &key);
    for position in (0..bytes.len()).step_by(97) {
        let mut flipped = bytes.clone();
        flipped[position] ^= 0x01;
        if flipped != bytes {
            let outcome = verify(&flipped, &trusted(&key));
            let padding_only = outcome
                .as_ref()
                .is_ok_and(|v| v.payload() == good.payload.as_slice());
            assert!(
                outcome.is_err() || padding_only,
                "bit flip at {position} was accepted"
            );
        }
    }
}

#[test]
fn malformed_signature_and_no_keys() {
    let key = PrivateKey::generate();
    let mut raw = package::read_package(&pack(&manifest(), &key), &Limits::default()).unwrap();
    assert_eq!(
        verify(&assemble(&raw).unwrap(), &TrustedKeys::new()).unwrap_err(),
        PackageError::Signature(SignatureError::NoTrustedKeys)
    );
    raw.signature = Some(b"garbage".to_vec());
    assert_eq!(
        verify(&assemble(&raw).unwrap(), &trusted(&key)).unwrap_err(),
        PackageError::Signature(SignatureError::Malformed)
    );
}

#[test]
fn container_structure_errors() {
    let key = PrivateKey::generate();
    let limits = Limits::default();
    assert_eq!(
        package::read_package(b"this is not a tar archive at all, sorry", &limits),
        Err(PackageError::Archive)
    );
    assert_eq!(
        package::read_package(&[], &limits),
        Err(PackageError::MissingManifest)
    );

    let mut builder = tar::Builder::new(Vec::new());
    let mut add = |name: &str, data: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
    };
    add(package::MANIFEST_NAME, manifest().as_bytes());
    let only_manifest = builder.into_inner().unwrap();
    assert_eq!(
        package::read_package(&only_manifest, &limits),
        Err(PackageError::MissingPayload)
    );

    let mut builder = tar::Builder::new(Vec::new());
    let mut add = |name: &str, data: &[u8]| {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_cksum();
        builder.append_data(&mut header, name, data).unwrap();
    };
    add(package::MANIFEST_NAME, b"a");
    add(package::MANIFEST_NAME, b"b");
    let duplicate = builder.into_inner().unwrap();
    assert_eq!(
        package::read_package(&duplicate, &limits),
        Err(PackageError::DuplicateEntry(package::MANIFEST_NAME.into()))
    );

    let mut builder = tar::Builder::new(Vec::new());
    let mut header = tar::Header::new_gnu();
    header.set_size(1);
    header.set_cksum();
    builder
        .append_data(&mut header, "install.sh", &b"x"[..])
        .unwrap();
    let extra = builder.into_inner().unwrap();
    assert_eq!(
        package::read_package(&extra, &limits),
        Err(PackageError::UnexpectedEntry("install.sh".into()))
    );

    let broken = signed_with_payload(b"not gzip".to_vec(), &key);
    assert_eq!(
        verify(&broken, &trusted(&key)).unwrap_err(),
        PackageError::PayloadCorrupt
    );
}

#[test]
fn size_limits() {
    let key = PrivateKey::generate();
    let bytes = pack(&manifest(), &key);
    let tiny = |change: fn(&mut Limits)| {
        let mut limits = Limits::default();
        change(&mut limits);
        limits
    };
    let too_large = |part, limit| PackageError::TooLarge { part, limit };
    let limits = tiny(|l| l.max_manifest = 10);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        too_large(Part::Manifest, 10)
    );
    assert_eq!(
        package::pack(
            manifest().as_bytes(),
            &payload_for(&manifest()),
            &key,
            &limits
        )
        .unwrap_err(),
        too_large(Part::Manifest, 10)
    );
    let limits = tiny(|l| l.max_payload = 10);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        too_large(Part::Payload, 10)
    );
    assert_eq!(
        package::pack(
            manifest().as_bytes(),
            &payload_for(&manifest()),
            &key,
            &limits
        )
        .unwrap_err(),
        too_large(Part::Payload, 10)
    );
}

#[test]
fn unpack_limits() {
    let key = PrivateKey::generate();
    let bytes = pack(&manifest(), &key);
    let tiny = |change: fn(&mut Limits)| {
        let mut limits = Limits::default();
        change(&mut limits);
        limits
    };
    let too_large = |part, limit| PackageError::TooLarge { part, limit };
    let limits = tiny(|l| l.max_signature = 10);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        too_large(Part::Signature, 10)
    );
    let limits = tiny(|l| l.max_unpacked_file = 5);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        too_large(Part::UnpackedFile, 5)
    );
    let limits = tiny(|l| l.max_unpacked_total = 5);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        too_large(Part::UnpackedTotal, 5)
    );
    let limits = tiny(|l| l.max_entries = 0);
    assert_eq!(
        package::verify(&bytes, &trusted(&key), &limits).unwrap_err(),
        PackageError::TooManyEntries { limit: 0 }
    );
    assert_eq!(
        too_large(Part::Payload, 10).to_string(),
        "driverpkg.package.too-large: Payload > 10"
    );
}

#[test]
fn decompression_bomb_is_stopped() {
    // 64 MiB of zeros compress to a few kilobytes; the unpack limit stops them.
    let files = vec![("drivers/drv.uko".to_string(), vec![0u8; 64 * 1024 * 1024])];
    let bomb = build_payload_from(&files).unwrap();
    assert!(bomb.len() < 1024 * 1024);
    let limits = Limits {
        max_unpacked_file: 1024 * 1024,
        max_unpacked_total: 2 * 1024 * 1024,
        ..Limits::default()
    };
    assert_eq!(
        list_payload(&bomb, &limits),
        Err(PackageError::TooLarge {
            part: Part::UnpackedFile,
            limit: 1024 * 1024
        })
    );
    // A header that lies about a small size while a long-name record is huge trips the byte cap.
    let long_name = hostile_payload(
        b"././@LongLink",
        tar::EntryType::GNULongName,
        &vec![b'a'; 8 * 1024 * 1024],
    );
    let limits = Limits {
        max_unpacked_total: 1024,
        max_entries: 4,
        ..Limits::default()
    };
    assert_eq!(
        list_payload(&long_name, &limits),
        Err(PackageError::TooLarge {
            part: Part::UnpackedTotal,
            limit: 1024
        })
    );
}

#[test]
fn hostile_paths_and_types_are_rejected() {
    let key = PrivateKey::generate();
    let keys = trusted(&key);
    let reject = |name: &[u8], kind| {
        verify(
            &signed_with_payload(hostile_payload(name, kind, b"x"), &key),
            &keys,
        )
    };
    let file = tar::EntryType::Regular;
    assert_eq!(
        reject(b"../escape", file).unwrap_err(),
        PackageError::PathTraversal("../escape".into())
    );
    assert_eq!(
        reject(b"a/../../escape", file).unwrap_err(),
        PackageError::PathTraversal("a/../../escape".into())
    );
    assert_eq!(
        reject(b"/etc/passwd", file).unwrap_err(),
        PackageError::AbsolutePath("/etc/passwd".into())
    );
    assert_eq!(
        reject(b"a\\b", file).unwrap_err(),
        PackageError::BadPath("a\\b".into())
    );
    assert!(matches!(
        reject(&[b'a', 0xff, b'b'], file).unwrap_err(),
        PackageError::BadPath(_)
    ));
    for kind in [
        tar::EntryType::Symlink,
        tar::EntryType::Link,
        tar::EntryType::Char,
        tar::EntryType::Fifo,
    ] {
        assert_eq!(
            reject(b"drivers/drv.uko", kind).unwrap_err(),
            PackageError::UnsafeEntryType("drivers/drv.uko".into())
        );
    }
    // Nothing is written when validation fails.
    let dir = TempDir::new("hostile");
    let payload = hostile_payload(b"../escape", file, b"x");
    assert!(unpack_payload(&payload, &dir.join("out"), &Limits::default()).is_err());
    assert!(!dir.join("out").exists());
    assert!(!dir.join("escape").exists());
}

#[test]
fn content_must_be_in_payload() {
    let key = PrivateKey::generate();
    let other = build_payload_from(&[("drivers/other.uko".to_string(), b"x".to_vec())]).unwrap();
    let missing = PackageError::MissingContent("drivers/drv.uko".into());
    assert_eq!(
        package::pack(manifest().as_bytes(), &other, &key, &Limits::default()).unwrap_err(),
        missing
    );
    assert_eq!(
        verify(&signed_with_payload(other, &key), &trusted(&key)).unwrap_err(),
        missing
    );
    let bad_manifest = package::pack(
        b"id = 1",
        &payload_for(&manifest()),
        &key,
        &Limits::default(),
    )
    .unwrap_err();
    assert!(matches!(bad_manifest, PackageError::Manifest(_)));
    assert_eq!(bad_manifest.code(), "driverpkg.manifest.syntax");
}
