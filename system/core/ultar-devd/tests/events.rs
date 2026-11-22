//! Device events: hot-unplug during a probe, removal, repeated announcements, quarantine scope.

mod common;

use common::{boot, index_of, key, manifest, nic, nic_at, three_drivers, NIC};
use ultar_devd::{
    AttemptOutcome, Capability, DevdError, DeviceEvent, FailureReason, HealthFailure,
    HealthVerdict, LoadError, Phase, StoredState,
};
use ultar_driverpkg::{DeviceDescriptor, HardwareId, Version};

const NO_RESPONSE: HealthVerdict = HealthVerdict::Unhealthy(HealthFailure::NoResponse);

fn outcomes(report: &ultar_devd::ProbeReport) -> Vec<(String, AttemptOutcome)> {
    report
        .attempts
        .iter()
        .map(|a| (a.driver.id.clone(), a.outcome))
        .collect()
}

fn failed_health(driver: &str) -> (String, AttemptOutcome) {
    (
        driver.to_string(),
        AttemptOutcome::Failed(FailureReason::Health(HealthFailure::NoResponse)),
    )
}

#[test]
fn hot_unplug_while_loading() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.fail_load("alpha", LoadError::DeviceGone);
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Removed]
    );
    assert_eq!(
        outcomes(&report),
        [("alpha".to_string(), AttemptOutcome::DeviceGone)]
    );
    assert_eq!(report.active, None);
    assert_eq!(rig.manager.phase(NIC), None);
    assert!(rig.manager.devices().is_empty());
    assert!(rig.events().is_empty());
    assert_eq!(rig.log(), ["load alpha pci:00:1f.6"]);
    assert_eq!(rig.stored(), StoredState::default());
    assert_eq!(rig.manager.store().saves(), 0);
}

#[test]
fn hot_unplug_during_the_health_check_discards_failures_of_that_probe() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.verdict("alpha", NO_RESPONSE);
    rig.verdict("beta", HealthVerdict::DeviceGone);
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Removed]
    );
    assert_eq!(
        outcomes(&report),
        [
            failed_health("alpha"),
            ("beta".to_string(), AttemptOutcome::DeviceGone)
        ]
    );
    assert!(!report.remembered_failed);
    // beta was loaded and is unloaded again; gamma is never tried.
    let log = rig.log();
    assert_eq!(
        log.last().map(String::as_str),
        Some("unload beta pci:00:1f.6")
    );
    assert!(!log.iter().any(|l| l.contains("gamma")));
    // alpha's failure happened while the device was going away: no quarantine.
    assert!(rig.manager.quarantine().is_empty());
    assert_eq!(rig.manager.phase(NIC), None);

    // Plugged in again (healthy now): a normal first-time search.
    rig.heal("alpha");
    rig.heal("beta");
    let report = rig.add(nic());
    assert_eq!(report.active, Some(key("alpha")));
    // A remembered choice survives an unplug during its own probe.
    let mut state = StoredState::default();
    state.choices.insert(nic().identity(), key("beta"));
    let mut rig = boot(&three_drivers(), state.clone());
    rig.fail_load("beta", LoadError::DeviceGone);
    let report = rig.add(nic());
    assert!(!report.remembered_failed && !report.used_remembered);
    assert_eq!(rig.manager.state(), &state);
}

#[test]
fn removal_unloads_and_readding_uses_the_remembered_driver() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.take_log();
    let report = rig
        .manager
        .handle_event(DeviceEvent::Removed(NIC.to_string()))
        .unwrap();
    assert_eq!(report.transitions, [Phase::Removed]);
    assert_eq!(rig.take_log(), ["unload alpha pci:00:1f.6"]);
    assert_eq!(rig.events(), ["+alpha", "-alpha"]);
    let lost = rig.world.borrow().events[1].clone();
    assert_eq!(lost.capabilities, [Capability::new("driver", "alpha")]);
    assert_eq!(rig.manager.phase(NIC), None);
    assert_eq!(
        rig.stored().choices.len(),
        1,
        "the choice is kept for the next plug-in"
    );

    let report = rig.add(nic());
    assert!(report.used_remembered);
    assert_eq!(rig.events(), ["+alpha", "-alpha", "+alpha"]);
    let unknown = rig
        .manager
        .handle_event(DeviceEvent::Removed("pci:nope".into()))
        .unwrap_err();
    assert_eq!(unknown, DevdError::UnknownDevice("pci:nope".into()));

    // Removing a device without driver emits nothing.
    let gpu = HardwareId::Pci {
        vendor: 0x10de,
        device: 0x1eb6,
        class: 0x030000,
        subsys: None,
    };
    rig.add(DeviceDescriptor::new("pci:01:00.0", gpu));
    let before = rig.events().len();
    rig.manager
        .handle_event(DeviceEvent::Removed("pci:01:00.0".into()))
        .unwrap();
    assert_eq!(rig.events().len(), before);
}

#[test]
fn repeated_and_replaced_add_events() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.add(nic());
    rig.take_log();
    // The same device announced again (coldplug after hotplug): nothing happens.
    let report = rig.add(nic().with_label("i219"));
    assert!(report.transitions.is_empty() && report.attempts.is_empty());
    assert_eq!(report.active, Some(key("alpha")));
    assert!(rig.take_log().is_empty());
    assert_eq!(
        rig.manager.devices()[0].device.label.as_deref(),
        Some("i219")
    );

    // Different hardware at the same address replaces the old device.
    let other = HardwareId::Pci {
        vendor: 0x8086,
        device: 0x15b8,
        class: 0x020000,
        subsys: None,
    };
    let report = rig.add(DeviceDescriptor::new(NIC, other));
    assert_eq!(
        report.transitions,
        [
            Phase::Removed,
            Phase::Detected,
            Phase::Probing,
            Phase::Active
        ]
    );
    assert_eq!(report.active, Some(key("beta")));
    assert_eq!(rig.events(), ["+alpha", "-alpha", "+beta"]);
    assert_eq!(rig.stored().choices.len(), 2);

    // A device that failed before is searched again when it is announced again.
    let mut rig = boot(&three_drivers(), StoredState::default());
    for driver in ["alpha", "beta", "gamma"] {
        rig.verdict(driver, NO_RESPONSE);
    }
    assert_eq!(rig.add(nic()).final_phase(), Some(Phase::Failed));
    rig.heal("gamma");
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(report.active, Some(key("gamma")));
}

#[test]
fn quarantine_is_per_device_and_per_version() {
    let index = three_drivers();
    let mut rig = boot(&index, StoredState::default());
    rig.verdict("alpha@pci:02:00.0", NO_RESPONSE);
    assert_eq!(rig.add(nic_at("pci:01:00.0")).active, Some(key("alpha")));
    assert_eq!(rig.add(nic_at("pci:02:00.0")).active, Some(key("beta")));
    let stored = rig.stored();
    assert_eq!(stored.quarantine.len(), 1);
    assert_eq!(
        stored.quarantine[0].device,
        nic_at("pci:02:00.0").identity()
    );
    assert_eq!(rig.manager.phase("pci:01:00.0"), Some(Phase::Active));

    // A new version of the quarantined driver gets its chance; the old version stays out.
    let mut state = StoredState::default();
    state.quarantine_driver(
        &nic().identity(),
        key("alpha"),
        FailureReason::Health(HealthFailure::NoResponse),
    );
    let newer = index_of(&[
        manifest("alpha", "1.0.0", "pci = [\"8086:0d4c\"]"),
        manifest("alpha", "1.1.0", "pci = [\"8086:0d4c\"]"),
        manifest("beta", "1.0.0", "pci = [\"8086:*\"]"),
    ]);
    let mut rig = boot(&newer, state);
    let report = rig.add(nic());
    assert_eq!(
        report.active.as_ref().map(|k| k.version.clone()),
        Some(Version::new(1, 1, 0))
    );
    assert_eq!(report.attempts.len(), 1);
    assert_eq!(rig.stored().quarantine.len(), 1);
}
