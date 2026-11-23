//! End to end (b), (c), (d): tampered packages, remembered choices across restarts,
//! damaged state files, rescans.

mod rig;

use rig::kernel_fmt::{beelink, write, KDev};
use rig::{availability, builtin_manifest, last_states, patch_file, row, summary, Rig};
use ultar_bus::messages::device_service::{
    ClearQuarantine,
    DevicePhase::{Active, Failed, NoDriver},
    Outcome, RefusalKind,
};
use ultar_bus::messages::devices::Availability::{Available, Unavailable};
use ultar_bus::messages::devices::DriverPhase;
use ultar_bus::{Text, TextArg};
use ultar_devd::store::file::{FileStore, CORRUPT_FILE};
use ultar_devd::ChoiceStore;

const NVME0: &str = "pci:01:00.0";
const NVME1: &str = "pci:02:00.0";

fn beelink_rig(tag: &str) -> Rig {
    let rig = Rig::new(tag);
    let machine = beelink();
    machine.write_ultar_root(&rig.root);
    rig
}

fn active(list: &ultar_bus::messages::device_service::ListReply, address: &str) -> String {
    let driver = row(list, address).active.as_ref().unwrap();
    format!("{} {}", driver.id, driver.version)
}

#[test]
fn tampered_package_is_rejected_and_the_device_is_driverless_with_reason() {
    let rig = beelink_rig("tampered");
    rig.standard_depot();
    // Modify the manifest inside the signed package after the index was written.
    let package = rig.depot.join("nvme-1.0.0.utr");
    patch_file(&package, b"rank = 50", b"rank = 51");
    let (mut daemon, memory) = rig.start();
    let list = daemon.list();
    for address in [NVME0, NVME1] {
        let device = row(&list, address);
        assert_eq!(device.phase, Failed);
        assert!(device.active.is_none());
        assert_eq!(
            device.reason,
            Some(
                Text::key("devd.reason.rejected-detail")
                    .with(TextArg::Key("driverpkg.signature.invalid".into()))
            )
        );
        let quarantined = device.alternatives[0].quarantined.as_deref();
        assert_eq!(quarantined, Some("load.package-rejected"));
    }
    // Everything else is unaffected.
    assert_eq!(row(&list, "pci:ad:00.0").phase, Active);
    let published = memory.published();
    let capabilities = availability(published.capabilities.last().unwrap());
    assert!(capabilities.contains(&("storage", Unavailable)));
    assert_eq!(
        last_states(&published)[NVME0],
        ("nvme".to_string(), DriverPhase::Failed)
    );
    // The quarantine is on disk.
    let stored = FileStore::new(&rig.state).load().unwrap();
    assert_eq!(stored.quarantine.len(), 2);
    assert_eq!(stored.quarantine[0].reason.code(), "load.package-rejected");
    assert!(!stored
        .choices
        .keys()
        .any(|device| device.starts_with(NVME0)));

    // The intact package comes back (USB update): a rescan retries and releases the quarantine.
    patch_file(&package, b"rank = 51", b"rank = 50");
    let reply = daemon.rescan(None);
    let devices = reply.devices.unwrap();
    let nvme = devices.iter().find(|d| d.address == NVME0).unwrap();
    assert_eq!((nvme.phase, nvme.reason.clone()), (Active, None));
    assert!(daemon.state().quarantine.is_empty());
    let published = memory.published();
    let capabilities = availability(published.capabilities.last().unwrap());
    assert!(capabilities.contains(&("storage", Available)));
    assert_eq!(
        last_states(&published)[NVME1],
        ("nvme".to_string(), DriverPhase::Active)
    );
}

#[test]
fn swapped_missing_and_foreign_packages() {
    // A validly signed package under another package's file name.
    let rig = beelink_rig("swapped");
    rig.standard_depot();
    std::fs::copy(
        rig.depot.join("gen-xhci-1.0.0.utr"),
        rig.depot.join("nvme-1.0.0.utr"),
    )
    .unwrap();
    let (daemon, _) = rig.start();
    assert_eq!(
        row(&daemon.list(), NVME0).reason,
        Some(
            Text::key("devd.reason.rejected-detail")
                .with(TextArg::Key("devd.detail.wrong-package".into()))
        )
    );
    // The package file was not copied to this system: no candidate, nothing quarantined.
    let rig = beelink_rig("missing");
    rig.standard_depot();
    std::fs::remove_file(rig.depot.join("nvme-1.0.0.utr")).unwrap();
    let (daemon, _) = rig.start();
    let list = daemon.list();
    assert_eq!(row(&list, NVME0).phase, NoDriver);
    assert_eq!(
        row(&list, NVME0).reason.as_ref().unwrap().key,
        "devd.reason.kernel-only"
    );
    assert!(daemon.state().quarantine.is_empty());
    // A package re-signed by a key this system does not trust.
    let rig = beelink_rig("foreign");
    rig.standard_depot();
    let other = Rig::new("foreign-key");
    other.add_repo_drivers();
    std::fs::copy(
        other.depot.join("nvme-1.0.0.utr"),
        rig.depot.join("nvme-1.0.0.utr"),
    )
    .unwrap();
    let (daemon, _) = rig.start();
    assert_eq!(
        row(&daemon.list(), NVME0).reason,
        Some(
            Text::key("devd.reason.rejected-detail")
                .with(TextArg::Key("driverpkg.signature.untrusted-key".into()))
        )
    );
}

#[test]
fn unverifiable_depot_leaves_every_device_driverless_with_a_notice() {
    let rig = beelink_rig("bad-index");
    rig.standard_depot();
    patch_file(&rig.depot.join("index.toml"), b"gen-ixgbe", b"gen-ixgbx");
    let (mut daemon, memory) = rig.start();
    let list = daemon.list();
    assert_eq!(list.devices.len(), 14);
    assert!(list.devices.iter().all(|d| d.phase == NoDriver));
    assert_eq!(
        list.notices[0],
        Text::key("devd.notice.depot-unreadable")
            .with(TextArg::Key("driverpkg.signature.invalid".into()))
    );
    let published = memory.published();
    let capabilities = availability(published.capabilities.last().unwrap());
    assert!(capabilities.iter().all(|(_, a)| *a == Unavailable));
    // No trusted keys at all.
    std::fs::remove_file(rig.keys.join("test.keys")).unwrap();
    write(&rig.keys, "broken.keys", "not a key\n");
    assert!(daemon.rescan(None).devices.is_ok());
    let keys: Vec<String> = daemon
        .list()
        .notices
        .iter()
        .map(|n| n.key.clone())
        .collect();
    assert_eq!(
        keys[..3],
        [
            "devd.notice.keys-unreadable",
            "devd.notice.keys-missing",
            "devd.notice.depot-unreadable"
        ]
    );
    // Repaired depot and keys: a rescan brings the drivers back.
    patch_file(&rig.depot.join("index.toml"), b"gen-ixgbx", b"gen-ixgbe");
    std::fs::remove_file(rig.keys.join("broken.keys")).unwrap();
    write(
        &rig.keys,
        "test.keys",
        &format!("{}\n", rig.key.public().to_hex()),
    );
    let devices = daemon.rescan(None).devices.unwrap();
    assert_eq!(devices.iter().filter(|d| d.phase == Active).count(), 7);
    assert!(daemon.list().notices.is_empty());
}

fn two_nvme_versions(tag: &str) -> Rig {
    let rig = beelink_rig(tag);
    rig.add_repo_drivers();
    let newer = builtin_manifest("nvme", "1.1.0", "storage", "pci_class = [\"010802\"]");
    rig.add_builtin(&newer, "nvme");
    rig.write_index();
    rig
}

#[test]
fn remembered_choice_survives_restart() {
    let rig = two_nvme_versions("remember");
    let (mut daemon, _) = rig.start();
    assert_eq!(active(&daemon.list(), NVME0), "nvme 1.1.0");
    let reply = daemon.switch(NVME0, "nvme", "1.0.0");
    assert!(matches!(reply.outcome, Outcome::Done(_)) && !reply.restored);
    assert_eq!(active(&daemon.list(), NVME0), "nvme 1.0.0");
    drop(daemon);

    // Second start: the remembered driver is used directly and nothing is rewritten.
    let before = std::fs::read(rig.state_file()).unwrap();
    let (mut daemon, memory) = rig.start();
    let list = daemon.list();
    assert_eq!(active(&list, NVME0), "nvme 1.0.0");
    assert_eq!(active(&list, NVME1), "nvme 1.1.0");
    assert_eq!(row(&list, NVME0).previous.as_deref(), Some("nvme 1.1.0"));
    assert_eq!(daemon.state_saves(), 0);
    assert_eq!(std::fs::read(rig.state_file()).unwrap(), before);
    assert_eq!(memory.published().capabilities.len(), 1);
    // The roll-back target survived as well.
    let reply = daemon.rollback(NVME0);
    assert!(matches!(reply.outcome, Outcome::Done(_)));
    drop(daemon);
    let (daemon, _) = rig.start();
    assert_eq!(active(&daemon.list(), NVME0), "nvme 1.1.0");
}

#[test]
fn corrupted_state_files_are_recovered() {
    let rig = two_nvme_versions("corrupt");
    let (mut daemon, _) = rig.start();
    daemon.switch(NVME0, "nvme", "1.0.0");
    drop(daemon);
    let good = std::fs::read(rig.state_file()).unwrap();
    let damaged: [(Vec<u8>, &str); 4] = [
        (good[..good.len() / 2].to_vec(), "devd.store.problem.length"),
        (b"\x00\xff garbage".to_vec(), "devd.store.problem.header"),
        (
            String::from_utf8(good.clone())
                .unwrap()
                .replace("1.0.0", "1.0.9")
                .into_bytes(),
            "devd.store.problem.checksum",
        ),
        (
            String::from_utf8(good.clone())
                .unwrap()
                .replacen("state v1", "state v2", 1)
                .into_bytes(),
            "devd.store.problem.version",
        ),
    ];
    for (bytes, problem) in damaged {
        std::fs::write(rig.state_file(), &bytes).unwrap();
        let (daemon, memory) = rig.start();
        let list = daemon.list();
        // The daemon works; the manual choice is lost with the file and the best driver is used.
        assert_eq!(active(&list, NVME0), "nvme 1.1.0", "{problem}");
        assert_eq!(list.devices.iter().filter(|d| d.phase == Active).count(), 7);
        assert!(list.notices.contains(
            &Text::key("devd.notice.state-recovered").with(TextArg::Key(problem.into()))
        ));
        assert_eq!(memory.published().capabilities.len(), 1);
        // The damaged file is kept, a valid one is written.
        assert_eq!(std::fs::read(rig.state.join(CORRUPT_FILE)).unwrap(), bytes);
        let stored = FileStore::new(&rig.state).load().unwrap();
        assert_eq!(&stored, daemon.state());
        assert_eq!(stored.choices.len(), 7);
        drop(daemon);
        // The next start is clean again.
        let (daemon, _) = rig.start();
        assert!(!daemon
            .list()
            .notices
            .iter()
            .any(|n| n.key.starts_with("devd.notice.state")));
    }
}

#[test]
fn unusable_state_directory_does_not_stop_the_daemon() {
    let rig = two_nvme_versions("no-state");
    std::fs::write(&rig.state, "a file where the state directory should be").unwrap();
    let (mut daemon, _) = rig.start();
    let list = daemon.list();
    assert_eq!(list.devices.iter().filter(|d| d.phase == Active).count(), 7);
    assert!(list.notices.contains(
        &Text::key("devd.notice.state-unavailable")
            .with(TextArg::Key("devd.store.unavailable".into()))
    ));
    // Operations still work in memory.
    assert!(matches!(
        daemon.switch(NVME0, "nvme", "1.0.0").outcome,
        Outcome::Done(_)
    ));
    assert_eq!(active(&daemon.list(), NVME0), "nvme 1.0.0");
}

#[test]
fn rescan_follows_hardware_changes() {
    let rig = beelink_rig("rescan");
    rig.standard_depot();
    let (mut daemon, memory) = rig.start();
    // A second audio controller appears, one network port and a USB device disappear.
    let mut machine = beelink();
    machine.pci.retain(|d| d.slot != "ad:00.1");
    machine.usb.pop();
    machine
        .pci
        .push(KDev::new("03:00.0", 0x10de, 0x10f8, 0x04_0300));
    machine.write_ultar_root(&rig.root);
    let reply = daemon.rescan(None);
    assert_eq!(reply.added, ["pci:03:00.0"]);
    assert_eq!(reply.removed, ["pci:ad:00.1", "usb:1-3"]);
    let devices = reply.devices.unwrap();
    assert_eq!(devices.len(), 13);
    let added = devices.iter().find(|d| d.address == "pci:03:00.0").unwrap();
    assert_eq!(added.active.as_ref().unwrap().id, "gen-hda");
    let states = memory.published().states;
    let tail: Vec<(&str, DriverPhase)> = states[14..]
        .iter()
        .map(|s| (s.device.as_str(), s.phase))
        .collect();
    assert_eq!(
        tail,
        [
            ("pci:ad:00.1", DriverPhase::Removed),
            ("pci:03:00.0", DriverPhase::Active),
            ("usb:1-3", DriverPhase::Removed),
        ]
    );
    // Rescanning one device: only allowed for devices without working driver.
    let refused = daemon.rescan(Some("pci:03:00.0")).devices.unwrap_err();
    assert_eq!(refused.kind, RefusalKind::WrongPhase);
    assert!(daemon.rescan(Some("pci:ab:00.0")).devices.is_ok());
    assert_eq!(
        daemon.rescan(Some("pci:ff:00.0")).devices.unwrap_err().kind,
        RefusalKind::UnknownDevice
    );
    // The system description disappears: refused, devices stay as they were.
    std::fs::remove_dir_all(rig.root.join("sys")).unwrap();
    let reply = daemon.rescan(None);
    assert_eq!(
        reply.devices.unwrap_err().kind,
        RefusalKind::SystemUnreadable
    );
    assert_eq!(daemon.list().devices.len(), 13);
    assert_eq!(
        daemon.list().notices[0].key,
        "devd.notice.hardware-unreadable"
    );
}

#[test]
fn quarantine_can_be_cleared_through_the_daemon() {
    let rig = beelink_rig("quarantine");
    rig.standard_depot();
    patch_file(
        &rig.depot.join("nvme-1.0.0.utr"),
        b"rank = 50",
        b"rank = 51",
    );
    let (mut daemon, _) = rig.start();
    assert_eq!(daemon.state().quarantine.len(), 2);
    let bad = daemon.clear_quarantine(ClearQuarantine {
        address: None,
        driver: Some(("nvme".into(), "1.0.0".into())),
    });
    assert_eq!(bad.released.unwrap_err().kind, RefusalKind::BadRequest);
    let bad = daemon.clear_quarantine(ClearQuarantine {
        address: Some(NVME0.into()),
        driver: Some(("nvme".into(), "one".into())),
    });
    assert_eq!(bad.released.unwrap_err().kind, RefusalKind::BadRequest);
    let unknown = daemon.clear_quarantine(ClearQuarantine {
        address: Some("pci:ff:00.0".into()),
        driver: None,
    });
    assert_eq!(
        unknown.released.unwrap_err().kind,
        RefusalKind::UnknownDevice
    );
    let one = daemon.clear_quarantine(ClearQuarantine {
        address: Some(NVME0.into()),
        driver: Some(("nvme".into(), "1.0.0".into())),
    });
    assert_eq!(one.released, Ok(1));
    assert_eq!(
        daemon.clear_quarantine(ClearQuarantine::default()).released,
        Ok(1)
    );
    assert_eq!(
        daemon.clear_quarantine(ClearQuarantine::default()).released,
        Ok(0)
    );
    assert_eq!(
        FileStore::new(&rig.state).load().unwrap().quarantine.len(),
        0
    );
    assert_eq!(summary(&daemon.list())[5].1, Failed);
}

#[test]
fn start_needs_a_known_kernel() {
    use ultar_devd::bus::MemoryPublisher;
    use ultar_devd::daemon::{Daemon, StartError};
    let rig = Rig::new("no-kernel");
    let start = |config| Daemon::start(config, Box::new(MemoryPublisher::default()));
    let error = start(rig.config()).err().unwrap();
    assert_eq!(error, StartError::UnknownKernel);
    assert_eq!(error.to_string(), "devd.start.unknown-kernel");
    // With the ABI given the daemon starts on an empty system and says what is missing.
    let mut config = rig.config();
    config.kernel_abi = Some("ultar-kern-abi-1".into());
    let daemon = start(config).unwrap();
    let keys: Vec<String> = daemon
        .list()
        .notices
        .iter()
        .map(|n| n.key.clone())
        .collect();
    assert_eq!(
        keys,
        [
            "devd.notice.depot-unreadable",
            "devd.notice.hardware-unreadable"
        ]
    );
    assert!(daemon.list().devices.is_empty());
    // A system description that exists but cannot be read is a start error.
    std::fs::create_dir_all(rig.root.join("sys/kernel/ultar/driver_abi")).unwrap();
    let error = start(rig.config()).err().unwrap();
    assert!(matches!(error, StartError::Hardware(_)));
    assert!(error
        .to_string()
        .starts_with("devd.start.hardware: hal.hardware.io"));
}

#[test]
fn refused_bus_messages_become_a_notice() {
    use ultar_bus::messages::devices::{Capabilities, DriverState};
    use ultar_bus::BusError;
    use ultar_devd::bus::Publisher;
    use ultar_devd::daemon::Daemon;

    /// A bus whose policy denies the daemon's topics.
    struct Denied;
    impl Publisher for Denied {
        fn capabilities(&mut self, _: &Capabilities) -> Result<(), BusError> {
            Err(BusError::NotConnected)
        }
        fn driver_state(&mut self, _: &DriverState) -> Result<(), BusError> {
            Err(BusError::NotConnected)
        }
    }

    let rig = beelink_rig("denied");
    rig.standard_depot();
    let daemon = Daemon::start(rig.config(), Box::new(Denied)).unwrap();
    let list = daemon.list();
    // The devices are managed all the same; the panel is told that nobody heard about it.
    assert_eq!(list.devices.iter().filter(|d| d.phase == Active).count(), 7);
    assert!(list
        .notices
        .contains(&Text::key("devd.notice.publish-failed")));
}

#[test]
fn kernel_device_events_trigger_a_new_reading() {
    let rig = beelink_rig("kernel-events");
    rig.standard_depot();
    write(&rig.root, "sys/kernel/uevent_seqnum", "5\n");
    let (mut daemon, _) = rig.start();
    assert!(!daemon.kernel_events_pending());
    // the kernel reports an event (a device came, went or changed its binding)
    write(&rig.root, "sys/kernel/uevent_seqnum", "6\n");
    assert!(daemon.kernel_events_pending());
    assert!(daemon.rescan(None).devices.is_ok());
    assert!(!daemon.kernel_events_pending());
    // the daemon's own binding changes are taken as seen
    write(&rig.root, "sys/kernel/uevent_seqnum", "8\n");
    daemon.note_kernel_events();
    assert!(!daemon.kernel_events_pending());
}
