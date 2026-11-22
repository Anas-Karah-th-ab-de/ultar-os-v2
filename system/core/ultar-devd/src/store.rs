//! The persisted decisions and their text form; an in-memory [`ChoiceStore`].
//!
//! The daemon binary writes the text returned by [`StoredState::to_toml`] to its state
//! file (atomically) and feeds it back through [`StoredState::from_toml`]; this crate
//! itself knows no paths.

use crate::traits::{ChoiceStore, StoreError};
use crate::types::{DriverKey, FailureReason, QuarantineEntry};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use ultar_driverpkg::Version;

/// Format version of the stored text.
pub const STORE_FORMAT: u32 = 1;

/// Everything the daemon remembers across boots. Devices are keyed by
/// [`ultar_driverpkg::DeviceDescriptor::identity`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StoredState {
    /// The driver that works for each device (loaded directly at the next boot).
    pub choices: BTreeMap<String, DriverKey>,
    /// The driver that was active before the last manual switch (target of a roll-back).
    pub previous: BTreeMap<String, DriverKey>,
    /// Drivers that failed, per device, with the reason.
    pub quarantine: Vec<QuarantineEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawState {
    format: u32,
    #[serde(default)]
    choice: Vec<RawPair>,
    #[serde(default)]
    previous: Vec<RawPair>,
    #[serde(default)]
    quarantine: Vec<RawPair>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPair {
    device: String,
    driver: String,
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}

impl RawPair {
    fn new(device: &str, driver: &DriverKey, reason: Option<FailureReason>) -> Self {
        Self {
            device: device.to_string(),
            driver: driver.id.clone(),
            version: driver.version.to_string(),
            reason: reason.map(FailureReason::code),
        }
    }

    fn key(&self) -> Result<DriverKey, StoreError> {
        let version = Version::parse(&self.version).map_err(|_| StoreError::Corrupt)?;
        if self.device.is_empty() || self.driver.is_empty() {
            return Err(StoreError::Corrupt);
        }
        Ok(DriverKey::new(self.driver.clone(), version))
    }
}

fn pairs(map: &BTreeMap<String, DriverKey>) -> Vec<RawPair> {
    map.iter()
        .map(|(device, driver)| RawPair::new(device, driver, None))
        .collect()
}

fn map(pairs: &[RawPair]) -> Result<BTreeMap<String, DriverKey>, StoreError> {
    pairs
        .iter()
        .map(|pair| Ok((pair.device.clone(), pair.key()?)))
        .collect()
}

impl StoredState {
    /// Renders the state as TOML text (deterministic).
    pub fn to_toml(&self) -> Result<String, StoreError> {
        let raw = RawState {
            format: STORE_FORMAT,
            choice: pairs(&self.choices),
            previous: pairs(&self.previous),
            quarantine: self
                .quarantine
                .iter()
                .map(|entry| RawPair::new(&entry.device, &entry.driver, Some(entry.reason)))
                .collect(),
        };
        toml::to_string(&raw).map_err(|_| StoreError::WriteFailed)
    }

    /// Parses text written by [`StoredState::to_toml`]. Empty text is the empty state.
    pub fn from_toml(text: &str) -> Result<Self, StoreError> {
        if text.trim().is_empty() {
            return Ok(Self::default());
        }
        let raw: RawState = toml::from_str(text).map_err(|_| StoreError::Corrupt)?;
        if raw.format != STORE_FORMAT {
            return Err(StoreError::Corrupt);
        }
        let mut quarantine = Vec::with_capacity(raw.quarantine.len());
        for pair in &raw.quarantine {
            let reason = pair
                .reason
                .as_deref()
                .and_then(FailureReason::parse)
                .ok_or(StoreError::Corrupt)?;
            quarantine.push(QuarantineEntry {
                device: pair.device.clone(),
                driver: pair.key()?,
                reason,
            });
        }
        Ok(Self {
            choices: map(&raw.choice)?,
            previous: map(&raw.previous)?,
            quarantine,
        })
    }

    /// The quarantine entry for a driver on a device, if any.
    pub fn quarantined(&self, device: &str, driver: &DriverKey) -> Option<&QuarantineEntry> {
        self.quarantine
            .iter()
            .find(|entry| entry.device == device && &entry.driver == driver)
    }

    /// Adds or replaces the quarantine entry for a driver on a device.
    pub fn quarantine_driver(&mut self, device: &str, driver: DriverKey, reason: FailureReason) {
        self.release(device, &driver);
        self.quarantine.push(QuarantineEntry {
            device: device.to_string(),
            driver,
            reason,
        });
    }

    /// Removes the quarantine entry for a driver on a device; `true` when one existed.
    pub fn release(&mut self, device: &str, driver: &DriverKey) -> bool {
        let before = self.quarantine.len();
        self.quarantine
            .retain(|entry| !(entry.device == device && &entry.driver == driver));
        self.quarantine.len() != before
    }
}

/// A [`ChoiceStore`] that keeps the state in memory (tests, live systems without a data partition).
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    state: StoredState,
    saves: u32,
}

impl MemoryStore {
    /// Creates a store that already holds `state`.
    pub fn with_state(state: StoredState) -> Self {
        Self { state, saves: 0 }
    }

    /// The state as last saved.
    pub fn state(&self) -> &StoredState {
        &self.state
    }

    /// How often the state was saved.
    pub fn saves(&self) -> u32 {
        self.saves
    }
}

impl ChoiceStore for MemoryStore {
    fn load(&mut self) -> Result<StoredState, StoreError> {
        Ok(self.state.clone())
    }

    fn save(&mut self, state: &StoredState) -> Result<(), StoreError> {
        self.state = state.clone();
        self.saves += 1;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{HealthFailure, LoadFailure};

    fn key(id: &str) -> DriverKey {
        DriverKey::new(id, Version::new(1, 0, 0))
    }

    fn sample() -> StoredState {
        let mut state = StoredState::default();
        state
            .choices
            .insert("pci:00:1f.6=pci:8086:0d4c".into(), key("gen-intel-eth"));
        state
            .choices
            .insert("usb:1-14=usb:8087:0026".into(), key("intel-bt-usb"));
        state
            .previous
            .insert("pci:00:1f.6=pci:8086:0d4c".into(), key("old-eth"));
        state.quarantine_driver(
            "pci:00:1f.6=pci:8086:0d4c",
            key("bad-eth"),
            FailureReason::Health(HealthFailure::NoResponse),
        );
        state.quarantine_driver(
            "usb:1-14=usb:8087:0026",
            key("bad-bt"),
            FailureReason::Load(LoadFailure::InitFailed),
        );
        state
    }

    #[test]
    fn text_round_trip() {
        let state = sample();
        let text = state.to_toml().unwrap();
        assert!(text.contains("reason = \"health.no-response\""));
        assert_eq!(StoredState::from_toml(&text).unwrap(), state);
        assert_eq!(
            StoredState::from_toml(&text).unwrap().to_toml().unwrap(),
            text
        );
        assert_eq!(StoredState::from_toml("").unwrap(), StoredState::default());
        assert_eq!(
            StoredState::from_toml(" \n").unwrap(),
            StoredState::default()
        );
        let empty = StoredState::default().to_toml().unwrap();
        assert_eq!(
            StoredState::from_toml(&empty).unwrap(),
            StoredState::default()
        );
    }

    #[test]
    fn corrupt_text_is_rejected() {
        let text = sample().to_toml().unwrap();
        let cases = [
            "format = ".to_string(),
            "format = 2".to_string(),
            "choices = 1".to_string(),
            text.replace("version = \"1.0.0\"", "version = \"one\""),
            text.replace("health.no-response", "health.bogus"),
            text.replace("reason = \"load.init-failed\"\n", ""),
            text.replace("driver = \"gen-intel-eth\"", "driver = \"\""),
            text.replace("device = \"usb:1-14=usb:8087:0026\"", "device = \"\""),
            format!("{text}\nextra = 1"),
        ];
        for case in cases {
            assert_eq!(
                StoredState::from_toml(&case),
                Err(StoreError::Corrupt),
                "{case}"
            );
        }
    }

    #[test]
    fn quarantine_helpers() {
        let mut state = sample();
        let device = "pci:00:1f.6=pci:8086:0d4c";
        assert!(state.quarantined(device, &key("bad-eth")).is_some());
        assert!(state.quarantined(device, &key("bad-bt")).is_none());
        assert!(state.quarantined("other", &key("bad-eth")).is_none());
        // Re-quarantining replaces the reason instead of duplicating the entry.
        state.quarantine_driver(
            device,
            key("bad-eth"),
            FailureReason::Health(HealthFailure::Timeout),
        );
        assert_eq!(state.quarantine.len(), 2);
        let entry = state.quarantined(device, &key("bad-eth")).unwrap();
        assert_eq!(entry.reason, FailureReason::Health(HealthFailure::Timeout));
        assert!(state.release(device, &key("bad-eth")));
        assert!(!state.release(device, &key("bad-eth")));
        assert_eq!(state.quarantine.len(), 1);
    }

    #[test]
    fn memory_store() {
        let mut store = MemoryStore::with_state(sample());
        assert_eq!(store.load().unwrap(), sample());
        assert_eq!(store.saves(), 0);
        store.save(&StoredState::default()).unwrap();
        assert_eq!(store.saves(), 1);
        assert_eq!(store.state(), &StoredState::default());
        assert_eq!(
            MemoryStore::default().load().unwrap(),
            StoredState::default()
        );
    }
}
