//! The "try all, remember the working one" selection of backends per device class.

use std::sync::Arc;
use ultar_hal::mock::MockBackend;
use ultar_hal::{
    Backend, BackendSet, ChoiceOrigin, ChoiceStore, DeviceClass, HalError, HalResult,
    MemoryChoiceStore, SystemSummary,
};
use ultar_profile::{Capability, CapabilitySet, CpuTopology, SimdLevel};

fn boxed(backend: &Arc<MockBackend>) -> Box<dyn Backend> {
    Box::new(backend.clone())
}

/// A device specific backend (display, input, Wi-Fi) in front of a generic one.
fn device_and_generic() -> (Arc<MockBackend>, Arc<MockBackend>) {
    let device = MockBackend::with_classes(
        "exynos7885",
        &[DeviceClass::Display, DeviceClass::Input, DeviceClass::Wifi],
    );
    let generic = MockBackend::phone().named("generic-linux");
    (Arc::new(device), Arc::new(generic))
}

#[test]
fn first_start_takes_the_first_working_candidate_per_class() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::new();
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);

    assert_eq!(set.candidate_names(), vec!["exynos7885", "generic-linux"]);
    assert_eq!(set.chosen(DeviceClass::Display), Some("exynos7885"));
    assert_eq!(set.chosen(DeviceClass::Wifi), Some("exynos7885"));
    assert_eq!(set.chosen(DeviceClass::Audio), Some("generic-linux"));
    assert_eq!(set.chosen(DeviceClass::Tpm), None);
    let tpm = set.selection(DeviceClass::Tpm).unwrap();
    assert_eq!(
        (tpm.backend.as_deref(), tpm.origin),
        (None, ChoiceOrigin::Missing)
    );
    assert_eq!(
        set.selection(DeviceClass::Audio).unwrap().origin,
        ChoiceOrigin::Found
    );
    assert_eq!(set.selections().len(), DeviceClass::COUNT);

    // Everything found was remembered, the missing class was not.
    assert_eq!(
        store.recall(DeviceClass::Display).as_deref(),
        Some("exynos7885")
    );
    assert_eq!(
        store.recall(DeviceClass::Audio).as_deref(),
        Some("generic-linux")
    );
    assert_eq!(store.recall(DeviceClass::Tpm), None);
    assert_eq!(store.writes(), DeviceClass::COUNT - 1);
    // Each candidate was probed exactly once although it serves several classes.
    assert_eq!((device.probe_count(), generic.probe_count()), (1, 1));
    assert_eq!(set.store_error(), None);
    assert!(set.probe());
    assert_eq!(set.name(), "backend-set");
}

#[test]
fn set_forwards_each_class_to_its_backend() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::new();
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    // The display class comes from the device backend, which has no display plugged.
    assert!(set.display().unwrap().displays().unwrap().is_empty());
    device
        .ui
        .display
        .plug("DSI-1", true, Default::default(), &[]);
    assert_eq!(set.display().unwrap().displays().unwrap().len(), 1);
    // Audio comes from the generic backend with the phone preset.
    assert_eq!(set.audio().unwrap().outputs().unwrap().len(), 2);
    assert!(set.tpm().is_none());
    for class in DeviceClass::ALL {
        assert_eq!(set.provides(class), class != DeviceClass::Tpm, "{class}");
    }
}

#[test]
fn later_start_tries_only_the_remembered_backend() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::new();
    BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    let saved = store.pairs().into_iter().map(|(c, b)| (c, b.to_string()));
    let saved: Vec<(&str, String)> = saved.collect();

    // Second start with a fresh store loaded from what was saved. Put a new backend
    // first that would also work: it must not even be probed.
    let newcomer = Arc::new(MockBackend::phone().named("newcomer"));
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::from_pairs(saved.iter().map(|(c, b)| (*c, b.as_str())));
    let set = BackendSet::select(
        vec![boxed(&newcomer), boxed(&device), boxed(&generic)],
        &mut store,
    );
    assert_eq!(set.chosen(DeviceClass::Display), Some("exynos7885"));
    assert_eq!(set.chosen(DeviceClass::Audio), Some("generic-linux"));
    let remembered = set
        .selections()
        .iter()
        .filter(|selection| selection.origin == ChoiceOrigin::Remembered)
        .count();
    assert_eq!(remembered, DeviceClass::COUNT - 1);
    assert_eq!(store.writes(), 0);
    assert_eq!(newcomer.probe_count(), 0);
    assert_eq!((device.probe_count(), generic.probe_count()), (1, 1));
}

#[test]
fn remembered_backend_that_stopped_working_is_replaced() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::new();
    BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    let writes_before = store.writes();

    // Other kernel: the device backend no longer probes.
    device.set_probe_result(false);
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    for class in [DeviceClass::Display, DeviceClass::Input, DeviceClass::Wifi] {
        assert_eq!(set.chosen(class), Some("generic-linux"));
        assert_eq!(set.selection(class).unwrap().origin, ChoiceOrigin::Found);
        assert_eq!(store.recall(class).as_deref(), Some("generic-linux"));
    }
    assert_eq!(
        set.selection(DeviceClass::Audio).unwrap().origin,
        ChoiceOrigin::Remembered
    );
    assert_eq!(store.writes(), writes_before + 3);
}

#[test]
fn remembered_backend_that_lost_a_class_or_vanished_is_replaced() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::from_pairs([
        ("wifi", "exynos7885"),
        ("display", "removed-backend"),
        ("tpm", "generic-linux"),
    ]);
    device.set_class(DeviceClass::Wifi, false);
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    assert_eq!(set.chosen(DeviceClass::Wifi), Some("generic-linux"));
    assert_eq!(set.chosen(DeviceClass::Display), Some("exynos7885"));
    assert_eq!(
        set.selection(DeviceClass::Display).unwrap().origin,
        ChoiceOrigin::Found
    );
    // The remembered TPM backend has no TPM: nothing works, the memory is dropped.
    assert_eq!(set.chosen(DeviceClass::Tpm), None);
    assert_eq!(store.recall(DeviceClass::Tpm), None);
}

#[test]
fn nothing_probes() {
    let (device, generic) = device_and_generic();
    device.set_probe_result(false);
    generic.set_probe_result(false);
    let mut store = MemoryChoiceStore::from_pairs([("audio", "generic-linux")]);
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    assert!(!set.probe());
    assert!(set.audio().is_none() && set.display().is_none());
    assert!(DeviceClass::ALL.iter().all(|class| !set.provides(*class)));
    assert!(store.pairs().is_empty());
    assert_eq!((device.probe_count(), generic.probe_count()), (1, 1));
}

#[test]
fn reselect_after_hardware_change() {
    let (device, generic) = device_and_generic();
    let mut store = MemoryChoiceStore::new();
    let mut set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut store);
    assert_eq!(set.chosen(DeviceClass::Wifi), Some("exynos7885"));

    // Still fine: the remembered backend is probed again and kept.
    let kept = set.reselect(DeviceClass::Wifi, &mut store);
    assert_eq!(kept.origin, ChoiceOrigin::Remembered);
    assert_eq!(device.probe_count(), 2);

    // The Wi-Fi chip of the device backend disappears.
    device.set_class(DeviceClass::Wifi, false);
    let moved = set.reselect(DeviceClass::Wifi, &mut store);
    assert_eq!(moved.backend.as_deref(), Some("generic-linux"));
    assert_eq!(moved.origin, ChoiceOrigin::Found);
    assert_eq!(
        store.recall(DeviceClass::Wifi).as_deref(),
        Some("generic-linux")
    );
    // Other classes are untouched; the report still has one entry per class.
    assert_eq!(set.chosen(DeviceClass::Display), Some("exynos7885"));
    assert_eq!(set.selections().len(), DeviceClass::COUNT);

    generic.set_class(DeviceClass::Wifi, false);
    let gone = set.reselect(DeviceClass::Wifi, &mut store);
    assert_eq!((gone.backend, gone.origin), (None, ChoiceOrigin::Missing));
    assert!(set.wifi().is_none());
    assert_eq!(store.recall(DeviceClass::Wifi), None);
}

/// A store whose medium is read-only.
struct BrokenStore;

impl ChoiceStore for BrokenStore {
    fn recall(&self, _class: DeviceClass) -> Option<String> {
        None
    }
    fn remember(&mut self, _class: DeviceClass, _backend: &str) -> HalResult<()> {
        Err(HalError::Io(30))
    }
    fn forget(&mut self, _class: DeviceClass) -> HalResult<()> {
        Err(HalError::PermissionDenied)
    }
}

#[test]
fn store_failure_does_not_break_the_selection() {
    let (device, generic) = device_and_generic();
    let set = BackendSet::select(vec![boxed(&device), boxed(&generic)], &mut BrokenStore);
    assert_eq!(set.chosen(DeviceClass::Display), Some("exynos7885"));
    assert_eq!(set.chosen(DeviceClass::Audio), Some("generic-linux"));
    // The first failed write is reported.
    assert_eq!(set.store_error(), Some(HalError::Io(30)));
}

#[test]
fn system_summary_is_merged_from_chosen_backends() {
    let (device, generic) = device_and_generic();
    device.set_system(SystemSummary {
        cpu: CpuTopology::default(),
        memory_mb: 0,
        extra: CapabilitySet::of(&[Capability::Npu]),
    });
    let unused = Arc::new(MockBackend::empty("unused"));
    unused.set_system(SystemSummary {
        cpu: CpuTopology::default(),
        memory_mb: 1,
        extra: CapabilitySet::of(&[Capability::Tpm]),
    });
    let mut store = MemoryChoiceStore::new();
    let set = BackendSet::select(
        vec![boxed(&unused), boxed(&device), boxed(&generic)],
        &mut store,
    );
    let system = set.system();
    // Memory and CPU come from the first chosen backend that knows them.
    assert_eq!(system.memory_mb, 3_712);
    assert_eq!(system.cpu.simd, SimdLevel::Neon);
    assert_eq!(
        system.extra,
        CapabilitySet::of(&[Capability::Npu, Capability::Fingerprint])
    );
}
