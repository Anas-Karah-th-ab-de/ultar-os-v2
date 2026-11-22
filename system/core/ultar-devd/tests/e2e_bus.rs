//! End to end (e): the daemon behind a real broker – a bus client lists devices, switches
//! drivers and gets `Unsupported` exactly where the kernel offers no way.

mod rig;

use rig::kernel_fmt::beelink;
use rig::{builtin_manifest, row, Rig};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::device_service::{
    ClearQuarantine, DevicePhase, List, Outcome, RefusalKind, Rescan, Rollback, Shutdown, Switch,
};
use ultar_bus::messages::devices::{Availability, Capabilities, DriverPhase, DriverState};
use ultar_bus::{
    Broker, BusError, Client, ClientOptions, ErrorCode, FixedIdentity, Identity, Policy,
};
use ultar_devd::daemon::Daemon;

/// Upper bound for anything that should happen at once (blocking waits, no polling).
const WAIT: Duration = Duration::from_secs(30);
const NVME0: &str = "pci:01:00.0";

/// Beelink on Ultar kernel texts; depot: repository drivers, planned drivers, a newer nvme
/// package for the same kernel driver, and an nvme package for a kernel driver that the
/// kernel does not have.
pub fn switch_rig(tag: &str) -> Rig {
    let rig = Rig::new(tag);
    let machine = beelink();
    machine.write_ultar_root(&rig.root, &machine.selftest_log());
    rig.add_repo_drivers();
    rig.add_planned_drivers();
    let rule = "pci_class = [\"010802\"]";
    rig.add_builtin(&builtin_manifest("nvme", "1.1.0", "storage", rule), "nvme");
    let turbo = builtin_manifest("nvme-turbo", "1.0.0", "storage", rule);
    rig.add_builtin(&turbo, "nvme-turbo");
    rig.write_index();
    rig
}

/// A broker, the daemon serving on a thread and a client playing the settings panel.
struct Session {
    _rig: Rig,
    _broker: Broker,
    panel: Client,
    states: ultar_bus::TypedSubscription<DriverState>,
    daemon: std::thread::JoinHandle<Result<(), BusError>>,
}

fn socket_of(rig: &Rig) -> PathBuf {
    rig.root.parent().unwrap().join("bus.sock")
}

fn start_broker(socket: &Path) -> Broker {
    let identity = Arc::new(FixedIdentity::new(Identity::new("test.peer").unwrap()));
    Broker::builder(Policy::allow_all())
        .listen(socket, identity)
        .start()
        .unwrap()
}

fn session(tag: &str) -> Session {
    let rig = switch_rig(tag);
    let socket = socket_of(&rig);
    let broker = start_broker(&socket);
    let panel = Client::connect(&socket, ClientOptions::named("panel")).unwrap();
    let states = panel.subscribe::<DriverState>().unwrap();
    let bus = Client::connect(&socket, ClientOptions::named("ultar-devd")).unwrap();
    let endpoint = Daemon::register(&bus).unwrap();
    let mut daemon = Daemon::start(rig.config(), Box::new(bus.clone())).unwrap();
    Session {
        _rig: rig,
        _broker: broker,
        panel,
        states,
        daemon: std::thread::spawn(move || daemon.serve(&endpoint)),
    }
}

impl Session {
    fn switch(&self, address: &str, driver: &str, version: &str) -> Outcome {
        let call = Switch {
            address: address.into(),
            driver: driver.into(),
            version: version.into(),
        };
        self.panel.call(&call, WAIT).unwrap().outcome
    }

    fn refused(&self, address: &str, driver: &str, version: &str) -> RefusalKind {
        match self.switch(address, driver, version) {
            Outcome::Refused(refusal) => refusal.kind,
            Outcome::Done(device) => panic!("unexpectedly done: {device:?}"),
        }
    }

    fn nvme_version(&self) -> String {
        let list = self.panel.call(&List {}, WAIT).unwrap();
        row(&list, NVME0).active.as_ref().unwrap().version.clone()
    }

    fn skip_startup_states(&self) {
        for _ in 0..14 {
            self.states.recv_timeout(WAIT).unwrap();
        }
    }

    fn shut_down(self) {
        self.panel.call(&Shutdown {}, WAIT).unwrap();
        assert!(self.daemon.join().unwrap().is_ok());
    }
}

#[test]
fn startup_publishes_states_and_a_retained_capability_set() {
    let s = session("bus-start");
    // Working devices first (in address order), then the devices without driver.
    let first = s.states.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (first.device.as_str(), first.phase),
        ("pci:00:0d.0", DriverPhase::Active)
    );
    let mut phases = vec![first.phase];
    for _ in 1..14 {
        phases.push(s.states.recv_timeout(WAIT).unwrap().phase);
    }
    let count = |phase| phases.iter().filter(|p| **p == phase).count();
    assert_eq!(count(DriverPhase::Active), 7);
    assert_eq!(count(DriverPhase::Failed), 3);
    assert_eq!(count(DriverPhase::Missing), 4);
    // A subscriber that comes late still gets the capability set (retained).
    let capabilities = s.panel.subscribe::<Capabilities>().unwrap();
    let retained = capabilities.recv_timeout(WAIT).unwrap();
    assert_eq!(retained.revision, 1);
    assert!(retained.has("storage") && retained.has("net") && !retained.has("wifi"));
    let gpu = retained.get("gpu").unwrap();
    assert_eq!(gpu.availability, Availability::Unavailable);
    let list = s.panel.call(&List {}, WAIT).unwrap();
    assert_eq!(list.devices.len(), 14);
    assert!(!list.kernel_switch_supported);
    let nvme = row(&list, NVME0);
    assert_eq!(nvme.active.as_ref().unwrap().version, "1.1.0");
    let alternatives: Vec<&str> = nvme.alternatives.iter().map(|a| a.id.as_str()).collect();
    assert_eq!(alternatives, ["nvme", "nvme-turbo"]);
    s.shut_down();
}

#[test]
fn switch_succeeds_within_the_kernel_driver_that_runs_the_device() {
    let s = session("bus-switch");
    s.skip_startup_states();
    let capabilities = s.panel.subscribe::<Capabilities>().unwrap();
    assert_eq!(capabilities.recv_timeout(WAIT).unwrap().revision, 1);
    match s.switch(NVME0, "nvme", "1.0.0") {
        Outcome::Done(device) => {
            assert_eq!(device.active.as_ref().unwrap().version, "1.0.0");
            assert_eq!(device.previous.as_deref(), Some("nvme 1.1.0"));
            assert_eq!(device.phase, DevicePhase::Active);
        }
        Outcome::Refused(refusal) => panic!("{refusal:?}"),
    }
    let removed = s.states.recv_timeout(WAIT).unwrap();
    let activated = s.states.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (removed.device.as_str(), removed.phase),
        (NVME0, DriverPhase::Removed)
    );
    assert_eq!(
        (activated.device.as_str(), activated.phase),
        (NVME0, DriverPhase::Active)
    );
    // Roll back to the earlier package; the second drive has nothing to roll back to.
    let call = Rollback {
        address: NVME0.into(),
    };
    let back = s.panel.call(&call, WAIT).unwrap();
    assert!(matches!(back.outcome, Outcome::Done(_)) && !back.restored);
    assert_eq!(s.nvme_version(), "1.1.0");
    let call = Rollback {
        address: "pci:02:00.0".into(),
    };
    let none = s.panel.call(&call, WAIT).unwrap();
    assert!(
        matches!(none.outcome, Outcome::Refused(r) if r.kind == RefusalKind::NothingToRollBack)
    );
    // "storage" stayed available throughout: no further capability message was published.
    assert!(capabilities.try_recv().unwrap().is_none());
    s.shut_down();
}

#[test]
fn switch_to_another_kernel_driver_is_unsupported_and_touches_nothing() {
    let s = session("bus-unsupported");
    s.skip_startup_states();
    // The Ultar kernel cannot deactivate `nvme` or activate anything else.
    let call = Switch {
        address: NVME0.into(),
        driver: "nvme-turbo".into(),
        version: "1.0.0".into(),
    };
    let reply = s.panel.call(&call, WAIT).unwrap();
    let Outcome::Refused(refusal) = reply.outcome else {
        panic!("a switch to another kernel driver must be refused");
    };
    assert_eq!(refusal.kind, RefusalKind::Unsupported);
    assert_eq!(refusal.reason.key, "devd.error.unsupported");
    assert!(!reply.restored);
    // Still the same driver, nothing quarantined, no state event.
    let list = s.panel.call(&List {}, WAIT).unwrap();
    let nvme = row(&list, NVME0);
    assert_eq!(nvme.active.as_ref().unwrap().version, "1.1.0");
    assert!(nvme.alternatives.iter().all(|a| a.quarantined.is_none()));
    assert!(s.states.try_recv().unwrap().is_none());
    // The same for a device the kernel left without driver (loadable module package).
    assert_eq!(
        s.refused("pci:ac:00.0", "intel-wifi", "1.0.0"),
        RefusalKind::Unsupported
    );
    s.shut_down();
}

#[test]
fn other_refusals_and_maintenance_calls_are_typed() {
    let s = session("bus-refusals");
    assert_eq!(
        s.refused(NVME0, "nvme", "1.1.0"),
        RefusalKind::AlreadyActive
    );
    assert_eq!(
        s.refused(NVME0, "gen-hda", "1.0.0"),
        RefusalKind::NotACandidate
    );
    assert_eq!(
        s.refused("pci:ff:00.0", "nvme", "1.0.0"),
        RefusalKind::UnknownDevice
    );
    assert_eq!(s.refused(NVME0, "nvme", "latest"), RefusalKind::BadRequest);
    let cleared = s.panel.call(&ClearQuarantine::default(), WAIT).unwrap();
    assert_eq!(cleared.released, Ok(0));
    let rescan = s.panel.call(&Rescan { address: None }, WAIT).unwrap();
    assert_eq!(rescan.devices.unwrap().len(), 14);
    assert!(rescan.added.is_empty() && rescan.removed.is_empty());
    // An unknown method is a typed bus error, not a hang.
    let unknown: Result<(), BusError> = s.panel.request("devices", "format_disk", &(), WAIT);
    assert!(
        matches!(unknown, Err(BusError::Remote(ErrorCode::UnknownMethod))),
        "{unknown:?}"
    );
    s.shut_down();
}
