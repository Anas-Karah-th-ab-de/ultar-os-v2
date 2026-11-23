use super::*;
use crate::version::Version;

fn spec() -> Manifest {
    Manifest {
        version: Version::new(1, 2, 3),
        rollback_index: 7,
        channel: "stable".into(),
        edition: "desktop".into(),
        arch: "x86_64".into(),
        devices: vec!["*".into()],
        min_version: Version::new(1, 0, 0),
        notes_key: "update.notes.generic".into(),
        created: 100,
        expires: 200,
        critical: true,
        payloads: Vec::new(),
    }
}

struct Kit {
    dir: tempfile::TempDir,
    files: Vec<PayloadFile>,
    secret: [u8; 32],
}

fn kit() -> Kit {
    let dir = tempfile::tempdir().unwrap();
    let kernel = dir.path().join("kernel.efi");
    let system = dir.path().join("system.img");
    fs::write(&kernel, vec![1u8; 70_000]).unwrap();
    fs::write(&system, vec![2u8; 150_000]).unwrap();
    // Given in the "wrong" order on purpose: the tool sorts by role.
    let files = vec![
        PayloadFile {
            role: Role::System,
            path: system,
            verity_root: Some([9; 32]),
        },
        PayloadFile {
            role: Role::Kernel,
            path: kernel,
            verity_root: None,
        },
    ];
    Kit {
        dir,
        files,
        secret: [3; 32],
    }
}

#[test]
fn manifest_sign_bundle_verify_round_trip() {
    let kit = kit();
    let manifest = build_manifest(spec(), &kit.files).unwrap();
    let parsed = Manifest::parse(&manifest).unwrap();
    assert_eq!(parsed.payloads[0].role, Role::Kernel);
    assert_eq!(parsed.payloads[0].size, 70_000);
    assert_eq!(parsed.payloads[1].verity_root, Some([9; 32]));
    let signed = sign_manifest(&manifest, &kit.secret).unwrap();
    let out = kit.dir.path().join("release.uup");
    let size = make_bundle(&signed, &[], &kit.files, &out).unwrap();
    assert_eq!(size, fs::metadata(&out).unwrap().len());
    assert!(!kit.dir.path().join("release.uup.tmp").exists());
    let verified = verify_bundle(&out, &[public_of(&kit.secret)]).unwrap();
    assert_eq!(verified.manifest(), &parsed);
    assert!(matches!(
        verify_bundle(&out, &[public_of(&[4; 32])]),
        Err(UpdateError::UnknownKey)
    ));
}

#[test]
fn bundle_refuses_payload_files_that_differ_from_the_signed_manifest() {
    let kit = kit();
    let signed = sign_manifest(&build_manifest(spec(), &kit.files).unwrap(), &kit.secret).unwrap();
    let out = kit.dir.path().join("release.uup");
    fs::write(&kit.files[0].path, vec![2u8; 150_001]).unwrap();
    assert!(make_bundle(&signed, &[], &kit.files, &out).is_err());
    assert!(!out.exists() && !kit.dir.path().join("release.uup.tmp").exists());
    fs::write(&kit.files[0].path, vec![5u8; 150_000]).unwrap();
    assert!(matches!(
        make_bundle(&signed, &[], &kit.files, &out),
        Err(UpdateError::HashMismatch(Role::System))
    ));
    fs::write(&kit.files[0].path, vec![2u8; 149_999]).unwrap();
    assert!(matches!(
        make_bundle(&signed, &[], &kit.files, &out),
        Err(UpdateError::Truncated)
    ));
    assert!(matches!(
        make_bundle(&signed, &[], &kit.files[..1], &out),
        Err(UpdateError::PayloadSetMismatch)
    ));
    assert!(!out.exists());
}

#[test]
fn verify_bundle_finds_every_kind_of_damage() {
    let kit = kit();
    let signed = sign_manifest(&build_manifest(spec(), &kit.files).unwrap(), &kit.secret).unwrap();
    let out = kit.dir.path().join("release.uup");
    make_bundle(&signed, &[], &kit.files, &out).unwrap();
    let keys = [public_of(&kit.secret)];
    let good = fs::read(&out).unwrap();
    let mut flipped = good.clone();
    let last = flipped.len() - 1;
    flipped[last] ^= 1;
    fs::write(&out, &flipped).unwrap();
    assert!(matches!(
        verify_bundle(&out, &keys),
        Err(UpdateError::HashMismatch(Role::System))
    ));
    fs::write(&out, &good[..good.len() - 1]).unwrap();
    assert!(matches!(
        verify_bundle(&out, &keys),
        Err(UpdateError::Truncated)
    ));
    let mut longer = good.clone();
    longer.push(0);
    fs::write(&out, &longer).unwrap();
    assert!(matches!(
        verify_bundle(&out, &keys),
        Err(UpdateError::SizeExceeded)
    ));
    let mut bad_sig = good;
    bad_sig[40] ^= 1;
    fs::write(&out, &bad_sig).unwrap();
    assert!(matches!(
        verify_bundle(&out, &keys),
        Err(UpdateError::BadSignature)
    ));
}

#[test]
fn signing_refuses_non_canonical_text() {
    let kit = kit();
    let mut manifest = build_manifest(spec(), &kit.files).unwrap();
    manifest.extend_from_slice(b"\n");
    assert!(sign_manifest(&manifest, &kit.secret).is_err());
    assert!(sign_manifest(b"hello", &kit.secret).is_err());
    let mut bad = spec();
    bad.expires = 1;
    assert!(build_manifest(bad, &kit.files).is_err());
    assert!(build_manifest(spec(), &[]).is_err(), "no payloads");
}

#[test]
fn keys_are_generated_and_stored_safely() {
    let dir = tempfile::tempdir().unwrap();
    let (secret, public) = keygen().unwrap();
    let (other, _) = keygen().unwrap();
    assert_ne!(secret, other);
    assert_eq!(public_of(&secret), public);
    let path = dir.path().join("release.secret");
    write_key_file(&path, &secret, true).unwrap();
    assert!(
        write_key_file(&path, &other, true).is_err(),
        "never overwrites a key"
    );
    assert_eq!(crate::keys::read_secret_file(&path).unwrap(), secret);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    let public_path = dir.path().join("release.ed25519");
    write_key_file(&public_path, &public, false).unwrap();
    let read = read_public_keys(&[public_path.to_str().unwrap()]).unwrap();
    assert_eq!(read, [public]);
}

#[test]
fn rotation_is_built_sorted_and_verifiable_by_a_device() {
    let dir = tempfile::tempdir().unwrap();
    let burned = dir.path().join("keys");
    fs::create_dir_all(&burned).unwrap();
    fs::write(burned.join("a.ed25519"), hex::encode(public_of(&[1; 32]))).unwrap();
    let new_keys = [
        public_of(&[9; 32]),
        public_of(&[8; 32]),
        public_of(&[9; 32]),
    ];
    let signed = make_rotation(1, 50, &new_keys, &[1; 32]).unwrap();
    let mut trust = crate::keys::TrustStore::open(&burned, dir.path()).unwrap();
    assert!(trust.accept(&signed).unwrap());
    assert_eq!(trust.keys().len(), 2);
    assert!(make_rotation(2, 50, &[], &[1; 32]).is_err());
}

#[test]
fn payload_argument_forms() {
    let plain = parse_payload_arg("kernel=/tmp/k.efi").unwrap();
    assert_eq!((plain.role, plain.verity_root), (Role::Kernel, None));
    let root = "ab".repeat(32);
    let with_root = parse_payload_arg(&format!("system=/tmp/s.img:{root}")).unwrap();
    assert_eq!(with_root.path, PathBuf::from("/tmp/s.img"));
    assert_eq!(with_root.verity_root, Some([0xab; 32]));
    // A colon in the path that is not followed by a root hash belongs to the path.
    assert_eq!(
        parse_payload_arg("boot=C:/x.img").unwrap().path,
        PathBuf::from("C:/x.img")
    );
    for bad in [
        "kernel",
        "initrd=/x",
        "kernel=",
        &format!("system=/s:{}", "zz".repeat(32)),
    ] {
        assert!(parse_payload_arg(bad).is_err(), "{bad}");
    }
}

#[test]
fn absurd_time_stamps_are_refused() {
    let now = 1_795_000_000;
    check_times(now, now + 30 * 86_400, now).unwrap();
    check_times(
        now + MAX_CREATED_AHEAD,
        now + MAX_CREATED_AHEAD + MAX_VALIDITY,
        now,
    )
    .unwrap();
    check_times(now - 10 * 86_400, now + 86_400, now).unwrap();
    for (created, expires) in [
        (now * 1000, now * 1000 + 30 * 86_400_000), // milliseconds by mistake
        (now + MAX_CREATED_AHEAD + 1, now + 2 * 86_400),
        (now, now),
        (now, now - 1),
        (now, now + MAX_VALIDITY + 1),
        (now, u64::MAX),
    ] {
        let refused = check_times(created, expires, now);
        assert!(
            matches!(refused, Err(UpdateError::InvalidArgument)),
            "{created} {expires}"
        );
    }
}
