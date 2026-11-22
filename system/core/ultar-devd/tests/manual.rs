//! Manual operations of the settings panel: list, switch, roll back, quarantine, rescan; store faults.

mod common;

use common::{boot, boot_with, index_of, key, manifest, nic, three_drivers, ABI, NIC};
use ultar_devd::{
    AttemptOutcome, Capability, Config, DevdError, FailureReason, HealthFailure, HealthVerdict,
    LoadError, LoadFailure, Phase, QuarantineFilter, StoredState,
};
use ultar_driverpkg::{DeviceDescriptor, HardwareId};

const NO_RESPONSE: HealthVerdict = HealthVerdict::Unhealthy(HealthFailure::NoResponse);
const HEALTH_NO_RESPONSE: FailureReason = FailureReason::Health(HealthFailure::NoResponse);

fn gpu() -> DeviceDescriptor {
    let id = HardwareId::Pci {
        vendor: 0x10de,
        device: 0x1eb6,
        class: 0x030000,
        subsys: None,
    };
    DeviceDescriptor::new("pci:01:00.0", id)
}

#[test]
fn lists_devices_with_driver_alternatives_and_state() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.verdict("alpha", NO_RESPONSE);
    rig.add(nic());
    rig.add(gpu());
    let views = rig.manager.devices();
    assert_eq!(views.len(), 2);
    let (nic_view, gpu_view) = (&views[0], &views[1]);
    assert_eq!(nic_view.device.address, NIC);
    assert_eq!(nic_view.phase, Phase::Active);
    assert_eq!(
        nic_view.active.as_ref().map(|c| c.id.as_str()),
        Some("beta")
    );
    assert_eq!(nic_view.capabilities, [Capability::new("driver", "beta")]);
    assert_eq!(nic_view.previous, None);
    let alternatives: Vec<(&str, Option<FailureReason>)> = nic_view
        .alternatives
        .iter()
        .map(|a| (a.driver.id.as_str(), a.quarantined))
        .collect();
    assert_eq!(
        alternatives,
        [("alpha", Some(HEALTH_NO_RESPONSE)), ("gamma", None)]
    );
    assert_eq!(
        nic_view.alternatives[1].driver.name_key,
        "driver.gamma.name"
    );

    assert_eq!(gpu_view.phase, Phase::NoDriver);
    assert!(
        gpu_view.active.is_none()
            && gpu_view.alternatives.is_empty()
            && gpu_view.capabilities.is_empty()
    );
}

#[test]
fn switch_and_roll_back() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.take_log();

    let report = rig.manager.switch_driver(NIC, &key("gamma")).unwrap();
    assert_eq!(report.transitions, [Phase::Probing, Phase::Active]);
    assert_eq!(report.active, Some(key("gamma")));
    assert!(!report.restored);
    assert_eq!(
        rig.take_log(),
        [
            "unload alpha pci:00:1f.6",
            "load gamma pci:00:1f.6",
            "check gamma pci:00:1f.6"
        ]
    );
    assert_eq!(rig.events(), ["+alpha", "-alpha", "+gamma"]);
    let stored = rig.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("gamma")));
    assert_eq!(stored.previous.get(&nic().identity()), Some(&key("alpha")));
    assert_eq!(rig.manager.devices()[0].previous, Some(key("alpha")));

    let report = rig.manager.roll_back(NIC).unwrap();
    assert_eq!(report.transitions, [Phase::Probing, Phase::Active]);
    assert_eq!(report.active, Some(key("alpha")));
    assert_eq!(
        rig.events(),
        ["+alpha", "-alpha", "+gamma", "-gamma", "+alpha"]
    );
    let stored = rig.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("alpha")));
    // Rolling back again returns to the driver we just left.
    assert_eq!(stored.previous.get(&nic().identity()), Some(&key("gamma")));
    assert_eq!(
        rig.manager.roll_back(NIC).unwrap().active,
        Some(key("gamma"))
    );
    assert!(stored.quarantine.is_empty());
}

#[test]
fn the_switched_driver_and_the_rollback_target_survive_a_reboot() {
    let index = three_drivers();
    let mut first = boot(&index, StoredState::default());
    first.add(nic());
    first.manager.switch_driver(NIC, &key("beta")).unwrap();

    let mut second = boot(&index, first.stored());
    let report = second.add(nic());
    assert_eq!(report.active, Some(key("beta")));
    assert!(report.used_remembered);
    assert_eq!(
        second.manager.roll_back(NIC).unwrap().active,
        Some(key("alpha"))
    );
}

#[test]
fn a_failed_switch_restores_the_previous_driver() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.verdict("gamma", NO_RESPONSE);
    rig.take_log();
    let report = rig.manager.switch_driver(NIC, &key("gamma")).unwrap();
    assert_eq!(report.transitions, [Phase::Probing, Phase::Active]);
    assert!(report.restored);
    assert_eq!(report.active, Some(key("alpha")));
    assert_eq!(
        report.attempts[0].outcome,
        AttemptOutcome::Failed(HEALTH_NO_RESPONSE)
    );
    assert_eq!(report.attempts[1].outcome, AttemptOutcome::Worked);
    assert_eq!(
        rig.take_log(),
        [
            "unload alpha pci:00:1f.6",
            "load gamma pci:00:1f.6",
            "check gamma pci:00:1f.6",
            "unload gamma pci:00:1f.6",
            "load alpha pci:00:1f.6",
            "check alpha pci:00:1f.6"
        ]
    );
    assert_eq!(rig.events(), ["+alpha", "-alpha", "+alpha"]);
    let stored = rig.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("alpha")));
    assert!(
        stored.previous.is_empty(),
        "a failed switch leaves no roll-back target"
    );
    assert_eq!(stored.quarantine.len(), 1);
    assert_eq!(stored.quarantine[0].driver, key("gamma"));
    // The failed driver is now refused until its quarantine is cleared.
    let refused = rig.manager.switch_driver(NIC, &key("gamma")).unwrap_err();
    assert_eq!(
        refused,
        DevdError::Quarantined {
            device: NIC.into(),
            driver: key("gamma")
        }
    );
}

#[test]
fn a_failed_switch_with_a_broken_previous_driver_searches_on() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.verdict("gamma", NO_RESPONSE);
    rig.fail_load("alpha", LoadError::Failed(LoadFailure::InitFailed));
    let report = rig.manager.switch_driver(NIC, &key("gamma")).unwrap();
    let tried: Vec<&str> = report
        .attempts
        .iter()
        .map(|a| a.driver.id.as_str())
        .collect();
    assert_eq!(tried, ["gamma", "alpha", "beta"]);
    assert_eq!(report.active, Some(key("beta")));
    assert!(!report.restored);
    assert_eq!(rig.stored().quarantine.len(), 2);

    // Nothing works at all: the device ends up failed and nothing stays remembered.
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    for driver in ["alpha", "beta", "gamma"] {
        rig.verdict(driver, NO_RESPONSE);
    }
    let report = rig.manager.switch_driver(NIC, &key("beta")).unwrap();
    assert_eq!(report.transitions, [Phase::Probing, Phase::Failed]);
    assert_eq!(report.active, None);
    assert!(!report.restored);
    assert_eq!(rig.manager.phase(NIC), Some(Phase::Failed));
    assert!(rig.stored().choices.is_empty());
    assert_eq!(rig.events(), ["+alpha", "-alpha"]);
}

#[test]
fn hot_unplug_during_a_switch() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.verdict("beta", HealthVerdict::DeviceGone);
    let report = rig.manager.switch_driver(NIC, &key("beta")).unwrap();
    assert_eq!(report.transitions, [Phase::Probing, Phase::Removed]);
    assert_eq!(report.attempts[0].outcome, AttemptOutcome::DeviceGone);
    assert_eq!(rig.manager.phase(NIC), None);
    assert!(rig.manager.quarantine().is_empty());
    assert_eq!(rig.events(), ["+alpha", "-alpha"]);
    // The earlier choice is still remembered for the next plug-in.
    assert_eq!(
        rig.stored().choices.get(&nic().identity()),
        Some(&key("alpha"))
    );
}

#[test]
fn switching_a_failed_device_to_a_chosen_driver() {
    let index = three_drivers();
    let mut state = StoredState::default();
    for driver in ["alpha", "beta"] {
        state.quarantine_driver(&nic().identity(), key(driver), HEALTH_NO_RESPONSE);
    }
    let config = Config {
        last_resort_retry: false,
        ..Config::new(ABI)
    };
    let mut rig = boot_with(&index, ultar_devd::MemoryStore::with_state(state), config);
    rig.verdict("gamma", NO_RESPONSE);
    assert_eq!(rig.add(nic()).final_phase(), Some(Phase::Failed));
    rig.heal("gamma");
    assert_eq!(
        rig.manager
            .clear_quarantine(&QuarantineFilter::Device(NIC.into())),
        Ok(3)
    );
    let report = rig.manager.switch_driver(NIC, &key("beta")).unwrap();
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(report.active, Some(key("beta")));
    assert!(rig.manager.state().previous.is_empty());
}

#[test]
fn refused_operations() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.add(gpu());
    let m = &mut rig.manager;
    assert_eq!(
        m.switch_driver("pci:nope", &key("alpha")),
        Err(DevdError::UnknownDevice("pci:nope".into()))
    );
    assert_eq!(
        m.switch_driver(NIC, &key("delta")),
        Err(DevdError::NotACandidate {
            device: NIC.into(),
            driver: key("delta")
        })
    );
    assert_eq!(
        m.switch_driver("pci:01:00.0", &key("alpha")),
        Err(DevdError::NotACandidate {
            device: "pci:01:00.0".into(),
            driver: key("alpha")
        })
    );
    assert_eq!(
        m.switch_driver(NIC, &key("alpha")),
        Err(DevdError::AlreadyActive(key("alpha")))
    );
    assert_eq!(
        m.roll_back(NIC),
        Err(DevdError::NothingToRollBack(NIC.into()))
    );
    assert_eq!(
        m.roll_back("pci:nope"),
        Err(DevdError::UnknownDevice("pci:nope".into()))
    );
    assert_eq!(
        m.rescan(NIC),
        Err(DevdError::WrongPhase {
            device: NIC.into(),
            phase: Phase::Active
        })
    );
    assert_eq!(
        m.rescan("pci:nope"),
        Err(DevdError::UnknownDevice("pci:nope".into()))
    );
    assert_eq!(
        m.clear_quarantine(&QuarantineFilter::Device("pci:nope".into())),
        Err(DevdError::UnknownDevice("pci:nope".into()))
    );
    // Refused operations change nothing.
    assert_eq!(rig.manager.phase(NIC), Some(Phase::Active));
    assert_eq!(rig.events(), ["+alpha"]);
    assert_eq!(rig.manager.store().saves(), 1);
}

#[test]
fn quarantine_persists_across_boots_until_cleared() {
    let index = three_drivers();
    let mut first = boot(&index, StoredState::default());
    first.verdict("alpha", NO_RESPONSE);
    first.add(nic());
    let stored = first.stored();
    assert_eq!(stored.quarantine.len(), 1);

    // Next boot, alpha would work again – but it stays in quarantine, even without a remembered choice.
    let mut without_choice = stored.clone();
    without_choice.choices.clear();
    let mut second = boot(&index, without_choice);
    let report = second.add(nic());
    assert_eq!(report.active, Some(key("beta")));
    assert!(!second.log().iter().any(|l| l.contains("alpha")));
    assert_eq!(second.manager.quarantine().len(), 1);
    assert_eq!(second.manager.quarantine()[0].reason, HEALTH_NO_RESPONSE);
    assert_eq!(
        second.manager.switch_driver(NIC, &key("alpha")),
        Err(DevdError::Quarantined {
            device: NIC.into(),
            driver: key("alpha")
        })
    );

    // The user clears it: the driver is usable again, and the cleared state is persisted.
    let filter = QuarantineFilter::Driver {
        device: NIC.into(),
        driver: key("alpha"),
    };
    assert_eq!(second.manager.clear_quarantine(&filter), Ok(1));
    assert_eq!(second.manager.clear_quarantine(&filter), Ok(0));
    assert!(second.stored().quarantine.is_empty());
    assert_eq!(
        second
            .manager
            .switch_driver(NIC, &key("alpha"))
            .unwrap()
            .active,
        Some(key("alpha"))
    );

    let mut third = boot(&index, second.stored());
    assert_eq!(third.add(nic()).active, Some(key("alpha")));
}

#[test]
fn clear_quarantine_filters() {
    let index = three_drivers();
    let other = common::nic_at("pci:02:00.0");
    let mut state = StoredState::default();
    state.quarantine_driver(&nic().identity(), key("alpha"), HEALTH_NO_RESPONSE);
    state.quarantine_driver(&nic().identity(), key("beta"), HEALTH_NO_RESPONSE);
    state.quarantine_driver(&other.identity(), key("alpha"), HEALTH_NO_RESPONSE);
    state.quarantine_driver("usb:gone=usb:1234:5678", key("old"), HEALTH_NO_RESPONSE);

    let mut rig = boot(&index, state.clone());
    rig.add(nic());
    rig.add(other.clone());
    assert_eq!(
        rig.manager
            .clear_quarantine(&QuarantineFilter::Device(NIC.into())),
        Ok(2)
    );
    let left: Vec<&str> = rig
        .manager
        .quarantine()
        .iter()
        .map(|e| e.device.as_str())
        .collect();
    assert_eq!(left, [other.identity().as_str(), "usb:gone=usb:1234:5678"]);
    assert_eq!(rig.stored().quarantine.len(), 2);
    // Entries of devices that are not present can only be cleared with `All`.
    assert_eq!(rig.manager.clear_quarantine(&QuarantineFilter::All), Ok(2));
    assert!(rig.stored().quarantine.is_empty());
    assert_eq!(rig.manager.clear_quarantine(&QuarantineFilter::All), Ok(0));

    let mut rig = boot(&index, state);
    rig.add(nic());
    let filter = QuarantineFilter::Driver {
        device: NIC.into(),
        driver: key("beta"),
    };
    assert_eq!(rig.manager.clear_quarantine(&filter), Ok(1));
    assert_eq!(rig.manager.quarantine().len(), 3);
}

#[test]
fn rescan_after_a_new_index_or_cleared_quarantine() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    assert_eq!(rig.add(gpu()).final_phase(), Some(Phase::NoDriver));
    assert_eq!(
        rig.manager.rescan("pci:01:00.0").unwrap().transitions,
        [Phase::Detected, Phase::NoDriver]
    );

    // A driver update brings a matching driver.
    let mut manifests = vec![manifest("nv-display", "1.0.0", "pci = [\"10de:1eb6\"]")];
    manifests.push(manifest("alpha", "1.0.0", "pci = [\"8086:0d4c\"]"));
    rig.manager.replace_index(index_of(&manifests));
    let report = rig.manager.rescan("pci:01:00.0").unwrap();
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(report.active, Some(key("nv-display")));
    assert_eq!(rig.events(), ["+nv-display"]);

    // A failed device recovers after the quarantine is cleared.
    let config = Config {
        last_resort_retry: false,
        ..Config::new(ABI)
    };
    let mut rig = boot_with(&three_drivers(), ultar_devd::MemoryStore::default(), config);
    for driver in ["alpha", "beta", "gamma"] {
        rig.verdict(driver, NO_RESPONSE);
    }
    rig.add(nic());
    rig.heal("beta");
    assert_eq!(
        rig.manager.rescan(NIC).unwrap().transitions,
        [Phase::Detected, Phase::Probing, Phase::Failed]
    );
    assert_eq!(rig.manager.clear_quarantine(&QuarantineFilter::All), Ok(3));
    rig.verdict("alpha", NO_RESPONSE);
    let report = rig.manager.rescan(NIC).unwrap();
    assert_eq!(report.active, Some(key("beta")));
    assert_eq!(rig.manager.phase(NIC), Some(Phase::Active));
}
