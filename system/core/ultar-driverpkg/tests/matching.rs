//! Candidate ordering of the matching engine.

mod common;

use common::{index_of, manifest_text};
use ultar_driverpkg::{
    candidates, DeviceDescriptor, HardwareId, MatchKind, PrivateKey, Specificity,
};

const ABI: &str = "ultar-kern-abi-1";

fn eth() -> DeviceDescriptor {
    let id = HardwareId::Pci {
        vendor: 0x8086,
        device: 0x0d4c,
        class: 0x020000,
        subsys: None,
    };
    DeviceDescriptor::new("pci:00:1f.6", id)
}

fn ids(device: &DeviceDescriptor, manifests: &[String]) -> Vec<String> {
    let index = index_of(manifests, &PrivateKey::generate());
    candidates(device, &index, ABI)
        .into_iter()
        .map(|c| format!("{}@{}", c.id, c.version))
        .collect()
}

fn m(id: &str, version: &str, rank: i32, rules: &str) -> String {
    manifest_text(id, version, rank, rules)
}

#[test]
fn exact_beats_wildcard_beats_class() {
    let manifests = [
        m("by-base-class", "1.0.0", 900, "pci_class = [\"02\"]"),
        m("by-sub-class", "1.0.0", 800, "pci_class = [\"0200\"]"),
        m("by-full-class", "1.0.0", 700, "pci_class = [\"020000\"]"),
        m("by-vendor", "1.0.0", 600, "pci = [\"8086:*\"]"),
        m("by-id", "1.0.0", -5, "pci = [\"8086:0d4c\"]"),
        m(
            "unrelated",
            "1.0.0",
            999,
            "pci = [\"10ec:8168\"]\npci_class = [\"0280\"]\nusb = [\"8086:0d4c\"]",
        ),
    ];
    let found = ids(&eth(), &manifests);
    assert_eq!(
        found,
        [
            "by-id@1.0.0",
            "by-vendor@1.0.0",
            "by-full-class@1.0.0",
            "by-sub-class@1.0.0",
            "by-base-class@1.0.0"
        ]
    );
    let index = index_of(&manifests, &PrivateKey::generate());
    let list = candidates(&eth(), &index, ABI);
    assert_eq!(list[0].specificity, Specificity::EXACT);
    assert_eq!(list[1].specificity.kind, MatchKind::Wildcard);
    assert_eq!(list[2].specificity, Specificity::class(6));
    assert_eq!(list[0].file, "by-id-1.0.0.utr");
    assert_eq!(list[0].name_key, "driver.by-id.name");
}

#[test]
fn rank_then_version_then_id() {
    let rule = "pci = [\"8086:0d4c\"]";
    let manifests = [
        m("low", "9.0.0", 10, rule),
        m("high", "1.0.0", 60, rule),
        m("same-b", "1.0.0", 50, rule),
        m("same-a", "1.0.0", 50, rule),
        m("same-a", "1.2.0", 50, rule),
        m("same-a", "1.2.0-rc.1", 50, rule),
    ];
    let expected = [
        "high@1.0.0",
        "same-a@1.2.0",
        "same-a@1.2.0-rc.1",
        "same-a@1.0.0",
        "same-b@1.0.0",
        "low@9.0.0",
    ];
    assert_eq!(ids(&eth(), &manifests), expected);
    // The order does not depend on the order of the packages in the depot.
    let mut reversed = manifests.to_vec();
    reversed.reverse();
    assert_eq!(ids(&eth(), &reversed), expected);
}

#[test]
fn a_driver_matching_twice_is_listed_once_with_its_best_match() {
    let manifests = [m(
        "both",
        "1.0.0",
        0,
        "pci = [\"8086:0d4c\", \"8086:*\"]\npci_class = [\"0200\"]",
    )];
    let index = index_of(&manifests, &PrivateKey::generate());
    let list = candidates(&eth(), &index, ABI);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].specificity, Specificity::EXACT);
}

#[test]
fn kernel_abi_filters() {
    let other_abi = m("for-linux", "1.0.0", 99, "pci = [\"8086:0d4c\"]")
        .replace("ultar-kern-abi-1", "linux-6.8.0-*");
    let manifests = [
        other_abi,
        m("for-ultar", "1.0.0", 1, "pci = [\"8086:0d4c\"]"),
    ];
    let index = index_of(&manifests, &PrivateKey::generate());
    let names = |abi: &str| {
        candidates(&eth(), &index, abi)
            .into_iter()
            .map(|c| c.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(names(ABI), ["for-ultar"]);
    assert_eq!(names("linux-6.8.0-45-generic"), ["for-linux"]);
    assert!(names("ultar-kern-abi-2").is_empty());
}

#[test]
fn machine_rules_restrict_and_prefer() {
    let rule = "pci = [\"8086:0d4c\"]";
    let manifests = [
        m("generic", "1.0.0", 90, rule),
        m(
            "thinkpad-tuned",
            "1.0.0",
            10,
            &format!("{rule}\ndmi = [\"ThinkPad P1*\"]"),
        ),
        m(
            "dell-tuned",
            "1.0.0",
            99,
            &format!("{rule}\ndmi = [\"Latitude*\", \"XPS*\"]"),
        ),
    ];
    let on = |machine: Option<&str>| {
        let mut device = eth();
        device.machine = machine.map(str::to_string);
        ids(&device, &manifests)
    };
    assert_eq!(
        on(Some("ThinkPad P1 Gen 3")),
        ["thinkpad-tuned@1.0.0", "generic@1.0.0"]
    );
    assert_eq!(on(Some("XPS 15")), ["dell-tuned@1.0.0", "generic@1.0.0"]);
    assert_eq!(on(Some("Beelink GTi15 Ultra")), ["generic@1.0.0"]);
    assert_eq!(on(None), ["generic@1.0.0"]);
    // Specificity still outranks machine tuning.
    let mixed = [
        m("exact", "1.0.0", 0, rule),
        m(
            "tuned-class",
            "1.0.0",
            0,
            "pci_class = [\"0200\"]\ndmi = [\"*\"]",
        ),
    ];
    let mut device = eth();
    device.machine = Some("anything".into());
    assert_eq!(ids(&device, &mixed), ["exact@1.0.0", "tuned-class@1.0.0"]);
}

#[test]
fn usb_devices() {
    let manifests = [
        m("bt-exact", "1.0.0", 0, "usb = [\"8087:0026\"]"),
        m("bt-vendor", "1.0.0", 0, "usb = [\"8087:*\"]"),
        m("bt-class", "1.0.0", 0, "usb_class = [\"e00101\"]"),
        m(
            "pci-lookalike",
            "1.0.0",
            0,
            "pci = [\"8087:0026\"]\npci_class = [\"e00101\"]",
        ),
    ];
    let with_class = DeviceDescriptor::new(
        "usb:1-14",
        HardwareId::Usb {
            vendor: 0x8087,
            product: 0x26,
            class: 0xe00101,
        },
    );
    assert_eq!(
        ids(&with_class, &manifests),
        ["bt-exact@1.0.0", "bt-vendor@1.0.0", "bt-class@1.0.0"]
    );
    // Class 0 means "defined per interface": class rules never match it.
    let no_class = DeviceDescriptor::new(
        "usb:1-14",
        HardwareId::Usb {
            vendor: 0x8087,
            product: 0x26,
            class: 0,
        },
    );
    let zero_class = [
        m("zero", "1.0.0", 0, "usb_class = [\"00\"]"),
        m("bt-exact", "1.0.0", 0, "usb = [\"8087:0026\"]"),
    ];
    assert_eq!(ids(&no_class, &zero_class), ["bt-exact@1.0.0"]);
    let generic = DeviceDescriptor::new(
        "usb:1-2",
        HardwareId::Usb {
            vendor: 0x0a12,
            product: 1,
            class: 0xe00101,
        },
    );
    assert_eq!(ids(&generic, &manifests), ["bt-class@1.0.0"]);
}

#[test]
fn named_devices_and_aliases() {
    let manifests = [
        m("decon", "1.0.0", 0, "dt = [\"samsung,exynos7885-decon\"]"),
        m("decon-generic", "1.0.0", 5, "dt = [\"samsung,decon\"]"),
        m("battery", "1.0.0", 0, "acpi = [\"pnp0c0a\"]"),
        m("ps2", "1.0.0", 0, "platform = [\"ps2\"]"),
    ];
    let panel = DeviceDescriptor::new(
        "dt:decon",
        HardwareId::Dt("samsung,exynos7885-decon".into()),
    )
    .with_alias(HardwareId::Dt("samsung,decon".into()));
    assert_eq!(
        ids(&panel, &manifests),
        ["decon-generic@1.0.0", "decon@1.0.0"]
    );
    let battery = DeviceDescriptor::new("acpi:BAT0", HardwareId::Acpi("PNP0C0A".into()));
    assert_eq!(ids(&battery, &manifests), ["battery@1.0.0"]);
    let lower = DeviceDescriptor::new("acpi:BAT0", HardwareId::Acpi("pnp0c0a".into()));
    assert_eq!(ids(&lower, &manifests), ["battery@1.0.0"]);
    let keyboard = DeviceDescriptor::new("platform:ps2", HardwareId::Platform("ps2".into()));
    assert_eq!(ids(&keyboard, &manifests), ["ps2@1.0.0"]);
    // Names are not shared between namespaces.
    let wrong_space = DeviceDescriptor::new("dt:ps2", HardwareId::Dt("ps2".into()));
    assert!(ids(&wrong_space, &manifests).is_empty());
    let unknown = DeviceDescriptor::new("platform:x", HardwareId::Platform("unknown".into()));
    assert!(ids(&unknown, &manifests).is_empty());
    assert!(ids(&eth(), &[]).is_empty());
}
