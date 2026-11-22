//! The stage-A loader and health check against fake system roots.

mod rig;

use rig::kernel_fmt::{write, KDev, Machine};
use rig::{builtin_manifest, Rig};
use ultar_devd::builtin::{Builtin, LoadDetail};
use ultar_devd::{
    DriverKey, DriverLoader, HealthCheck, HealthFailure, HealthVerdict, LoadError, LoadFailure,
};
use ultar_driverpkg::depot::load_signed_index;
use ultar_driverpkg::{candidates, Candidate, DeviceDescriptor, TrustedKeys};
use ultar_hal_system::{BindState, ControlInterface, HardwareReader};

const ABI: &str = "ultar-kern-abi-1";

/// QEMU-like machine: NVMe, Bochs graphics, an unknown card and a failing AHCI controller.
fn machine() -> Machine {
    let mut machine = Machine {
        vendor: "QEMU".into(),
        model: "Standard PC (Q35 + ICH9, 2009)".into(),
        ..Machine::default()
    };
    machine
        .pci
        .push(KDev::new("00:01.0", 0x1b36, 0x0010, 0x01_0802));
    machine
        .pci
        .push(KDev::new("00:02.0", 0x1234, 0x1111, 0x03_0000));
    machine
        .pci
        .push(KDev::new("00:03.0", 0x10ec, 0x8168, 0x02_0000));
    machine
        .pci
        .push(KDev::new("00:1f.2", 0x8086, 0x2922, 0x01_0601));
    machine
        .probe_errors
        .insert("00:1f.2".into(), "ABAR (BAR5) fehlt".into());
    machine
}

struct Setup {
    rig: Rig,
    builtin: Builtin,
    devices: Vec<DeviceDescriptor>,
}

fn setup(tag: &str) -> Setup {
    let rig = Rig::new(tag);
    let machine = machine();
    machine.write_ultar_root(&rig.root, &machine.selftest_log());
    rig.add_repo_drivers();
    let display = builtin_manifest("bochs-display", "1.0.0", "display", "pci = [\"1234:1111\"]");
    rig.add_builtin(&display, "gen-bochs-vga");
    let realtek = builtin_manifest("realtek-eth", "1.0.0", "net", "pci = [\"10ec:8168\"]");
    rig.add_builtin(&realtek, "gen-realtek-eth");
    let module = builtin_manifest("module-eth", "1.0.0", "net", "pci = [\"10ec:8168\"]").replace(
        "hal_config = \"builtin/module-eth.toml\"",
        "module = \"drivers/module_eth.uko\"",
    );
    rig.add_with_files(
        &module,
        &[("drivers/module_eth.uko".into(), b"elf".to_vec())],
    );
    let broken = builtin_manifest("broken-record", "1.0.0", "net", "pci = [\"10ec:8168\"]");
    let record = b"kernel_driver = \"Not A Name\"\n".to_vec();
    rig.add_with_files(&broken, &[("builtin/broken-record.toml".into(), record)]);
    let unknown_key = builtin_manifest("extra-key", "1.0.0", "net", "pci = [\"10ec:8168\"]");
    let record = b"kernel_driver = \"x\"\nenable = true\n".to_vec();
    rig.add_with_files(&unknown_key, &[("builtin/extra-key.toml".into(), record)]);
    rig.write_index();
    let reader = HardwareReader::at(&rig.root);
    let devices = reader.inventory().unwrap().devices;
    let builtin = Builtin::new(reader, &rig.depot, rig.trusted()).unwrap();
    Setup {
        rig,
        builtin,
        devices,
    }
}

impl Setup {
    fn device(&self, address: &str) -> DeviceDescriptor {
        self.devices
            .iter()
            .find(|d| d.address == address)
            .unwrap()
            .clone()
    }

    fn candidate(&self, address: &str, id: &str) -> Candidate {
        let index = load_signed_index(&self.rig.depot, &self.rig.trusted()).unwrap();
        candidates(&self.device(address), &index, ABI)
            .into_iter()
            .find(|c| c.id == id)
            .unwrap_or_else(|| panic!("{id} is no candidate for {address}"))
    }

    fn load(&mut self, address: &str, id: &str) -> (Result<(), LoadError>, LoadDetail) {
        let (device, candidate) = (self.device(address), self.candidate(address, id));
        let result = self.builtin.load(&device, &candidate);
        let detail = self
            .builtin
            .detail(address, &DriverKey::of(&candidate))
            .unwrap();
        (result, detail)
    }

    fn check(&mut self, address: &str, id: &str) -> HealthVerdict {
        let (device, candidate) = (self.device(address), self.candidate(address, id));
        self.builtin.check(&device, &candidate)
    }
}

fn failed(failure: LoadFailure) -> Result<(), LoadError> {
    Err(LoadError::Failed(failure))
}

#[test]
fn load_confirms_the_kernel_binding() {
    let mut s = setup("load-ok");
    let (result, detail) = s.load("pci:00:01.0", "nvme");
    assert_eq!(result, Ok(()));
    assert_eq!(
        detail,
        LoadDetail::Loaded {
            kernel_driver: "nvme".into()
        }
    );
    match s.check("pci:00:01.0", "nvme") {
        HealthVerdict::Healthy(capabilities) => {
            assert_eq!(capabilities.len(), 1);
            assert_eq!(
                (
                    capabilities[0].name.as_str(),
                    capabilities[0].value.as_str()
                ),
                ("storage", "present")
            );
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(
        s.builtin.binding("pci:00:01.0"),
        BindState::Bound {
            driver: "nvme".into()
        }
    );
    assert_eq!(s.builtin.control_interface(), ControlInterface::None);
    // Unloading changes nothing: the kernel has no such interface.
    let (device, candidate) = (s.device("pci:00:01.0"), s.candidate("pci:00:01.0", "nvme"));
    s.builtin.unload(&device, &candidate);
    assert!(matches!(
        s.check("pci:00:01.0", "nvme"),
        HealthVerdict::Healthy(_)
    ));
    assert!(s.builtin.can_switch(&device, &candidate));
}

#[test]
fn every_load_failure_has_its_own_detail() {
    let mut s = setup("load-fail");
    let nic = "pci:00:03.0";
    // The kernel has no such driver bound to the device.
    let (result, detail) = s.load(nic, "realtek-eth");
    assert_eq!(result, failed(LoadFailure::Unsupported));
    assert_eq!(
        detail,
        LoadDetail::NotBound {
            kernel_driver: "gen-realtek-eth".into(),
            bound_to: None
        }
    );
    // A loadable module without activation record.
    let (result, detail) = s.load(nic, "module-eth");
    assert_eq!(
        (result, detail),
        (
            failed(LoadFailure::Unsupported),
            LoadDetail::NoActivationRecord
        )
    );
    // Invalid activation records.
    for id in ["broken-record", "extra-key"] {
        let (result, detail) = s.load(nic, id);
        assert_eq!(result, failed(LoadFailure::PackageRejected), "{id}");
        assert_eq!(detail, LoadDetail::BadActivationRecord, "{id}");
    }
}

#[test]
fn kernel_probe_failures_and_unusable_packages() {
    let mut s = setup("load-fail-2");
    let nic = "pci:00:03.0";
    // The kernel's own probe failed: a real driver fault.
    let (result, detail) = s.load("pci:00:1f.2", "ahci");
    assert_eq!(result, failed(LoadFailure::InitFailed));
    assert_eq!(detail, LoadDetail::ProbeFailed("ABAR (BAR5) fehlt".into()));
    // A health check without a confirmed load never reports healthy.
    assert_eq!(
        s.check("pci:00:1f.2", "ahci"),
        HealthVerdict::Unhealthy(HealthFailure::NoResponse)
    );
    let (device, candidate) = (s.device(nic), s.candidate(nic, "realtek-eth"));
    assert!(!s.builtin.can_switch(&device, &candidate));
    // Package file gone after the index was read.
    let nvme = s.candidate("pci:00:01.0", "nvme");
    std::fs::remove_file(s.rig.depot.join(&nvme.file)).unwrap();
    let (result, detail) = s.load("pci:00:01.0", "nvme");
    assert_eq!(
        (result, detail),
        (
            failed(LoadFailure::PackageMissing),
            LoadDetail::PackageMissing
        )
    );
    // A candidate whose file name tries to leave the depot is refused before any read.
    let mut escaping = nvme.clone();
    escaping.file = "../keys/test.keys".into();
    let device = s.device("pci:00:01.0");
    assert_eq!(
        s.builtin.load(&device, &escaping),
        failed(LoadFailure::PackageRejected)
    );
    // Without trusted keys nothing verifies.
    s.builtin.set_trusted(TrustedKeys::new());
    let (result, detail) = s.load(nic, "realtek-eth");
    assert_eq!(result, failed(LoadFailure::PackageRejected));
    assert!(
        matches!(detail, LoadDetail::PackageRejected(code) if code.starts_with("driverpkg.signature."))
    );
    // Details of a vanished device are dropped.
    let key = DriverKey::of(&candidate);
    assert!(s.builtin.detail(nic, &key).is_some());
    s.builtin.forget(nic);
    assert_eq!(s.builtin.detail(nic, &key), None);
}

#[test]
fn display_health_uses_the_kernel_device_description() {
    let mut s = setup("display");
    let card = "pci:00:02.0";
    assert_eq!(s.load(card, "bochs-display").0, Ok(()));
    match s.check(card, "bochs-display") {
        HealthVerdict::Healthy(capabilities) => assert_eq!(capabilities[0].name, "gpu"),
        other => panic!("{other:?}"),
    }
    // The kernel now says the card only shows the firmware picture.
    write(
        &s.rig.root,
        "proc/ultar/geraet",
        "art=pc\nanzeige=gop 1024x768\ngrafik0=1234:1111 Firmware-Bildspeicher\n",
    );
    s.builtin.refresh().unwrap();
    assert_eq!(
        s.check(card, "bochs-display"),
        HealthVerdict::Unhealthy(HealthFailure::SelfTestFailed)
    );
    // No description of the card at all: the binding alone decides.
    write(&s.rig.root, "proc/ultar/geraet", "art=pc\n");
    s.builtin.refresh().unwrap();
    assert!(matches!(
        s.check(card, "bochs-display"),
        HealthVerdict::Healthy(_)
    ));
}

#[test]
fn vanished_devices_and_changed_bindings() {
    let mut s = setup("vanish");
    assert_eq!(s.load("pci:00:01.0", "nvme").0, Ok(()));
    // After a refresh the kernel no longer lists the device.
    let mut smaller = machine();
    smaller.pci.remove(0);
    smaller.write_ultar_root(&s.rig.root, &smaller.selftest_log());
    s.builtin.refresh().unwrap();
    assert_eq!(s.check("pci:00:01.0", "nvme"), HealthVerdict::DeviceGone);
    let (result, detail) = s.load("pci:00:01.0", "nvme");
    assert_eq!(
        (result, detail),
        (Err(LoadError::DeviceGone), LoadDetail::DeviceGone)
    );
    // An unreadable system description is an error of the refresh, the old view stays.
    std::fs::remove_file(s.rig.root.join("proc/ultar/protokoll")).unwrap();
    std::fs::create_dir_all(s.rig.root.join("proc/ultar/protokoll")).unwrap();
    assert!(s.builtin.refresh().is_err());
    assert_eq!(
        s.builtin.binding("pci:00:02.0"),
        BindState::Bound {
            driver: "gen-bochs-vga".into()
        }
    );
}
