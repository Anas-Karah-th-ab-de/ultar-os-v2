//! Choosing a backend per device class: try all candidates in order, remember the one
//! that works.
//!
//! This generalises the proven device-switch principle of the old init: on the first
//! start every candidate is tried in order and the first that works is remembered; on
//! later starts only the remembered one is tried. If it stopped working (other kernel,
//! other hardware) the search runs again for that one class and the memory is corrected.

use crate::audio::Audio;
use crate::backend::{Backend, DeviceClass, SystemSummary};
use crate::camera::Camera;
use crate::connectivity::{Bluetooth, Network, Wifi};
use crate::display::{Display, Renderer};
use crate::error::{HalError, HalResult};
use crate::input::Input;
use crate::misc::{Haptics, Leds, Storage, Tpm};
use crate::power::Power;
use crate::radio::Radio;
use crate::sensors::{Gnss, Sensors};
use std::collections::BTreeMap;

/// Persistent memory of which backend worked for which class.
///
/// The real store belongs to the device daemon and is bound to the device id of the
/// profile; a store for another device must simply recall nothing.
pub trait ChoiceStore {
    /// Name of the backend remembered for a class.
    fn recall(&self, class: DeviceClass) -> Option<String>;
    /// Remembers the backend that works for a class.
    fn remember(&mut self, class: DeviceClass, backend: &str) -> HalResult<()>;
    /// Drops the memory for a class (nothing works any more).
    fn forget(&mut self, class: DeviceClass) -> HalResult<()>;
}

/// [`ChoiceStore`] that lives in memory; also usable as a cache in front of a file.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryChoiceStore {
    choices: BTreeMap<DeviceClass, String>,
    writes: usize,
}

impl MemoryChoiceStore {
    /// Empty store (first start).
    pub fn new() -> Self {
        MemoryChoiceStore::default()
    }

    /// Store filled from `(class name, backend name)` pairs; unknown class names are skipped.
    pub fn from_pairs<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let choices = pairs
            .into_iter()
            .filter_map(|(class, backend)| Some((DeviceClass::parse(class)?, backend.to_string())))
            .collect();
        MemoryChoiceStore { choices, writes: 0 }
    }

    /// Every remembered choice as `(class name, backend name)`, in class order.
    pub fn pairs(&self) -> Vec<(&'static str, &str)> {
        self.choices
            .iter()
            .map(|(class, backend)| (class.as_str(), backend.as_str()))
            .collect()
    }

    /// Number of changes since creation; zero means nothing needs to be saved.
    pub fn writes(&self) -> usize {
        self.writes
    }
}

impl ChoiceStore for MemoryChoiceStore {
    fn recall(&self, class: DeviceClass) -> Option<String> {
        self.choices.get(&class).cloned()
    }

    fn remember(&mut self, class: DeviceClass, backend: &str) -> HalResult<()> {
        if self.choices.get(&class).map(String::as_str) != Some(backend) {
            self.choices.insert(class, backend.to_string());
            self.writes += 1;
        }
        Ok(())
    }

    fn forget(&mut self, class: DeviceClass) -> HalResult<()> {
        if self.choices.remove(&class).is_some() {
            self.writes += 1;
        }
        Ok(())
    }
}

/// How a class got its backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChoiceOrigin {
    /// The remembered backend still works.
    Remembered,
    /// Found by trying the candidates in order.
    Found,
    /// No candidate works for this class.
    Missing,
}

/// Result of the choice for one class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    /// The class.
    pub class: DeviceClass,
    /// Name of the chosen backend, if any.
    pub backend: Option<String>,
    /// How it was chosen.
    pub origin: ChoiceOrigin,
}

/// Ordered candidates plus the backend chosen for every class.
///
/// A `BackendSet` is itself a [`Backend`]: each class method forwards to the backend
/// chosen for that class, so services can treat the whole device as one backend.
pub struct BackendSet {
    candidates: Vec<Box<dyn Backend>>,
    chosen: [Option<usize>; DeviceClass::COUNT],
    selections: Vec<Selection>,
    store_error: Option<HalError>,
}

/// Probe results of one selection run; each candidate is probed at most once.
struct ProbeCache<'a> {
    candidates: &'a [Box<dyn Backend>],
    results: Vec<Option<bool>>,
}

impl ProbeCache<'_> {
    fn works(&mut self, index: usize, class: DeviceClass) -> bool {
        let Some(candidate) = self.candidates.get(index) else {
            return false;
        };
        if !candidate.provides(class) {
            return false;
        }
        match self.results.get(index).copied().flatten() {
            Some(result) => result,
            None => {
                let result = candidate.probe();
                if let Some(slot) = self.results.get_mut(index) {
                    *slot = Some(result);
                }
                result
            }
        }
    }

    fn choose(
        &mut self,
        class: DeviceClass,
        remembered: Option<&str>,
    ) -> (Option<usize>, ChoiceOrigin) {
        let remembered_index = remembered.and_then(|name| {
            self.candidates
                .iter()
                .position(|candidate| candidate.name() == name)
        });
        if let Some(index) = remembered_index {
            if self.works(index, class) {
                return (Some(index), ChoiceOrigin::Remembered);
            }
        }
        for index in 0..self.candidates.len() {
            if Some(index) != remembered_index && self.works(index, class) {
                return (Some(index), ChoiceOrigin::Found);
            }
        }
        (None, ChoiceOrigin::Missing)
    }
}

impl BackendSet {
    /// Chooses a backend for every class.
    ///
    /// Per class: the remembered backend is tried first and alone; only if it is
    /// unknown, no longer provides the class or fails its probe, the candidates are
    /// tried in the given order. The result is written back to the store. Each
    /// candidate is probed at most once, and candidates that are never needed are
    /// not probed at all.
    pub fn select(candidates: Vec<Box<dyn Backend>>, store: &mut dyn ChoiceStore) -> Self {
        let mut set = BackendSet {
            candidates,
            chosen: [None; DeviceClass::COUNT],
            selections: Vec::with_capacity(DeviceClass::COUNT),
            store_error: None,
        };
        let mut cache = ProbeCache {
            candidates: &set.candidates,
            results: vec![None; set.candidates.len()],
        };
        let mut outcomes = Vec::with_capacity(DeviceClass::COUNT);
        for class in DeviceClass::ALL {
            let remembered = store.recall(class);
            outcomes.push((class, cache.choose(class, remembered.as_deref())));
        }
        for (class, (index, origin)) in outcomes {
            set.apply(class, index, origin, store);
        }
        set
    }

    /// Chooses again for one class, probing afresh. Call this when the chosen backend
    /// reports [`HalError::Disconnected`] or hardware was hot-plugged.
    pub fn reselect(&mut self, class: DeviceClass, store: &mut dyn ChoiceStore) -> Selection {
        let remembered = store.recall(class);
        let mut cache = ProbeCache {
            candidates: &self.candidates,
            results: vec![None; self.candidates.len()],
        };
        let (index, origin) = cache.choose(class, remembered.as_deref());
        self.selections.retain(|selection| selection.class != class);
        self.apply(class, index, origin, store)
    }

    fn apply(
        &mut self,
        class: DeviceClass,
        index: Option<usize>,
        origin: ChoiceOrigin,
        store: &mut dyn ChoiceStore,
    ) -> Selection {
        self.chosen[class.index()] = index;
        let backend = index
            .and_then(|index| self.candidates.get(index))
            .map(|candidate| candidate.name().to_string());
        let stored = match (&backend, origin) {
            (Some(name), ChoiceOrigin::Found) => store.remember(class, name),
            (None, _) => store.forget(class),
            _ => Ok(()),
        };
        if let Err(error) = stored {
            self.store_error.get_or_insert(error);
        }
        let selection = Selection {
            class,
            backend,
            origin,
        };
        self.selections.push(selection.clone());
        selection
    }

    fn backend_for(&self, class: DeviceClass) -> Option<&dyn Backend> {
        let index = self.chosen[class.index()]?;
        self.candidates
            .get(index)
            .map(|candidate| candidate.as_ref())
    }

    /// Name of the backend chosen for a class.
    pub fn chosen(&self, class: DeviceClass) -> Option<&str> {
        self.backend_for(class).map(Backend::name)
    }

    /// The choice for one class.
    pub fn selection(&self, class: DeviceClass) -> Option<&Selection> {
        self.selections
            .iter()
            .find(|selection| selection.class == class)
    }

    /// The choices for every class (for the "drivers and hardware" panel and logs).
    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// Names of the candidates in their order.
    pub fn candidate_names(&self) -> Vec<&str> {
        self.candidates
            .iter()
            .map(|candidate| candidate.name())
            .collect()
    }

    /// First error the store reported while saving choices. The selection itself is
    /// valid even if it could not be remembered.
    pub fn store_error(&self) -> Option<HalError> {
        self.store_error
    }
}

impl Backend for BackendSet {
    fn name(&self) -> &str {
        "backend-set"
    }

    /// True if at least one class found a backend.
    fn probe(&self) -> bool {
        self.chosen.iter().any(Option::is_some)
    }

    /// CPU and memory from the first chosen backend that reports them; the extra
    /// capabilities of all chosen backends combined.
    fn system(&self) -> SystemSummary {
        let mut summary = SystemSummary::default();
        let mut seen: Vec<usize> = Vec::new();
        for index in self.chosen.iter().flatten().copied() {
            if seen.contains(&index) {
                continue;
            }
            seen.push(index);
            let Some(candidate) = self.candidates.get(index) else {
                continue;
            };
            let part = candidate.system();
            summary.extra = summary.extra.union(part.extra);
            if summary.memory_mb == 0 {
                summary.memory_mb = part.memory_mb;
            }
            if summary.cpu.total_cores() == 0 {
                summary.cpu = part.cpu;
            }
        }
        summary
    }

    fn display(&self) -> Option<&dyn Display> {
        self.backend_for(DeviceClass::Display)?.display()
    }
    fn renderer(&self) -> Option<&dyn Renderer> {
        self.backend_for(DeviceClass::Renderer)?.renderer()
    }
    fn input(&self) -> Option<&dyn Input> {
        self.backend_for(DeviceClass::Input)?.input()
    }
    fn audio(&self) -> Option<&dyn Audio> {
        self.backend_for(DeviceClass::Audio)?.audio()
    }
    fn power(&self) -> Option<&dyn Power> {
        self.backend_for(DeviceClass::Power)?.power()
    }
    fn network(&self) -> Option<&dyn Network> {
        self.backend_for(DeviceClass::Network)?.network()
    }
    fn wifi(&self) -> Option<&dyn Wifi> {
        self.backend_for(DeviceClass::Wifi)?.wifi()
    }
    fn bluetooth(&self) -> Option<&dyn Bluetooth> {
        self.backend_for(DeviceClass::Bluetooth)?.bluetooth()
    }
    fn radio(&self) -> Option<&dyn Radio> {
        self.backend_for(DeviceClass::Radio)?.radio()
    }
    fn camera(&self) -> Option<&dyn Camera> {
        self.backend_for(DeviceClass::Camera)?.camera()
    }
    fn sensors(&self) -> Option<&dyn Sensors> {
        self.backend_for(DeviceClass::Sensors)?.sensors()
    }
    fn gnss(&self) -> Option<&dyn Gnss> {
        self.backend_for(DeviceClass::Gnss)?.gnss()
    }
    fn haptics(&self) -> Option<&dyn Haptics> {
        self.backend_for(DeviceClass::Haptics)?.haptics()
    }
    fn leds(&self) -> Option<&dyn Leds> {
        self.backend_for(DeviceClass::Leds)?.leds()
    }
    fn storage(&self) -> Option<&dyn Storage> {
        self.backend_for(DeviceClass::Storage)?.storage()
    }
    fn tpm(&self) -> Option<&dyn Tpm> {
        self.backend_for(DeviceClass::Tpm)?.tpm()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_counts_real_changes() {
        let mut store = MemoryChoiceStore::new();
        assert_eq!(store.recall(DeviceClass::Wifi), None);
        store.remember(DeviceClass::Wifi, "a").unwrap();
        store.remember(DeviceClass::Wifi, "a").unwrap();
        assert_eq!(store.writes(), 1);
        store.remember(DeviceClass::Wifi, "b").unwrap();
        assert_eq!(store.recall(DeviceClass::Wifi).as_deref(), Some("b"));
        store.forget(DeviceClass::Wifi).unwrap();
        store.forget(DeviceClass::Wifi).unwrap();
        assert_eq!(store.writes(), 3);
        assert!(store.pairs().is_empty());
    }

    #[test]
    fn memory_store_pairs_round_trip() {
        let store = MemoryChoiceStore::from_pairs([
            ("wifi", "exynos7885"),
            ("display", "generic-linux"),
            ("toaster", "x"),
        ]);
        assert_eq!(
            store.pairs(),
            vec![("display", "generic-linux"), ("wifi", "exynos7885")]
        );
        assert_eq!(store.writes(), 0);
        let copy = MemoryChoiceStore::from_pairs(store.pairs());
        assert_eq!(copy, store);
    }

    #[test]
    fn empty_candidate_list() {
        let mut store = MemoryChoiceStore::from_pairs([("wifi", "gone")]);
        let set = BackendSet::select(Vec::new(), &mut store);
        assert!(!set.probe());
        assert!(set.candidate_names().is_empty());
        assert_eq!(set.selections().len(), DeviceClass::COUNT);
        assert!(set
            .selections()
            .iter()
            .all(|selection| selection.origin == ChoiceOrigin::Missing));
        // A remembered backend that no longer exists is forgotten.
        assert_eq!(store.recall(DeviceClass::Wifi), None);
        assert!(set.wifi().is_none() && set.system().is_empty());
        assert_eq!(set.store_error(), None);
    }
}
