//! End to end (a): the two test machines against a depot built in the test.
//!
//! The Beelink runs on Ultar kernel texts (log after a self-test boot, device
//! description); the ThinkPad on a Linux-style sysfs tree whose `driver` links follow the
//! Ultar kernel's match rules. Both are fixtures – see INTEGRATION.md for what that proves.

mod rig;

use rig::kernel_fmt::{beelink, thinkpad, write};
use rig::{availability, last_states, row, summary, Rig};
use std::collections::BTreeMap;
use ultar_bus::messages::device_service::DevicePhase::{Active, Failed, NoDriver};
use ultar_bus::messages::device_service::{DeviceInfo, DevicePhase, ListReply};
use ultar_bus::messages::devices::Availability::{Available, Unavailable};
use ultar_bus::messages::devices::DriverPhase;
use ultar_bus::{Text, TextArg};
use ultar_devd::bus::MemoryPublisher;
use ultar_devd::daemon::Daemon;

fn s(text: &str) -> String {
    text.to_string()
}

fn beelink_selftest(tag: &str) -> (Rig, Daemon, MemoryPublisher) {
    let rig = Rig::new(tag);
    rig.standard_depot();
    let machine = beelink();
    machine.write_ultar_root(&rig.root);
    let (daemon, memory) = rig.start();
    (rig, daemon, memory)
}

type Row = (String, DevicePhase, String, String);

fn beelink_rows() -> Vec<Row> {
    let no_package = "devd.reason.no-package";
    let not_loadable = "devd.reason.not-loadable";
    vec![
        (s("pci:00:02.0"), Failed, s(""), s("devd.reason.not-bound")),
        (s("pci:00:0b.0"), NoDriver, s(""), s(no_package)),
        (s("pci:00:0d.0"), Active, s("gen-xhci"), s("")),
        (s("pci:00:14.0"), Active, s("gen-xhci"), s("")),
        (s("pci:00:1f.3"), Active, s("gen-hda"), s("")),
        (s("pci:01:00.0"), Active, s("nvme"), s("")),
        (s("pci:02:00.0"), Active, s("nvme"), s("")),
        (s("pci:ab:00.0"), NoDriver, s(""), s(no_package)),
        (s("pci:ac:00.0"), Failed, s(""), s(not_loadable)),
        (s("pci:ad:00.0"), Active, s("gen-ixgbe"), s("")),
        (s("pci:ad:00.1"), Active, s("gen-ixgbe"), s("")),
        (s("usb:1-1"), NoDriver, s(""), s(no_package)),
        (s("usb:1-2"), Failed, s(""), s(not_loadable)),
        (s("usb:1-3"), NoDriver, s(""), s(no_package)),
    ]
}

#[test]
fn beelink_devices_after_a_selftest_boot_of_the_ultar_kernel() {
    let (rig, daemon, _) = beelink_selftest("beelink");
    let list = daemon.list();
    assert_eq!(summary(&list), beelink_rows());
    assert_eq!(row(&list, "pci:01:00.0").capabilities, ["storage"]);
    let wifi = &row(&list, "pci:ac:00.0").alternatives[0];
    assert_eq!(wifi.id, "intel-wifi");
    assert!(wifi.quarantined.is_none());
    // the kernel has bind/unbind control files (drivers without teardown are still refused)
    assert!(list.kernel_switch_supported);
    // The kernel's device tree is complete: nothing to warn about.
    assert!(list.notices.is_empty());
    // Seven working choices are remembered, nothing is quarantined.
    assert_eq!(daemon.state().choices.len(), 7);
    assert!(daemon.state().quarantine.is_empty());
    assert!(rig.state_file().exists());
}

#[test]
fn beelink_capabilities_and_driver_states_are_published() {
    let (_rig, daemon, memory) = beelink_selftest("beelink-published");
    let published = memory.published();
    // One retained capability message for the whole start-up.
    assert_eq!(published.capabilities.len(), 1);
    assert_eq!(published.capabilities[0].revision, 1);
    assert_eq!(
        availability(&published.capabilities[0]),
        // the kernel's device tree gives the USB class too: the Bluetooth adapter is seen
        [
            ("audio_out", Available),
            ("bluetooth", Unavailable),
            ("bus", Available),
            ("gpu", Unavailable),
            ("net", Available),
            ("npu", Unavailable),
            ("storage", Available),
            ("wifi", Unavailable),
        ]
    );
    assert_eq!(daemon.capabilities(), published.capabilities[0]);
    // One driver state per device, none repeated.
    assert_eq!(published.states.len(), 14);
    let states = last_states(&published);
    let state = |address: &str| states[address].clone();
    assert_eq!(state("pci:ad:00.1"), (s("gen-ixgbe"), DriverPhase::Active));
    let display = (s("intel-display"), DriverPhase::Failed);
    assert_eq!(state("pci:00:02.0"), display);
    assert_eq!(state("pci:ab:00.0"), (s(""), DriverPhase::Missing));
    assert_eq!(state("usb:1-2"), (s("intel-bt-usb"), DriverPhase::Failed));
    let wifi = published.states.iter().find(|x| x.device == "pci:ac:00.0");
    let reason = Text::key("devd.reason.not-loadable");
    assert_eq!(wifi.unwrap().reason, Some(reason));
}

/// ThinkPad on a sysfs tree whose `driver` links follow the Ultar kernel's match rules.
struct ThinkPad {
    list: ListReply,
    memory: MemoryPublisher,
    by_label: BTreeMap<String, String>,
}

fn thinkpad_on_sysfs(tag: &str) -> ThinkPad {
    let rig = Rig::new(tag);
    rig.standard_depot();
    let machine = thinkpad();
    machine.write_sysfs_root(&rig.root, true);
    // Kernel identification of the Ultar kernel (the packages are built for its ABI).
    write(&rig.root, "sys/kernel/ultar/driver_abi", "1\n");
    let (daemon, memory) = rig.start();
    let by_label = machine
        .pci
        .iter()
        .map(|d| (d.label.clone().unwrap(), d.address()))
        .collect();
    ThinkPad {
        list: daemon.list(),
        memory,
        by_label,
    }
}

impl ThinkPad {
    fn device(&self, label: &str) -> &DeviceInfo {
        row(&self.list, &self.by_label[label])
    }

    fn state(&self, label: &str) -> (DevicePhase, String, String) {
        let device = self.device(label);
        let driver = device.active.as_ref().map(|a| a.id.clone());
        let reason = device.reason.as_ref().map(|r| r.key.clone());
        (
            device.phase,
            driver.unwrap_or_default(),
            reason.unwrap_or_default(),
        )
    }
}

#[test]
fn thinkpad_devices_with_packages() {
    let t = thinkpad_on_sysfs("thinkpad-packages");
    assert_eq!(t.list.devices.len(), 26);
    assert!(t.list.notices.is_empty(), "{:?}", t.list.notices);
    for (label, driver) in [
        ("samsung-pm981a", "nvme"),
        ("pch-xhci", "gen-xhci"),
        ("thunderbolt-xhci", "gen-xhci"),
        ("nvidia-usb-c-xhci", "gen-xhci"),
        ("intel-i219-lm", "gen-intel-eth"),
        ("nvidia-hdmi-audio", "gen-hda"),
    ] {
        assert_eq!(t.state(label), (Active, s(driver), s("")), "{label}");
    }
    // Packages exist but need loadable modules (stage B).
    let not_loadable = (Failed, s(""), s("devd.reason.not-loadable"));
    assert_eq!(t.state("intel-ax201"), not_loadable);
    // Built into the kernel (intel-display, nvidia-display), but the recorded kernel did not
    // bind them: the opt-in was off.
    let not_bound = (Failed, s(""), s("devd.reason.not-bound"));
    for label in ["intel-uhd-gen9.5", "nvidia-quadro-rtx-4000"] {
        assert_eq!(t.state(label), not_bound, "{label}");
    }
    assert_eq!(row(&t.list, "usb:1-1").phase, Failed);
    // The gen-hda package lists the cAVS id, but the kernel's gen-hda only takes class
    // 04.03 and this controller reports 04.01: the kernel did not bind it.
    let not_bound = Text::key("devd.reason.not-bound").with(TextArg::Verbatim(s("gen-hda")));
    assert_eq!(t.device("intel-cavs-audio").reason, Some(not_bound));
    assert_eq!(t.state("intel-cavs-audio").0, Failed);
}

#[test]
fn thinkpad_devices_without_packages() {
    let t = thinkpad_on_sysfs("thinkpad-driverless");
    // The kernel runs these with built-in drivers that have no package.
    for (label, kernel_driver) in [
        ("host-bridge", "bruecke"),
        ("lpc", "bruecke"),
        ("thunderbolt-pcie-switch", "bruecke"),
        ("smbus", "gen-smbus"),
    ] {
        let expected =
            Text::key("devd.reason.kernel-only").with(TextArg::Verbatim(s(kernel_driver)));
        assert_eq!(t.device(label).reason, Some(expected), "{label}");
    }
    let no_package = (NoDriver, s(""), s("devd.reason.no-package"));
    for label in [
        "nvidia-usb-c-ucsi",
        "thunderbolt-controller",
        "lpss-i2c-0",
        "lpss-i2c-1",
        "genesys-card-reader",
        "thermal",
        "mei",
        "spi",
        "amt-sol",
    ] {
        assert_eq!(t.state(label), no_package, "{label}");
    }
    // USB: camera and fingerprint reader have no package.
    assert_eq!(row(&t.list, "usb:1-2").phase, NoDriver);
    assert_eq!(row(&t.list, "usb:1-3").phase, NoDriver);
}

#[test]
fn thinkpad_capabilities_and_driver_states_are_published() {
    let t = thinkpad_on_sysfs("thinkpad-published");
    let published = t.memory.published();
    assert_eq!(published.capabilities.len(), 1);
    assert_eq!(
        availability(&published.capabilities[0]),
        [
            ("audio_out", Available),
            ("bluetooth", Unavailable),
            ("bus", Available),
            ("gpu", Unavailable),
            ("net", Available),
            ("storage", Available),
            ("wifi", Unavailable),
        ]
    );
    let states = last_states(&published);
    assert_eq!(states.len(), 26);
    let count = |phase: DriverPhase| states.values().filter(|(_, p)| *p == phase).count();
    assert_eq!(count(DriverPhase::Active), 6);
    assert_eq!(count(DriverPhase::Failed), 5);
    assert_eq!(count(DriverPhase::Missing), 15);
}

#[test]
fn capability_names_agree_with_the_profile_crate() {
    use ultar_devd::caps::{class_capability, hardware_hint};
    use ultar_driverpkg::{DriverClass, HardwareId};
    use ultar_profile::Capability;
    for (class, capability) in [
        (DriverClass::Display, Capability::Gpu),
        (DriverClass::Audio, Capability::AudioOut),
        (DriverClass::Wifi, Capability::Wifi),
        (DriverClass::Bluetooth, Capability::Bluetooth),
        (DriverClass::Modem, Capability::Modem),
        (DriverClass::Camera, Capability::Camera),
        (DriverClass::Sensor, Capability::Sensors),
    ] {
        assert_eq!(class_capability(class), capability.as_str());
    }
    let npu = HardwareId::Pci {
        vendor: 0x8086,
        device: 0x7d1d,
        class: 0x12_0000,
        subsys: None,
    };
    assert_eq!(hardware_hint(&npu), Some(Capability::Npu.as_str()));
}
