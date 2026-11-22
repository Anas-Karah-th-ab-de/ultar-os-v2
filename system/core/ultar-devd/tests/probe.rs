//! Automatic driver selection: every path of the state machine with mock loader and health check.

mod common;

use common::{boot, boot_with, key, nic, three_drivers, ABI, NIC};
use ultar_devd::{
    AttemptOutcome, Capability, ChangeKind, Config, FailureReason, HealthFailure, HealthVerdict,
    LoadError, LoadFailure, MemoryStore, Phase, StoredState,
};
use ultar_driverpkg::{DeviceDescriptor, DriverClass, HardwareId};

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

fn worked(driver: &str) -> (String, AttemptOutcome) {
    (driver.to_string(), AttemptOutcome::Worked)
}

#[test]
fn success_on_first_try() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(outcomes(&report), [worked("alpha")]);
    assert_eq!(report.active, Some(key("alpha")));
    assert!(!report.used_remembered && !report.remembered_failed && !report.restored);
    assert_eq!(report.store_error, None);
    assert_eq!(
        rig.log(),
        ["load alpha pci:00:1f.6", "check alpha pci:00:1f.6"]
    );
    assert_eq!(rig.manager.phase(NIC), Some(Phase::Active));

    let world = rig.world.borrow();
    assert_eq!(world.events.len(), 1);
    let event = &world.events[0];
    assert_eq!(
        (event.device.as_str(), event.kind, event.class),
        (NIC, ChangeKind::Gained, DriverClass::Net)
    );
    assert_eq!(event.driver, key("alpha"));
    assert_eq!(event.capabilities, [Capability::new("driver", "alpha")]);
    drop(world);

    let stored = rig.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("alpha")));
    assert!(stored.quarantine.is_empty() && stored.previous.is_empty());
    assert_eq!(rig.manager.store().saves(), 1);
}

#[test]
fn falls_back_to_the_next_candidate_and_quarantines_the_failed_one() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.verdict("alpha", NO_RESPONSE);
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(outcomes(&report), [failed_health("alpha"), worked("beta")]);
    assert_eq!(
        rig.log(),
        [
            "load alpha pci:00:1f.6",
            "check alpha pci:00:1f.6",
            "unload alpha pci:00:1f.6",
            "load beta pci:00:1f.6",
            "check beta pci:00:1f.6"
        ]
    );
    assert_eq!(rig.events(), ["+beta"]);
    let stored = rig.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("beta")));
    assert_eq!(stored.quarantine.len(), 1);
    let entry = &stored.quarantine[0];
    assert_eq!(
        (entry.device.as_str(), &entry.driver),
        (nic().identity().as_str(), &key("alpha"))
    );
    assert_eq!(
        entry.reason,
        FailureReason::Health(HealthFailure::NoResponse)
    );
    assert_eq!(rig.manager.quarantine(), stored.quarantine.as_slice());
}

#[test]
fn load_failures_are_quarantined_with_their_reason_and_not_unloaded() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.fail_load("alpha", LoadError::Failed(LoadFailure::InitFailed));
    rig.fail_load("beta", LoadError::Failed(LoadFailure::PackageRejected));
    let report = rig.add(nic());
    assert_eq!(report.active, Some(key("gamma")));
    assert_eq!(
        rig.log(),
        [
            "load alpha pci:00:1f.6",
            "load beta pci:00:1f.6",
            "load gamma pci:00:1f.6",
            "check gamma pci:00:1f.6"
        ]
    );
    let reasons: Vec<String> = rig
        .stored()
        .quarantine
        .iter()
        .map(|e| e.reason.code())
        .collect();
    assert_eq!(reasons, ["load.init-failed", "load.package-rejected"]);
}

#[test]
fn a_missing_package_is_skipped_without_quarantine() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.fail_load("alpha", LoadError::Failed(LoadFailure::PackageMissing));
    let report = rig.add(nic());
    assert_eq!(
        outcomes(&report),
        [
            ("alpha".to_string(), AttemptOutcome::Skipped),
            worked("beta")
        ]
    );
    assert!(rig.stored().quarantine.is_empty());
}

#[test]
fn all_candidates_fail() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    rig.verdict("alpha", NO_RESPONSE);
    rig.fail_load("beta", LoadError::Failed(LoadFailure::Timeout));
    rig.verdict(
        "gamma",
        HealthVerdict::Unhealthy(HealthFailure::SelfTestFailed),
    );
    let report = rig.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Failed]
    );
    assert_eq!(report.attempts.len(), 3);
    assert_eq!(report.active, None);
    assert_eq!(rig.manager.phase(NIC), Some(Phase::Failed));
    assert!(rig.events().is_empty());
    let stored = rig.stored();
    assert!(stored.choices.is_empty());
    let reasons: Vec<String> = stored.quarantine.iter().map(|e| e.reason.code()).collect();
    assert_eq!(
        reasons,
        [
            "health.no-response",
            "load.timeout",
            "health.self-test-failed"
        ]
    );
    // Every loaded driver was unloaded again.
    let log = rig.log();
    assert_eq!(log.iter().filter(|l| l.starts_with("unload")).count(), 2);
    assert!(!log.contains(&"unload beta pci:00:1f.6".to_string()));
}

#[test]
fn no_driver_in_the_depot() {
    let mut rig = boot(&three_drivers(), StoredState::default());
    let gpu = HardwareId::Pci {
        vendor: 0x10de,
        device: 0x1eb6,
        class: 0x030000,
        subsys: None,
    };
    let report = rig.add(DeviceDescriptor::new("pci:01:00.0", gpu));
    assert_eq!(report.transitions, [Phase::Detected, Phase::NoDriver]);
    assert!(report.attempts.is_empty());
    assert!(rig.log().is_empty());
    assert_eq!(rig.manager.store().saves(), 0);
    // Drivers for another kernel are no candidates either.
    let mut other = boot_with(
        &three_drivers(),
        MemoryStore::default(),
        Config::new("linux-6.8.0"),
    );
    assert_eq!(other.add(nic()).final_phase(), Some(Phase::NoDriver));
    assert_eq!(Config::new(ABI).kernel_abi, ABI);
}

#[test]
fn the_remembered_choice_is_loaded_directly_at_the_next_boot() {
    let index = three_drivers();
    let mut first = boot(&index, StoredState::default());
    first.verdict("alpha", NO_RESPONSE);
    first.add(nic());

    // Second boot: alpha is healthy again, but the remembered beta is used without any search.
    let mut second = boot(&index, first.stored());
    let report = second.add(nic());
    assert_eq!(
        report.transitions,
        [Phase::Detected, Phase::Probing, Phase::Active]
    );
    assert_eq!(outcomes(&report), [worked("beta")]);
    assert!(report.used_remembered && !report.remembered_failed);
    assert_eq!(
        second.log(),
        ["load beta pci:00:1f.6", "check beta pci:00:1f.6"]
    );
    // Nothing changed, so nothing is written (no needless flash writes).
    assert_eq!(second.manager.store().saves(), 0);

    // A remembered choice wins even over a better-ranked candidate that is not quarantined.
    let mut state = StoredState::default();
    state.choices.insert(nic().identity(), key("gamma"));
    let mut third = boot(&index, state);
    assert_eq!(third.add(nic()).active, Some(key("gamma")));
    assert_eq!(third.log().len(), 2);
}

#[test]
fn a_broken_remembered_choice_triggers_a_new_search_and_corrects_the_store() {
    let index = three_drivers();
    let mut first = boot(&index, StoredState::default());
    first.verdict("alpha", NO_RESPONSE);
    first.add(nic());
    assert_eq!(
        first.stored().choices.get(&nic().identity()),
        Some(&key("beta"))
    );

    let mut second = boot(&index, first.stored());
    second.verdict("beta", HealthVerdict::Unhealthy(HealthFailure::Unstable));
    let report = second.add(nic());
    // beta (remembered) fails, alpha stays quarantined, gamma is found.
    assert_eq!(
        outcomes(&report),
        [
            (
                "beta".to_string(),
                AttemptOutcome::Failed(FailureReason::Health(HealthFailure::Unstable))
            ),
            worked("gamma")
        ]
    );
    assert!(report.remembered_failed && !report.used_remembered);
    assert_eq!(report.final_phase(), Some(Phase::Active));
    let stored = second.stored();
    assert_eq!(stored.choices.get(&nic().identity()), Some(&key("gamma")));
    let quarantined: Vec<&str> = stored
        .quarantine
        .iter()
        .map(|e| e.driver.id.as_str())
        .collect();
    assert_eq!(quarantined, ["alpha", "beta"]);
    assert_eq!(second.manager.store().saves(), 1);

    // A remembered driver that left the depot is forgotten and searched anew.
    let mut state = StoredState::default();
    state.choices.insert(nic().identity(), key("retired"));
    let mut third = boot(&index, state);
    let report = third.add(nic());
    assert_eq!(report.active, Some(key("alpha")));
    assert!(report.remembered_failed);
    assert_eq!(
        third.stored().choices.get(&nic().identity()),
        Some(&key("alpha"))
    );
}

#[test]
fn remembered_choice_broken_and_nothing_else_works() {
    let index = three_drivers();
    let mut state = StoredState::default();
    state.choices.insert(nic().identity(), key("beta"));
    let mut rig = boot(&index, state);
    for driver in ["alpha", "beta", "gamma"] {
        rig.verdict(driver, NO_RESPONSE);
    }
    let report = rig.add(nic());
    assert_eq!(
        outcomes(&report),
        [
            failed_health("beta"),
            failed_health("alpha"),
            failed_health("gamma")
        ]
    );
    assert_eq!(report.final_phase(), Some(Phase::Failed));
    assert!(report.remembered_failed);
    let stored = rig.stored();
    assert!(
        stored.choices.is_empty(),
        "the broken choice must not be loaded again"
    );
    assert_eq!(stored.quarantine.len(), 3);
}

/// beta is remembered; alpha and gamma are quarantined.
fn beta_remembered_others_quarantined() -> StoredState {
    let mut state = StoredState::default();
    let identity = nic().identity();
    state.choices.insert(identity.clone(), key("beta"));
    let timeout = FailureReason::Health(HealthFailure::Timeout);
    state.quarantine_driver(&identity, key("alpha"), timeout);
    let init_failed = FailureReason::Load(LoadFailure::InitFailed);
    state.quarantine_driver(&identity, key("gamma"), init_failed);
    state
}

#[test]
fn last_resort_retries_quarantined_drivers_once() {
    let index = three_drivers();
    let state = beta_remembered_others_quarantined();
    let identity = nic().identity();

    // With the retry: beta fails, the quarantined alpha gets one more chance, works and is released.
    let mut rig = boot(&index, state.clone());
    rig.verdict("beta", NO_RESPONSE);
    let report = rig.add(nic());
    assert_eq!(outcomes(&report), [failed_health("beta"), worked("alpha")]);
    let stored = rig.stored();
    assert_eq!(stored.choices.get(&identity), Some(&key("alpha")));
    let quarantined: Vec<&str> = stored
        .quarantine
        .iter()
        .map(|e| e.driver.id.as_str())
        .collect();
    assert_eq!(quarantined, ["gamma", "beta"]);

    // A retried driver that fails again keeps its entry, with the fresh reason.
    let mut rig = boot(&index, state.clone());
    for driver in ["alpha", "beta", "gamma"] {
        rig.verdict(driver, NO_RESPONSE);
    }
    let report = rig.add(nic());
    assert_eq!(report.attempts.len(), 3);
    assert_eq!(report.final_phase(), Some(Phase::Failed));
    assert!(rig
        .stored()
        .quarantine
        .iter()
        .all(|e| e.reason == FailureReason::Health(HealthFailure::NoResponse)));
    assert_eq!(rig.stored().quarantine.len(), 3);
}

#[test]
fn without_last_resort_quarantined_drivers_are_never_touched() {
    let index = three_drivers();
    let state = beta_remembered_others_quarantined();
    let config = Config {
        last_resort_retry: false,
        ..Config::new(ABI)
    };
    let mut strict = boot_with(&index, MemoryStore::with_state(state), config);
    strict.verdict("beta", NO_RESPONSE);
    let report = strict.add(nic());
    assert_eq!(outcomes(&report), [failed_health("beta")]);
    assert_eq!(report.final_phase(), Some(Phase::Failed));
    assert_eq!(
        strict.log(),
        [
            "load beta pci:00:1f.6",
            "check beta pci:00:1f.6",
            "unload beta pci:00:1f.6"
        ]
    );
}
