use super::*;

pub(crate) fn sample() -> Manifest {
    Manifest {
        version: Version::new(0, 7, 1),
        rollback_index: 3,
        channel: "stable".into(),
        edition: "desktop".into(),
        arch: "x86_64".into(),
        devices: vec!["beelink-gti15".into(), "pc-uefi".into()],
        min_version: Version::new(0, 7, 0),
        notes_key: "update.notes.v0_7_1".into(),
        created: 1_795_400_000,
        expires: 1_798_000_000,
        critical: false,
        payloads: vec![
            Payload {
                role: Role::Kernel,
                size: 1024,
                sha256: [0x11; 32],
                verity_root: None,
            },
            Payload {
                role: Role::System,
                size: 4096,
                sha256: [0x22; 32],
                verity_root: Some([0x33; 32]),
            },
        ],
    }
}

#[test]
fn round_trip_is_byte_exact() {
    let m = sample();
    let bytes = m.encode().unwrap();
    assert_eq!(Manifest::parse(&bytes).unwrap(), m);
    let text = String::from_utf8(bytes).unwrap();
    assert!(text.starts_with("ultar-update-manifest 1\nversion=0.7.1\nrollback_index=3\n"));
    assert!(text.contains("devices=beelink-gti15,pc-uefi\n"));
    assert!(text.ends_with(&format!(
        "payload=system 4096 {} {}\n",
        "22".repeat(32),
        "33".repeat(32)
    )));
}

#[test]
fn wildcard_device_and_lookup() {
    let mut m = sample();
    assert!(m.allows_device("pc-uefi"));
    assert!(!m.allows_device("a30"));
    assert!(!m.allows_device("*"));
    m.devices = vec!["*".into()];
    let parsed = Manifest::parse(&m.encode().unwrap()).unwrap();
    assert!(parsed.allows_device("a30"));
    assert_eq!(parsed.payload(Role::System).unwrap().size, 4096);
    assert!(parsed.payload(Role::Boot).is_none());
    assert_eq!(parsed.payload_bytes(), 5120);
}

#[test]
fn every_non_canonical_variant_is_refused() {
    let good = String::from_utf8(sample().encode().unwrap()).unwrap();
    let variants: Vec<String> = vec![
        good.replace("version=0.7.1", "version=0.07.1"),
        good.replace("rollback_index=3", "rollback_index=03"),
        good.replace("rollback_index=3", "rollback_index=+3"),
        good.replace("critical=0", "critical=2"),
        good.replace("critical=0", "critical=false"),
        good.replace('\n', "\r\n"),
        good.trim_end().to_string(),
        format!("{good}\n"),
        format!("{good}extra=1\n"),
        format!(" {good}"),
        good.replace("channel=stable\n", ""),
        good.replace("channel=stable\n", "channel=stable\nchannel=beta\n"),
        good.replace(
            "channel=stable\nedition=desktop\n",
            "edition=desktop\nchannel=stable\n",
        ),
        good.replace(
            "devices=beelink-gti15,pc-uefi",
            "devices=pc-uefi,beelink-gti15",
        ),
        good.replace("devices=beelink-gti15,pc-uefi", "devices=pc-uefi,pc-uefi"),
        good.replace("devices=beelink-gti15,pc-uefi", "devices="),
        good.replace("devices=beelink-gti15,pc-uefi", "devices=*,pc-uefi"),
        good.replace("devices=beelink-gti15,pc-uefi", "devices=../etc"),
        good.replace("payload=kernel 1024", "payload=kernel 0"),
        good.replace("payload=kernel 1024", "payload=kernel 01024"),
        good.replace("payload=kernel 1024", "payload=kernel 68719476737"),
        good.replace("payload=kernel 1024", "payload=initrd 1024"),
        good.replace("payload=kernel", "payload=system"),
        good.replace(&"11".repeat(32), &"1G".repeat(32)),
        good.replace(&"11".repeat(32), &"AA".repeat(32)),
        good.replace(&"11".repeat(32), "11"),
        good.replace(" -\n", "\n"),
        good.replace(" -\n", " - x\n"),
        good.replace("expires=1798000000", "expires=1"),
        good.replace("min_version=0.7.0", "min_version=0.8.0"),
        good.replace("notes_key=update.notes.v0_7_1", "notes_key=Release notes!"),
        good.replace("ultar-update-manifest 1", "ultar-update-manifest 2"),
        good.replace("edition=desktop", "edition=desk top"),
        good.replace("arch=x86_64", &format!("arch={}", "a".repeat(33))),
    ];
    for (index, text) in variants.iter().enumerate() {
        assert_ne!(text, &good, "variant {index} did not change anything");
        assert!(
            Manifest::parse(text.as_bytes()).is_err(),
            "variant {index}: {text}"
        );
    }
}

#[test]
fn structural_limits() {
    let mut m = sample();
    m.payloads.clear();
    assert_eq!(m.encode(), Err(Malformed::Field));
    let mut m = sample();
    m.payloads.swap(0, 1);
    assert_eq!(m.encode(), Err(Malformed::Field));
    let mut m = sample();
    m.devices = (0..=MAX_DEVICES).map(|i| format!("d{i:03}")).collect();
    assert_eq!(m.encode(), Err(Malformed::Field));
    m.devices.pop();
    assert!(m.encode().is_ok());
    assert_eq!(
        Manifest::parse(&vec![b'a'; MAX_MANIFEST + 1]),
        Err(Malformed::Length)
    );
    assert_eq!(Manifest::parse(b"\xff\xfe"), Err(Malformed::Text));
    assert_eq!(Manifest::parse(b""), Err(Malformed::Magic));
}

#[test]
fn identifiers() {
    for good in ["stable", "x86_64", "a", "0.7+b", "A-b_c.d"] {
        assert!(valid_ident(good), "{good}");
    }
    for bad in [
        "",
        ".a",
        "-a",
        "a..b",
        "a/b",
        "a b",
        "ä",
        &"a".repeat(33),
        "a\n",
    ] {
        assert!(!valid_ident(bad), "{bad:?}");
    }
    assert_eq!(Role::from_name("boot"), Some(Role::Boot));
    assert_eq!(Role::from_name("Boot"), None);
}
