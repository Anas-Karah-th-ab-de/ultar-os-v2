//! The two real test machines resolve to the expected drivers (plan §5, DRIVER-TARGETS).
//!
//! The hardware lists in `tests/fixtures/` are transcribed from
//! `devices/thinkpad-p1-gen3/hardware.md` and `devices/beelink-gti15/hardware.md`.

mod common;

use common::{crate_dir, example_manifests, index_of};
use std::collections::BTreeMap;
use ultar_driverpkg::hwlist::parse_hardware_list;
use ultar_driverpkg::{resolve, select_for_install, Plan, PrivateKey};

fn plan_for(fixture: &str) -> Plan {
    let text = std::fs::read_to_string(crate_dir().join("tests/fixtures").join(fixture)).unwrap();
    let devices = parse_hardware_list(&text).unwrap();
    let index = index_of(&example_manifests(), &PrivateKey::generate());
    resolve(&devices, &index, "ultar-kern-abi-1")
}

/// Inventory label → chosen driver id (`None` = driverless).
fn chosen(plan: &Plan) -> BTreeMap<String, Option<String>> {
    plan.devices
        .iter()
        .map(|d| {
            (
                d.device.label.clone().unwrap(),
                d.chosen.as_ref().map(|c| c.id.clone()),
            )
        })
        .collect()
}

fn expect(plan: &Plan, with_driver: &[(&str, &str)], driverless: &[&str]) {
    let actual = chosen(plan);
    let mut expected: BTreeMap<String, Option<String>> = BTreeMap::new();
    for (label, driver) in with_driver {
        expected.insert((*label).to_string(), Some((*driver).to_string()));
    }
    for label in driverless {
        expected.insert((*label).to_string(), None);
    }
    assert_eq!(actual, expected);
    assert_eq!(plan.devices.len(), expected.len(), "labels must be unique");
    assert_eq!(plan.driverless().len(), driverless.len());
    assert_eq!(plan.blueprint_requests().len(), driverless.len());
}

#[test]
fn thinkpad_p1_gen3() {
    let plan = plan_for("thinkpad-p1-gen3.hw");
    expect(
        &plan,
        &[
            ("samsung-pm981a", "nvme"),
            ("pch-xhci", "gen-xhci"),
            ("thunderbolt-xhci", "gen-xhci"),
            ("nvidia-usb-c-xhci", "gen-xhci"),
            ("intel-i219-lm", "gen-intel-eth"),
            ("intel-uhd-gen9.5", "intel-display"),
            ("nvidia-quadro-rtx-4000", "nvidia-display"),
            ("intel-ax201", "intel-wifi"),
            ("intel-bluetooth", "intel-bt-usb"),
            ("intel-cavs-audio", "gen-hda"),
            ("nvidia-hdmi-audio", "gen-hda"),
        ],
        &[
            "host-bridge",
            "lpc",
            "nvidia-usb-c-ucsi",
            "thunderbolt-controller",
            "thunderbolt-pcie-switch",
            "lpss-i2c-0",
            "lpss-i2c-1",
            "smbus",
            "genesys-card-reader",
            "thermal",
            "mei",
            "spi",
            "amt-sol",
            "integrated-camera",
            "synaptics-fingerprint",
        ],
    );
    assert!(plan
        .devices
        .iter()
        .all(|d| d.device.machine.as_deref() == Some("ThinkPad P1 Gen 3")));
    // Only the UHD 630 has a second candidate: the planned gen9 mode set below intel-display.
    assert!(plan.devices.iter().all(
        |d| d.alternatives.is_empty() || d.device.label.as_deref() == Some("intel-uhd-gen9.5")
    ));
    let files = select_for_install(&plan, false).packages;
    assert_eq!(
        files,
        [
            "gen-hda-1.0.0.utr",
            "gen-intel-eth-1.0.0.utr",
            "gen-xhci-1.0.0.utr",
            "intel-bt-usb-1.0.0.utr",
            "intel-display-1.0.0.utr",
            "intel-wifi-1.0.0.utr",
            "nvidia-display-0.1.0.utr",
            "nvme-1.0.0.utr",
        ]
    );
    assert_eq!(select_for_install(&plan, true).packages.len(), 11);
}

#[test]
fn beelink_gti15_ultra() {
    let plan = plan_for("beelink-gti15.hw");
    expect(
        &plan,
        &[
            ("intel-e610-port0", "gen-ixgbe"),
            ("intel-e610-port1", "gen-ixgbe"),
            ("crucial-e100-0", "nvme"),
            ("crucial-e100-1", "nvme"),
            ("pch-xhci", "gen-xhci"),
            ("type-c-xhci", "gen-xhci"),
            ("intel-arc-140t", "intel-display"),
            ("hd-audio", "gen-hda"),
            ("intel-be200", "intel-wifi"),
            ("intel-bluetooth", "intel-bt-usb"),
        ],
        &[
            "o2-micro-card-reader",
            "intel-npu",
            "fingerprint",
            "usb-audio",
        ],
    );
    assert_eq!(
        plan.device("pci:ad:00.1")
            .unwrap()
            .chosen
            .as_ref()
            .unwrap()
            .file,
        "gen-ixgbe-1.0.0.utr"
    );
    let files = select_for_install(&plan, false).packages;
    assert_eq!(
        files,
        [
            "gen-hda-1.0.0.utr",
            "gen-ixgbe-1.0.0.utr",
            "gen-xhci-1.0.0.utr",
            "intel-bt-usb-1.0.0.utr",
            "intel-display-1.0.0.utr",
            "intel-wifi-1.0.0.utr",
            "nvme-1.0.0.utr",
        ]
    );
}

#[test]
fn drivers_do_not_cross_machines() {
    let thinkpad = chosen(&plan_for("thinkpad-p1-gen3.hw"));
    let beelink = chosen(&plan_for("beelink-gti15.hw"));
    let uses = |plan: &BTreeMap<String, Option<String>>, id: &str| {
        plan.values().flatten().any(|d| d == id)
    };
    // intel-display (built in) covers display 9 and 14; the planned gen9 mode set ranks below it.
    assert!(uses(&thinkpad, "intel-display") && uses(&beelink, "intel-display"));
    assert!(!uses(&thinkpad, "intel-display-gen9") && !uses(&beelink, "intel-display-gen9"));
    assert!(uses(&thinkpad, "nvidia-display") && !uses(&beelink, "nvidia-display"));
    assert!(uses(&thinkpad, "gen-intel-eth") && !uses(&thinkpad, "gen-ixgbe"));
    assert!(uses(&beelink, "gen-ixgbe") && !uses(&beelink, "gen-intel-eth"));
    // Neither machine has a SATA controller; the AHCI driver is only chosen for one.
    assert!(!uses(&thinkpad, "ahci") && !uses(&beelink, "ahci"));
    let sata = parse_hardware_list("pci 8086:a352 class=010601").unwrap();
    let index = index_of(&example_manifests(), &PrivateKey::generate());
    let plan = resolve(&sata, &index, "ultar-kern-abi-1");
    assert_eq!(plan.devices[0].chosen.as_ref().unwrap().id, "ahci");
}
