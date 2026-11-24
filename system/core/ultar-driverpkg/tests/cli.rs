//! The `udrv` tool end to end, through its library entry point.

mod common;

use common::{crate_dir, example_manifests, manifest_text, TempDir};
use ultar_driverpkg::cli::{describe, run, CliError};
use ultar_driverpkg::msg::Lang;
use ultar_driverpkg::{Coded, Manifest, PrivateKey};

fn udrv(lang: Lang, args: &[&str]) -> Result<String, CliError> {
    let args: Vec<String> = args.iter().map(|a| (*a).to_string()).collect();
    let mut out = Vec::new();
    run(&args, lang, &mut out).map(|()| String::from_utf8(out).unwrap())
}

fn s(path: &std::path::Path) -> &str {
    path.to_str().unwrap()
}

/// Writes key files, a manifest and a payload directory; returns (private key file, trusted file).
fn setup(dir: &TempDir, manifest: &str) -> (String, String) {
    let key = PrivateKey::generate();
    std::fs::write(dir.join("publisher.key"), key.to_hex()).unwrap();
    std::fs::write(
        dir.join("trusted.keys"),
        format!("# publisher\n{}\n", key.public().to_hex()),
    )
    .unwrap();
    std::fs::write(dir.join("ultar-driver.toml"), manifest).unwrap();
    for path in Manifest::parse(manifest).unwrap().content.paths() {
        let target = dir.join("payload").join(path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(target, b"content").unwrap();
    }
    (
        s(&dir.join("publisher.key")).to_string(),
        s(&dir.join("trusted.keys")).to_string(),
    )
}

#[test]
fn keygen_prints_a_usable_pair() {
    let out = udrv(Lang::En, &["keygen"]).unwrap();
    let value = |label: &str| {
        out.lines()
            .find_map(|l| l.strip_prefix(label))
            .unwrap()
            .trim()
            .to_string()
    };
    let private = PrivateKey::from_hex(&value("Private key (secret, keep offline):")).unwrap();
    assert_eq!(private.public().to_hex(), value("Public key:"));
    assert_eq!(private.public().key_id(), value("Key id:"));
    assert!(matches!(
        udrv(Lang::En, &["keygen", "extra"]),
        Err(CliError::Usage)
    ));
}

#[test]
fn pack_verify_info() {
    let dir = TempDir::new("cli-pack");
    let (key, trusted) = setup(
        &dir,
        &manifest_text("drv", "1.2.3", 7, "pci = [\"8086:0d4c\"]"),
    );
    let package = dir.join("drv.utr");
    let (manifest, payload) = (dir.join("ultar-driver.toml"), dir.join("payload"));
    let out = udrv(
        Lang::En,
        &["pack", s(&manifest), s(&payload), s(&package), &key],
    )
    .unwrap();
    assert!(out.contains("Package created and signed"));
    assert!(package.is_file());

    let out = udrv(Lang::En, &["verify", s(&package), &trusted]).unwrap();
    assert!(out.contains("Signature valid, package trusted"));
    assert!(out.contains("Id: drv") && out.contains("Version: 1.2.3") && out.contains("Rank: 7"));
    let out = udrv(Lang::De, &["verify", s(&package), &trusted]).unwrap();
    assert!(out.contains("Signatur gültig") && out.contains("Kennung: drv"));
    let out = udrv(Lang::Ar, &["verify", s(&package), &trusted]).unwrap();
    assert!(out.contains("التوقيع صحيح") && out.contains("المعرّف: drv"));

    let out = udrv(Lang::En, &["info", s(&package)]).unwrap();
    assert!(out.contains("Display only – signature not verified"));
    assert!(out.contains("File: drivers/drv.uko (7)") && out.contains("Payload SHA-256: "));

    // The private key may also be given as hexadecimal text.
    let hex = std::fs::read_to_string(dir.join("publisher.key")).unwrap();
    let second = dir.join("drv2.utr");
    udrv(
        Lang::En,
        &["pack", s(&manifest), s(&payload), s(&second), &hex],
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&second).unwrap(),
        std::fs::read(&package).unwrap()
    );
}

/// Packs the manifest prepared by `setup` into `<dir>/<name>` and returns the package path.
fn pack_in(dir: &TempDir, name: &str, key: &str) -> String {
    let out = dir.join(name);
    let (manifest, payload) = (dir.join("ultar-driver.toml"), dir.join("payload"));
    udrv(Lang::En, &["pack", s(&manifest), s(&payload), s(&out), key]).unwrap();
    s(&out).to_string()
}

fn code_of(args: &[&str]) -> &'static str {
    udrv(Lang::En, args).unwrap_err().code()
}

#[test]
fn verify_failures_are_distinct() {
    let dir = TempDir::new("cli-reject");
    let (key, _) = setup(
        &dir,
        &manifest_text("drv", "1.0.0", 0, "pci = [\"8086:0d4c\"]"),
    );
    let package = pack_in(&dir, "drv.utr", &key);
    let keys = |name: &str, content: String| {
        std::fs::write(dir.join(name), content).unwrap();
        s(&dir.join(name)).to_string()
    };
    let other = keys("other.keys", PrivateKey::generate().public().to_hex());
    let err = udrv(Lang::En, &["verify", &package, &other]).unwrap_err();
    assert_eq!(err.code(), "driverpkg.signature.untrusted-key");
    assert!(describe(&err, Lang::En).starts_with("Signed with an untrusted key – rejected ["));
    assert!(describe(&err, Lang::De).starts_with("Mit fremdem Schlüssel signiert"));
    assert!(describe(&err, Lang::Ar).starts_with("موقّع بمفتاح غير موثوق"));

    let empty = keys("empty.keys", "# none\n".into());
    assert_eq!(
        code_of(&["verify", &package, &empty]),
        "driverpkg.signature.no-trusted-keys"
    );
    let bad = keys("bad.keys", "xyz\n".into());
    assert_eq!(
        code_of(&["verify", &package, &bad]),
        "driverpkg.key.bad-hex"
    );
    let err = udrv(Lang::En, &["verify", s(&dir.join("missing.utr")), &other]).unwrap_err();
    assert!(matches!(
        err,
        CliError::Io {
            kind: std::io::ErrorKind::NotFound,
            ..
        }
    ));
    assert!(describe(&err, Lang::En).starts_with("Cannot access the file [driverpkg.cli.io: "));
}

#[test]
fn pack_failures() {
    let dir = TempDir::new("cli-pack-fail");
    let (key, _) = setup(
        &dir,
        &manifest_text("drv", "1.0.0", 0, "pci = [\"8086:0d4c\"]"),
    );
    let (manifest, payload, out) = (
        dir.join("ultar-driver.toml"),
        dir.join("payload"),
        dir.join("o.utr"),
    );
    assert_eq!(
        code_of(&["pack", s(&manifest), s(&payload), s(&out), "zz"]),
        "driverpkg.key.bad-hex"
    );
    std::fs::write(dir.join("broken.toml"), "id = 5").unwrap();
    let broken = dir.join("broken.toml");
    assert_eq!(
        code_of(&["pack", s(&broken), s(&payload), s(&out), &key]),
        "driverpkg.manifest.syntax"
    );
    std::fs::remove_file(dir.join("payload/drivers/drv.uko")).unwrap();
    assert_eq!(
        code_of(&["pack", s(&manifest), s(&payload), s(&out), &key]),
        "driverpkg.package.missing-content"
    );
    assert!(!out.exists());
}

/// A depot directory with all example drivers packed and indexed; returns (key file, trusted file).
fn example_depot(dir: &TempDir) -> (String, String) {
    std::fs::create_dir_all(dir.join("depot")).unwrap();
    let keys = setup(dir, &example_manifests()[0]);
    for text in example_manifests() {
        let work = TempDir::new("cli-one");
        setup(&work, &text);
        let id = Manifest::parse(&text).unwrap().id;
        let packed = pack_in(&work, "out.utr", &keys.0);
        std::fs::copy(packed, dir.join("depot").join(format!("{id}.utr"))).unwrap();
    }
    let out = udrv(Lang::En, &["index", s(&dir.join("depot")), &keys.0]).unwrap();
    assert!(out.contains("Index created and signed, packages: 11"));
    keys
}

#[test]
fn index_and_match_a_real_machine() {
    let dir = TempDir::new("cli-depot");
    let (_, trusted) = example_depot(&dir);
    let index = dir.join("depot/index.toml");
    let list = crate_dir().join("tests/fixtures/beelink-gti15.hw");

    let out = udrv(
        Lang::En,
        &["match", s(&index), s(&list), "--trusted", &trusted],
    )
    .unwrap();
    assert!(!out.contains("Warning"));
    let port =
        "pci:ad:00.0  pci:8086:57b0  intel-e610-port0\n  Driver: gen-ixgbe 1.0.0 (gen-ixgbe.utr)";
    assert!(out.contains(port));
    assert!(out.contains("intel-npu\n  No driver – workshop blueprint requested"));
    assert!(out.contains("Devices with driver: 10") && out.contains("Devices without driver: 4"));

    let out = udrv(Lang::Ar, &["match", s(&index), s(&list)]).unwrap();
    assert!(out.contains("تنبيه") && out.contains("أجهزة بلا تعريف: 4"));
    let out = udrv(
        Lang::De,
        &["match", s(&index), s(&list), "--abi", "linux-6.8.0-45"],
    )
    .unwrap();
    assert!(out.contains("Geräte mit Treiber: 0") && out.contains("Geräte ohne Treiber: 14"));
}

#[test]
fn match_and_index_rejections() {
    let dir = TempDir::new("cli-depot-reject");
    let (key, trusted) = example_depot(&dir);
    let (depot, index) = (dir.join("depot"), dir.join("depot/index.toml"));
    let list = crate_dir().join("tests/fixtures/beelink-gti15.hw");
    // A modified index is refused when trust is requested.
    let text = std::fs::read_to_string(&index).unwrap();
    std::fs::write(&index, text.replace("rank = 50", "rank = 51")).unwrap();
    assert_eq!(
        code_of(&["match", s(&index), s(&list), "--trusted", &trusted]),
        "driverpkg.signature.invalid"
    );
    std::fs::write(dir.join("bad.hw"), "pci nonsense").unwrap();
    assert_eq!(
        code_of(&["match", s(&index), s(&dir.join("bad.hw"))]),
        "driverpkg.hwlist.bad-id"
    );
    // A foreign package in the depot stops indexing unless its publisher is trusted.
    let stranger = TempDir::new("cli-stranger");
    let manifest = manifest_text("extra", "1.0.0", 0, "platform = [\"x\"]");
    let (stranger_key, stranger_trusted) = setup(&stranger, &manifest);
    let packed = pack_in(&stranger, "extra.utr", &stranger_key);
    std::fs::copy(packed, depot.join("extra.utr")).unwrap();
    assert_eq!(
        code_of(&["index", s(&depot), &key]),
        "driverpkg.index.package"
    );
    let out = udrv(
        Lang::En,
        &["index", s(&depot), &key, "--trusted", &stranger_trusted],
    )
    .unwrap();
    assert!(out.contains("packages: 12"));
}

#[test]
fn usage_and_unknown_commands() {
    assert!(matches!(udrv(Lang::En, &[]), Err(CliError::Usage)));
    assert!(udrv(Lang::En, &["help"]).unwrap().starts_with("Usage:"));
    assert!(udrv(Lang::De, &["--help"]).unwrap().starts_with("Aufruf:"));
    assert!(udrv(Lang::Ar, &["-h"]).unwrap().starts_with("الاستخدام:"));
    let err = udrv(Lang::En, &["frobnicate"]).unwrap_err();
    assert!(matches!(&err, CliError::UnknownCommand(name) if name == "frobnicate"));
    assert_eq!(
        describe(&err, Lang::En),
        "Unknown command [driverpkg.cli.unknown-command: frobnicate]"
    );
    for args in [
        &["pack", "a"][..],
        &["verify", "a"],
        &["info"],
        &["index", "a"],
        &["match", "a"],
    ] {
        assert!(
            matches!(udrv(Lang::En, args), Err(CliError::Usage)),
            "{args:?}"
        );
    }
    assert!(matches!(
        udrv(Lang::En, &["info", "a", "--bogus", "x"]),
        Err(CliError::Usage)
    ));
    assert!(matches!(
        udrv(Lang::En, &["match", "a", "b", "--abi"]),
        Err(CliError::Usage)
    ));
    assert_eq!(CliError::Usage.to_string(), "driverpkg.cli.usage");
    assert_eq!(CliError::Output.code(), "driverpkg.cli.output");
}
