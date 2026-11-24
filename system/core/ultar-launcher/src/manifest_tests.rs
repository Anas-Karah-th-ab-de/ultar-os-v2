//! Tests of the manifest reader (hostile and normal input).

use super::*;

const GOOD: &str = r#"
id          = "camera"
version     = "1.4.0"
publisher   = "km-code-craft"
kind        = "native"
min_os      = "2.0"
category    = "media"
name        = "app.camera.name"
description = "app.camera.desc"
icon        = "assets/icon.svg"
exec        = "/usr/lib/ultar/apps/camera/camera"
args        = ["--mode", "photo"]

[form]
factors = ["phone", "tablet"]
modes   = ["mobile", "desktop"]

[needs]
required = ["camera"]

[permissions]
bus        = ["camera.capture", "media.store", "notify.post"]
files      = ["media:pictures:rw"]
network    = "ask"
background = true
ui         = ["ui.capture"]

[agent]
actions = ["take_photo"]

[limits]
memory_mb = 120
pids      = 40
cpu       = "foreground"

[service]
exec    = "/usr/lib/ultar/apps/camera/camerad"
restart = "always"
"#;

#[test]
fn the_example_of_the_app_rules_is_read() {
    let m = Manifest::parse(GOOD.as_bytes()).unwrap();
    assert_eq!(m.id.as_str(), "camera");
    assert_eq!(m.version, "1.4.0");
    assert_eq!(m.ui.exec, "/usr/lib/ultar/apps/camera/camera");
    assert_eq!(m.ui.args, ["--mode", "photo"]);
    let (service, restart) = m.service.clone().unwrap();
    assert_eq!(service.exec, "/usr/lib/ultar/apps/camera/camerad");
    assert_eq!(restart, Restart::Always);
    assert_eq!(m.declared.names(), "network,background,ui.capture");
    assert!(!m.network_allowed);
    assert_eq!(m.bus.len(), 3);
    assert_eq!(m.files, ["media:pictures:rw"]);
    assert_eq!(m.kernel_rights, 0);
    assert_eq!(m.limits.memory_mb, Some(120));
    assert_eq!(m.limits.pids, Some(40));
}

fn with(replace: &str, by: &str) -> Result<Manifest, ManifestError> {
    assert!(GOOD.contains(replace), "{replace}");
    Manifest::parse(GOOD.replacen(replace, by, 1).as_bytes())
}

/// Each rule's breach: replace the first text by the second.
const BREACHES: [(&str, &str, ManifestError); 18] = [
    (
        "id          = \"camera\"",
        "id = \"Camera\"",
        ManifestError::Id,
    ),
    (
        "kind        = \"native\"",
        "kind = \"compat-deb\"",
        ManifestError::NotNative,
    ),
    (
        "version     = \"1.4.0\"",
        "version = \"\"",
        ManifestError::Version,
    ),
    (
        "version     = \"1.4.0\"",
        "version = \"1 4\"",
        ManifestError::Version,
    ),
    (
        "/usr/lib/ultar/apps/camera/camera\"",
        "camera\"",
        ManifestError::Program,
    ),
    (
        "/usr/lib/ultar/apps/camera/camera\"",
        "/usr/../etc/shadow\"",
        ManifestError::Program,
    ),
    (
        "/usr/lib/ultar/apps/camera/camera\"",
        "/usr//x\"",
        ManifestError::Program,
    ),
    (
        "network    = \"ask\"",
        "network = \"vpn-only\"",
        ManifestError::Unsupported,
    ),
    (
        "network    = \"ask\"",
        "network = \"always\"",
        ManifestError::Permission,
    ),
    (
        "ui         = [\"ui.capture\"]",
        "ui = [\"network\"]",
        ManifestError::Permission,
    ),
    (
        "ui         = [\"ui.capture\"]",
        "ui = [\"ui.root\"]",
        ManifestError::Permission,
    ),
    (
        "background = true",
        "kernel = [\"rooot\"]",
        ManifestError::Permission,
    ),
    (
        "\"notify.post\"]",
        "\"Notify Post\"]",
        ManifestError::Permission,
    ),
    ("memory_mb = 120", "memory_mb = 0", ManifestError::Limit),
    (
        "cpu       = \"foreground\"",
        "cpu = \"turbo\"",
        ManifestError::Limit,
    ),
    (
        "restart = \"always\"",
        "restart = \"sometimes\"",
        ManifestError::Restart,
    ),
    ("[limits]", "[limits]\nswap_mb = 1", ManifestError::Syntax),
    (
        "[permissions]",
        "[permissions]\nroot = true",
        ManifestError::Syntax,
    ),
];

#[test]
fn every_rule_refuses_its_breach() {
    for (replace, by, error) in BREACHES {
        assert_eq!(with(replace, by), Err(error), "{by}");
        assert!(error.key().starts_with("launcher.manifest."));
    }
    assert_eq!(
        with("publisher   = \"km-code-craft\"", "publisher = 1"),
        Err(ManifestError::Syntax)
    );
    assert_eq!(
        with("[form]", "form = \"x\"\n[x]"),
        Err(ManifestError::Syntax)
    );
}

#[test]
fn sizes_types_and_unknown_tables_are_refused() {
    assert_eq!(
        Manifest::parse(&vec![b' '; MAX_BYTES + 1]),
        Err(ManifestError::Size)
    );
    assert_eq!(Manifest::parse(&[0xff, 0xfe]), Err(ManifestError::Size));
    assert_eq!(Manifest::parse(b"id = 1"), Err(ManifestError::Syntax));
    assert_eq!(
        with("[agent]", "[sandbox]\noff = true\n[agent]"),
        Err(ManifestError::Syntax)
    );
    // the legacy list form of permissions is not a manifest of the app rules
    let legacy = "id = \"a\"\nversion = \"1\"\nkind = \"native\"\nexec = \"/bin/a\"\npermissions = [\"network\"]\n";
    assert_eq!(
        Manifest::parse(legacy.as_bytes()),
        Err(ManifestError::Syntax)
    );
    let many: Vec<String> = (0..=MAX_ARGS).map(|n| format!("\"{n}\"")).collect();
    let args = format!("args = [{}]", many.join(","));
    assert_eq!(
        with("args        = [\"--mode\", \"photo\"]", &args),
        Err(ManifestError::Program)
    );
}

#[test]
fn a_minimal_manifest_declares_nothing() {
    let text = "id = \"x.y\"\nversion = \"1\"\nkind = \"native\"\nexec = \"/bin/y\"\n";
    let m = Manifest::parse(text.as_bytes()).unwrap();
    assert!(m.declared.is_empty() && m.service.is_none() && m.bus.is_empty());
    assert_eq!(m.limits, Requested::default());
    let allowed = text.replace("exec", "[permissions]\nnetwork = \"allowed\"\n[x]\nexec");
    // `[x]` is an unknown table
    assert_eq!(
        Manifest::parse(allowed.as_bytes()),
        Err(ManifestError::Syntax)
    );
    let allowed = format!("{text}[permissions]\nnetwork = \"allowed\"\nkernel = [\"set_id\"]\n");
    let m = Manifest::parse(allowed.as_bytes()).unwrap();
    assert!(m.network_allowed && m.declared.has(Permission::Network));
    assert_eq!(m.kernel_rights, 1 << 8);
    assert_eq!(Restart::Never.word(), "never");
}

/// Fixed-seed fuzz: random bytes and mutated manifests never panic.
#[test]
fn random_and_mutated_input_never_panics() {
    let mut seed: u64 = 0x5eed_1a0c;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..3000 {
        let len = (next() % 300) as usize;
        let bytes: Vec<u8> = (0..len).map(|_| next() as u8).collect();
        let _ = Manifest::parse(&bytes);
        let mut good = GOOD.as_bytes().to_vec();
        for _ in 0..(next() % 8) {
            let at = (next() as usize) % good.len();
            good[at] = next() as u8;
        }
        let _ = Manifest::parse(&good);
    }
}

#[test]
fn the_publisher_and_the_open_section_are_read() {
    assert_eq!(
        Manifest::parse(GOOD.as_bytes())
            .unwrap()
            .publisher
            .as_deref(),
        Some("km-code-craft")
    );
    let open = GOOD.replace(
        "[agent]",
        "[open]\nschemes = [\"trash\", \"file\"]\nmulti_window = true\n\n[agent]",
    );
    let m = Manifest::parse(open.as_bytes()).unwrap();
    assert_eq!(
        (m.open_schemes.clone(), m.multi_window),
        (vec!["trash".to_owned(), "file".to_owned()], true)
    );
    let bad = GOOD.replace("[agent]", "[open]\nschemes = [\"Trash\"]\n\n[agent]");
    assert_eq!(
        Manifest::parse(bad.as_bytes()),
        Err(ManifestError::Permission)
    );
    let unknown = GOOD.replace("[agent]", "[open]\nexec = true\n\n[agent]");
    assert_eq!(
        Manifest::parse(unknown.as_bytes()),
        Err(ManifestError::Syntax)
    );
}
