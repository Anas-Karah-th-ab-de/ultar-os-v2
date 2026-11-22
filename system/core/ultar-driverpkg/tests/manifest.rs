//! Manifest parsing and strict validation.

mod common;

use common::{example_manifests, manifest_text};
use ultar_driverpkg::manifest::{Manifest, ManifestError};
use ultar_driverpkg::pattern::PatternError;
use ultar_driverpkg::{Coded, Version};
use ultar_driverpkg::{DriverClass, Origin};

const FULL: &str = r#"
id = "gen-intel-eth"
version = "1.2.0"
class = "net"
kernel_abi = ["ultar-kern-abi-1", "linux-6.8.0-*"]
origin = "workshop"
rank = 50
name_key = "driver.gen-intel-eth.name"
description_key = "driver.gen-intel-eth.description"

[match]
pci = ["8086:10d3", "8086:*"]
pci_class = ["0200"]
usb = ["0bda:8153"]
usb_class = ["020600"]
dt = ["snps,dwmac"]
acpi = ["pnp0c0a"]
dmi = ["ThinkPad*"]
platform = ["loopback"]

[content]
module = "./drivers/gen_intel_eth.uko"
firmware = ["firmware/a.bin", "firmware/b.bin"]
hal_config = "hal/net.toml"
"#;

fn err(text: &str) -> ManifestError {
    Manifest::parse(text).unwrap_err()
}

fn with(rules: &str) -> String {
    manifest_text("drv", "1.0.0", 0, rules)
}

#[test]
fn parses_full_manifest() {
    let m = Manifest::parse(FULL).unwrap();
    assert_eq!(m.id, "gen-intel-eth");
    assert_eq!(m.version, Version::new(1, 2, 0));
    assert_eq!(m.class, DriverClass::Net);
    assert_eq!(m.origin, Origin::Workshop);
    assert_eq!(m.rank, 50);
    assert_eq!(m.name_key, "driver.gen-intel-eth.name");
    assert_eq!(
        m.description_key.as_deref(),
        Some("driver.gen-intel-eth.description")
    );
    assert_eq!(m.rules.pci.len(), 2);
    assert_eq!(m.rules.acpi, vec!["PNP0C0A".to_string()]);
    assert_eq!(m.rules.dmi, vec!["ThinkPad*".to_string()]);
    assert_eq!(
        m.content.module.as_deref(),
        Some("drivers/gen_intel_eth.uko")
    );
    assert_eq!(m.content.paths().count(), 4);
    assert!(m.supports_kernel("ultar-kern-abi-1"));
    assert!(m.supports_kernel("linux-6.8.0-45-generic"));
    assert!(!m.supports_kernel("ultar-kern-abi-2"));
    assert!(!m.supports_kernel("linux-6.9.0"));
    assert_eq!(Manifest::from_bytes(FULL.as_bytes()).unwrap(), m);
}

#[test]
fn defaults_and_enums() {
    let text = "id = \"a\"\nversion = \"0.1.0\"\nclass = \"other\"\nkernel_abi = [\"k\"]\norigin = \"hand\"\n\
                name_key = \"a.b\"\n[match]\nplatform = [\"x\"]\n[content]\nfirmware = [\"f.bin\"]\n";
    let m = Manifest::parse(text).unwrap();
    assert_eq!(m.rank, 0);
    assert_eq!(m.description_key, None);
    assert_eq!(m.content.module, None);
    assert_eq!(m.content.hal_config, None);
    assert_eq!(DriverClass::ALL.len(), 14);
    for class in DriverClass::ALL {
        assert_eq!(DriverClass::parse(class.as_str()), Some(*class));
        assert_eq!(class.to_string(), class.as_str());
    }
    for origin in Origin::ALL {
        assert_eq!(Origin::parse(origin.as_str()), Some(*origin));
    }
    assert_eq!(Origin::ALL.len(), 3);
    assert_eq!(DriverClass::parse("netz"), None);
}

#[test]
fn every_example_manifest_is_valid() {
    let examples = example_manifests();
    assert_eq!(examples.len(), 10);
    for text in &examples {
        let m = Manifest::parse(text).unwrap();
        assert!(m.supports_kernel("ultar-kern-abi-1"), "{}", m.id);
        assert!(m.name_key.starts_with("driver."), "{}", m.id);
    }
}

#[test]
fn syntax_errors() {
    assert!(matches!(err("id = "), ManifestError::Syntax(_)));
    assert!(matches!(
        err(&FULL.replace("rank = 50", "rank = \"high\"")),
        ManifestError::Syntax(_)
    ));
    assert!(matches!(
        err(&FULL.replace("rank = 50", "rank = 50\nsurprise = 1")),
        ManifestError::Syntax(_)
    ));
    assert!(matches!(
        err(&FULL.replace("[content]", "[content]\nextra = 1")),
        ManifestError::Syntax(_)
    ));
    assert!(matches!(
        err(&FULL.replace("[match]", "[match]\nserial = []")),
        ManifestError::Syntax(_)
    ));
    assert!(matches!(
        err(&FULL.replace("id = \"gen-intel-eth\"\n", "")),
        ManifestError::Syntax(_)
    ));
    assert!(matches!(
        err(&FULL.replace("name_key", "title_key")),
        ManifestError::Syntax(_)
    ));
    assert_eq!(
        Manifest::from_bytes(&[0xff, 0xfe]).unwrap_err(),
        ManifestError::NotUtf8
    );
    assert!(err("id = ")
        .to_string()
        .starts_with("driverpkg.manifest.syntax: "));
}

#[test]
fn field_errors() {
    for bad in ["", "Upper", "-lead", "has space", "ä", &"x".repeat(65)] {
        let text = manifest_text(bad, "1.0.0", 0, "platform = [\"x\"]")
            .replace("driver..name", "driver.x.name");
        let text = text.replace(&format!("driver.{bad}.name"), "driver.x.name");
        assert_eq!(err(&text), ManifestError::BadId(bad.to_string()), "{bad:?}");
    }
    assert!(matches!(
        err(&manifest_text("a", "1.0", 0, "platform = [\"x\"]")),
        ManifestError::BadVersion(_)
    ));
    assert_eq!(
        err(&FULL.replace("\"net\"", "\"netz\"")),
        ManifestError::UnknownClass("netz".into())
    );
    assert_eq!(
        err(&FULL.replace("\"workshop\"", "\"linux\"")),
        ManifestError::UnknownOrigin("linux".into())
    );
    let abi = |list: &str| FULL.replace("[\"ultar-kern-abi-1\", \"linux-6.8.0-*\"]", list);
    assert_eq!(err(&abi("[]")), ManifestError::NoKernelAbi);
    for bad in ["", "*", "a*b", "has space", "**"] {
        assert_eq!(
            err(&abi(&format!("[\"{bad}\"]"))),
            ManifestError::BadKernelAbi(bad.to_string()),
            "{bad:?}"
        );
    }
    assert_eq!(
        err(&manifest_text("a", "1.0.0", 1001, "platform = [\"x\"]")),
        ManifestError::RankOutOfRange(1001)
    );
    assert_eq!(
        err(&manifest_text("a", "1.0.0", -1001, "platform = [\"x\"]")),
        ManifestError::RankOutOfRange(-1001)
    );
    assert!(Manifest::parse(&manifest_text("a", "1.0.0", 1000, "platform = [\"x\"]")).is_ok());
    for bad in [
        "Intel Ethernet",
        "nodots",
        ".lead",
        "trail.",
        "a..b",
        "Upper.case",
        "",
    ] {
        let text = FULL.replace("driver.gen-intel-eth.name", bad);
        assert_eq!(
            err(&text),
            ManifestError::BadMessageKey(bad.to_string()),
            "{bad:?}"
        );
    }
    let text = FULL.replace("driver.gen-intel-eth.description", "Plain text");
    assert_eq!(
        err(&text),
        ManifestError::BadMessageKey("Plain text".into())
    );
}

#[test]
fn match_errors() {
    assert_eq!(err(&with("")), ManifestError::NoMatchRule);
    assert_eq!(
        err(&with("dmi = [\"ThinkPad*\"]")),
        ManifestError::NoMatchRule
    );
    let bad_pattern = |rules: &str, field: &'static str, problem: PatternError| {
        assert_eq!(
            err(&with(rules)),
            ManifestError::BadPattern { field, problem }
        );
    };
    bad_pattern(
        "pci = [\"8086\"]",
        "pci",
        PatternError::BadId("8086".into()),
    );
    bad_pattern("pci = [\"*:*\"]", "pci", PatternError::BadId("*:*".into()));
    bad_pattern(
        "usb = [\"8087:00260\"]",
        "usb",
        PatternError::BadId("8087:00260".into()),
    );
    bad_pattern(
        "pci_class = [\"0c033\"]",
        "pci_class",
        PatternError::BadClass("0c033".into()),
    );
    bad_pattern(
        "usb_class = [\"zz\"]",
        "usb_class",
        PatternError::BadClass("zz".into()),
    );
    bad_pattern("dt = [\"\"]", "dt", PatternError::BadName(String::new()));
    bad_pattern(
        "acpi = [\" PNP\"]",
        "acpi",
        PatternError::BadName(" PNP".into()),
    );
    bad_pattern(
        "platform = [\"x\"]\ndmi = [\"\"]",
        "dmi",
        PatternError::BadName(String::new()),
    );
    bad_pattern(
        "platform = [\"\"]",
        "platform",
        PatternError::BadName(String::new()),
    );
}

#[test]
fn duplicate_patterns() {
    let dup = err(&with("pci = [\"8086:0D4C\", \"8086:0d4c\"]"));
    assert_eq!(
        dup,
        ManifestError::DuplicatePattern {
            field: "pci",
            value: "8086:0d4c".into()
        }
    );
    assert_eq!(
        dup.to_string(),
        "driverpkg.manifest.duplicate-pattern: pci: 8086:0d4c"
    );
    let dup = err(&with("acpi = [\"pnp0c0a\", \"PNP0C0A\"]"));
    assert_eq!(
        dup,
        ManifestError::DuplicatePattern {
            field: "acpi",
            value: "PNP0C0A".into()
        }
    );
}

#[test]
fn content_errors() {
    let base = with("platform = [\"x\"]");
    let content = |body: &str| base.replace("module = \"drivers/drv.uko\"", body);
    assert_eq!(err(&content("")), ManifestError::NoContent);
    assert_eq!(
        err(&content("module = \"\"\nfirmware = []")),
        ManifestError::NoContent
    );
    for bad in ["/lib/firmware/x.bin", "../x.bin", "a/../../x", "a\\\\b"] {
        let shown = bad.replace("\\\\", "\\");
        assert_eq!(
            err(&content(&format!("module = \"{bad}\""))),
            ManifestError::BadContentPath(shown.clone())
        );
        assert_eq!(
            err(&content(&format!("firmware = [\"{bad}\"]"))),
            ManifestError::BadContentPath(shown.clone())
        );
        assert_eq!(
            err(&content(&format!("hal_config = \"{bad}\""))),
            ManifestError::BadContentPath(shown)
        );
    }
}

#[test]
fn every_error_has_a_code_and_text() {
    let errors = [
        err("id = "),
        err(&with("")),
        err(&with("pci = [\"x\"]")),
        ManifestError::NotUtf8,
        ManifestError::NoContent,
        ManifestError::RankOutOfRange(5000),
    ];
    for e in errors {
        assert!(e.code().starts_with("driverpkg.manifest."));
        assert!(e.to_string().starts_with(e.code()));
    }
}
