//! Depot: signed index, lookup, resolve and install selection.

mod common;

use common::{example_manifests, index_of, manifest_text, pack, TempDir};
use ultar_driverpkg::depot::{load_signed_index, scan_directory, write_signed_index};
use ultar_driverpkg::index::{
    DriverIndex, IndexEntry, IndexError, INDEX_NAME, INDEX_SIGNATURE_NAME,
};
use ultar_driverpkg::package::{Limits, PackageError};
use ultar_driverpkg::sign::SignatureError;
use ultar_driverpkg::{
    resolve, select_for_install, Coded, DeviceDescriptor, HardwareId, PrivateKey, TrustedKeys,
};
use ultar_driverpkg::{Manifest, Version};

const ABI: &str = "ultar-kern-abi-1";

fn pci(address: &str, vendor: u16, device: u16, class: u32) -> DeviceDescriptor {
    DeviceDescriptor::new(
        address,
        HardwareId::Pci {
            vendor,
            device,
            class,
            subsys: None,
        },
    )
}

fn write_depot(dir: &TempDir, manifests: &[String], key: &PrivateKey) {
    for text in manifests {
        let id = Manifest::parse(text).unwrap().id;
        std::fs::write(dir.join(&format!("{id}.utr")), pack(text, key)).unwrap();
    }
}

#[test]
fn index_text_round_trip_is_stable() {
    let key = PrivateKey::generate();
    let index = index_of(&example_manifests(), &key);
    assert_eq!(index.len(), 10);
    assert!(!index.is_empty());
    let text = index.to_toml().unwrap();
    let again = DriverIndex::parse(&text).unwrap();
    assert_eq!(again.entries(), index.entries());
    assert_eq!(again.to_toml().unwrap(), text);
    let entry = index.find("gen-ixgbe", &Version::new(1, 0, 0)).unwrap();
    assert_eq!(entry.file, "gen-ixgbe-1.0.0.utr");
    assert_eq!(entry.key_id, key.public().key_id());
    assert!(index.find("gen-ixgbe", &Version::new(2, 0, 0)).is_none());
    assert!(index.get(index.len()).is_none());
    let ids: Vec<&str> = index
        .entries()
        .iter()
        .map(|e| e.manifest.id.as_str())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted);
}

#[test]
fn signed_index_on_disk() {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let dir = TempDir::new("depot");
    write_depot(&dir, &example_manifests(), &key);
    std::fs::write(dir.join("README.txt"), b"ignored").unwrap();
    let built = write_signed_index(dir.path(), &key, &trusted, &Limits::default()).unwrap();
    assert_eq!(built.len(), 10);
    let loaded = load_signed_index(dir.path(), &trusted).unwrap();
    assert_eq!(loaded.entries(), built.entries());
    // Every package file is exactly the one recorded in the signed index.
    for entry in loaded.entries() {
        let bytes = std::fs::read(dir.join(&entry.file)).unwrap();
        assert_eq!(entry.check_file(&bytes), Ok(()));
        assert_eq!(
            entry.check_file(&bytes[1..]),
            Err(IndexError::FileMismatch(entry.file.clone()))
        );
    }
    // Re-running the scan gives byte-identical output.
    let before = std::fs::read(dir.join(INDEX_NAME)).unwrap();
    write_signed_index(dir.path(), &key, &trusted, &Limits::default()).unwrap();
    assert_eq!(std::fs::read(dir.join(INDEX_NAME)).unwrap(), before);
}

#[test]
fn index_signature_is_enforced() {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let dir = TempDir::new("depot-sig");
    write_depot(&dir, &example_manifests()[..2], &key);
    write_signed_index(dir.path(), &key, &trusted, &Limits::default()).unwrap();

    let stranger = TrustedKeys::from(PrivateKey::generate().public());
    let err = load_signed_index(dir.path(), &stranger).unwrap_err();
    assert_eq!(
        err,
        IndexError::Signature(SignatureError::UntrustedKey {
            key_id: key.public().key_id()
        })
    );
    assert_eq!(err.code(), "driverpkg.signature.untrusted-key");
    assert_eq!(
        load_signed_index(dir.path(), &TrustedKeys::new()).unwrap_err(),
        IndexError::Signature(SignatureError::NoTrustedKeys)
    );

    let text = std::fs::read_to_string(dir.join(INDEX_NAME)).unwrap();
    std::fs::write(
        dir.join(INDEX_NAME),
        text.replace("rank = 50", "rank = 999"),
    )
    .unwrap();
    assert_eq!(
        load_signed_index(dir.path(), &trusted).unwrap_err(),
        IndexError::Signature(SignatureError::Invalid)
    );
    std::fs::write(dir.join(INDEX_NAME), &text).unwrap();

    std::fs::write(dir.join(INDEX_SIGNATURE_NAME), b"junk").unwrap();
    assert_eq!(
        load_signed_index(dir.path(), &trusted).unwrap_err(),
        IndexError::Signature(SignatureError::Malformed)
    );
    std::fs::remove_file(dir.join(INDEX_SIGNATURE_NAME)).unwrap();
    assert_eq!(
        load_signed_index(dir.path(), &trusted).unwrap_err(),
        IndexError::Unsigned
    );
    std::fs::remove_file(dir.join(INDEX_NAME)).unwrap();
    assert!(matches!(
        load_signed_index(dir.path(), &trusted).unwrap_err(),
        IndexError::Io { .. }
    ));

    let bad_utf8 = [0xffu8, 0xfe];
    let signature = DriverIndex::sign("x", &key);
    assert!(DriverIndex::parse_verified(b"x", Some(signature.as_bytes()), &trusted).is_err());
    let blob = ultar_driverpkg::sign::SignatureBlob::create(
        &ultar_driverpkg::sign::index_message(&bad_utf8),
        &key,
    );
    let err = DriverIndex::parse_verified(&bad_utf8, Some(blob.encode().as_bytes()), &trusted)
        .unwrap_err();
    assert_eq!(err, IndexError::NotUtf8);
}

#[test]
fn scan_rejects_bad_packages() {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let dir = TempDir::new("depot-bad");
    let manifests = example_manifests();
    write_depot(&dir, &manifests[..1], &key);
    std::fs::write(
        dir.join("foreign.utr"),
        pack(&manifests[1], &PrivateKey::generate()),
    )
    .unwrap();
    let err = scan_directory(dir.path(), &trusted, &Limits::default()).unwrap_err();
    assert!(matches!(
        &err,
        IndexError::Package { file, problem: PackageError::Signature(SignatureError::UntrustedKey { .. }) }
            if file == "foreign.utr"
    ));
    assert!(err
        .to_string()
        .starts_with("driverpkg.index.package: foreign.utr: driverpkg.signature.untrusted-key"));
    std::fs::write(
        dir.join("foreign.utr"),
        b"garbage that is long enough to not be a tar header",
    )
    .unwrap();
    assert!(matches!(
        scan_directory(dir.path(), &trusted, &Limits::default()),
        Err(IndexError::Package { .. })
    ));
    std::fs::remove_file(dir.join("foreign.utr")).unwrap();
    // The same driver in the same version twice.
    std::fs::write(dir.join("copy.utr"), pack(&manifests[0], &key)).unwrap();
    assert!(matches!(
        scan_directory(dir.path(), &trusted, &Limits::default()),
        Err(IndexError::DuplicateDriver { .. })
    ));
    assert!(matches!(
        scan_directory(&dir.join("missing"), &trusted, &Limits::default()),
        Err(IndexError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        })
    ));
}

#[test]
fn index_validation_errors() {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let text = manifest_text("drv", "1.0.0", 0, "platform = [\"x\"]");
    let bytes = pack(&text, &key);
    let entry = |file: &str| IndexEntry::from_package(file, &bytes, &trusted, &Limits::default());
    for bad in [
        "drv.zip",
        "sub/drv.utr",
        "..\\drv.utr",
        ".utr",
        ".hidden.utr",
        "utr",
        "",
    ] {
        assert_eq!(
            entry(bad).unwrap_err(),
            IndexError::BadFileName(bad.to_string()),
            "{bad}"
        );
    }
    let good = entry("drv.utr").unwrap();
    let twice = DriverIndex::new(vec![good.clone(), good.clone()]).unwrap_err();
    assert_eq!(twice, IndexError::DuplicateFile("drv.utr".into()));
    let mut renamed = good.clone();
    renamed.file = "other.utr".into();
    let same_driver = DriverIndex::new(vec![good.clone(), renamed]).unwrap_err();
    assert_eq!(
        same_driver,
        IndexError::DuplicateDriver {
            id: "drv".into(),
            version: "1.0.0".into()
        }
    );
    assert_eq!(
        same_driver.to_string(),
        "driverpkg.index.duplicate-driver: drv 1.0.0"
    );
}

#[test]
fn index_text_errors() {
    let key = PrivateKey::generate();
    let trusted = TrustedKeys::from(key.public());
    let bytes = pack(
        &manifest_text("drv", "1.0.0", 0, "platform = [\"x\"]"),
        &key,
    );
    let good = IndexEntry::from_package("drv.utr", &bytes, &trusted, &Limits::default()).unwrap();
    let toml = DriverIndex::new(vec![good]).unwrap().to_toml().unwrap();
    assert!(matches!(
        DriverIndex::parse("format = "),
        Err(IndexError::Syntax(_))
    ));
    assert_eq!(
        DriverIndex::parse("format = 2").unwrap_err(),
        IndexError::UnsupportedFormat(2)
    );
    assert!(DriverIndex::parse("format = 1").unwrap().is_empty());
    assert!(matches!(
        DriverIndex::parse(&toml.replace("drv.utr", "../drv.utr")),
        Err(IndexError::BadFileName(_))
    ));
    let sha = toml.lines().find(|l| l.starts_with("sha256")).unwrap();
    assert_eq!(
        DriverIndex::parse(&toml.replace(sha, "sha256 = \"abc\"")).unwrap_err(),
        IndexError::BadHash("drv.utr".into())
    );
    assert!(matches!(
        DriverIndex::parse(&toml.replace("class = \"net\"", "class = \"netz\"")),
        Err(IndexError::BadEntry { .. })
    ));
    assert!(matches!(
        DriverIndex::parse(&format!("{toml}\nunknown = 1")),
        Err(IndexError::Syntax(_))
    ));
}

/// Four packages and four devices: one NIC with two drivers, two NVMe drives, one GPU without driver.
fn small_machine() -> (ultar_driverpkg::DriverIndex, Vec<DeviceDescriptor>) {
    let key = PrivateKey::generate();
    let manifests = [
        manifest_text("eth-exact", "1.0.0", 50, "pci = [\"8086:0d4c\"]"),
        manifest_text("eth-class", "1.0.0", 50, "pci_class = [\"0200\"]"),
        manifest_text("nvme", "1.0.0", 50, "pci_class = [\"010802\"]"),
        manifest_text("unused", "1.0.0", 50, "pci = [\"1234:5678\"]"),
    ];
    let index = index_of(&manifests, &key);
    let devices = [
        pci("pci:00:1f.6", 0x8086, 0x0d4c, 0x020000).with_label("i219"),
        pci("pci:01:00.0", 0x144d, 0xa808, 0x010802),
        pci("pci:02:00.0", 0x144d, 0xa808, 0x010802),
        pci("pci:00:02.0", 0x10de, 0x1eb6, 0x030000)
            .with_label("nvidia")
            .with_machine("ThinkPad"),
    ];
    (index, devices.to_vec())
}

#[test]
fn resolve_and_select() {
    let (index, devices) = small_machine();
    let plan = resolve(&devices, &index, ABI);
    assert_eq!(plan.devices.len(), 4);
    let eth = plan.device("pci:00:1f.6").unwrap();
    assert_eq!(eth.chosen.as_ref().unwrap().id, "eth-exact");
    assert_eq!(
        eth.alternatives
            .iter()
            .map(|c| c.id.as_str())
            .collect::<Vec<_>>(),
        ["eth-class"]
    );
    assert!(plan.device("pci:01:00.0").unwrap().alternatives.is_empty());
    assert!(plan.device("nope").is_none());

    let driverless = plan.driverless();
    assert_eq!(driverless.len(), 1);
    assert_eq!(driverless[0].address, "pci:00:02.0");
    let requests = plan.blueprint_requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].address, "pci:00:02.0");
    assert_eq!(requests[0].ids, vec![devices[3].primary.clone()]);
    assert_eq!(requests[0].label.as_deref(), Some("nvidia"));
    assert_eq!(requests[0].machine.as_deref(), Some("ThinkPad"));

    let chosen = select_for_install(&plan, false);
    assert_eq!(chosen.packages, ["eth-exact-1.0.0.utr", "nvme-1.0.0.utr"]);
    assert_eq!(chosen.index_files, [INDEX_NAME, INDEX_SIGNATURE_NAME]);
    let everything = select_for_install(&plan, true);
    assert_eq!(
        everything.packages,
        [
            "eth-class-1.0.0.utr",
            "eth-exact-1.0.0.utr",
            "nvme-1.0.0.utr",
            "unused-1.0.0.utr"
        ]
    );
}

#[test]
fn installed_subset_and_edge_cases() {
    let (index, devices) = small_machine();
    let chosen = select_for_install(&resolve(&devices, &index, ABI), false);
    // The installed system limits the verified index to the copied packages.
    let installed = index.retain_files(&chosen.packages);
    assert_eq!(installed.len(), 2);
    let plan = resolve(&devices, &installed, ABI);
    assert!(plan.device("pci:00:1f.6").unwrap().alternatives.is_empty());
    assert_eq!(plan.driverless().len(), 1);

    let nothing = resolve(&[], &index, ABI);
    assert!(nothing.devices.is_empty() && nothing.driverless().is_empty());
    assert!(select_for_install(&nothing, false).packages.is_empty());
    assert_eq!(resolve(&devices, &index, "other-abi").driverless().len(), 4);
}
