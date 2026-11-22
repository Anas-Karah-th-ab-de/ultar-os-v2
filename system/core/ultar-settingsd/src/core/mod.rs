//! The core of the service: one state machine without threads, sockets or
//! real time. The bus server and the tests drive it.

mod displays;
mod list;
mod pending;
mod write;

use crate::apply::{Appliers, Change};
use crate::clock::Clock;
use crate::displays::{combination_id, DisplayDefaults, DisplayInfo};
use crate::error::SettingsError;
use crate::schema::{
    DefaultProvider, DefaultValue, InstanceKind, Registry, Resolved, Scope, SettingDef,
};
use crate::store::{Notice, Slot, Store, StoreConfig, StoreStats};
use crate::value::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::PendingOutcome;
use ultar_profile::Capabilities;

pub use list::{panel_visible, MAX_LISTED_APPS};

/// Most provisional changes that may wait for confirmation at once.
pub const MAX_PENDING: usize = 32;

/// Everything the core is built from.
pub struct CoreConfig {
    /// Where and how values are stored.
    pub store: StoreConfig,
    /// The declared settings.
    pub registry: Arc<Registry>,
    /// The clock for write delays and confirmation deadlines.
    pub clock: Arc<dyn Clock>,
    /// The components that put values into effect.
    pub appliers: Appliers,
    /// Device-computed defaults beyond the display ones.
    pub defaults: Box<dyn DefaultProvider>,
    /// What the device can do at start.
    pub capabilities: Capabilities,
}

/// Something the service has to announce on the bus.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A setting has a new value in effect.
    Changed {
        /// Concrete key.
        key: String,
        /// The value now in effect.
        value: Value,
    },
    /// A provisional value is in effect and needs confirmation.
    Pending {
        /// Concrete key.
        key: String,
        /// The provisional value.
        value: Value,
        /// The value that returns without confirmation.
        previous: Value,
        /// Seconds until the value is taken back.
        seconds: u32,
    },
    /// A pending change ended.
    Resolved {
        /// Concrete key.
        key: String,
        /// How it ended.
        outcome: PendingOutcome,
        /// The value in effect afterwards.
        value: Value,
    },
}

/// A provisional change waiting for confirmation.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    /// Value in effect before the first provisional change.
    previous: Value,
    /// The provisional value; `None` stands for "back to the default".
    target: Option<Value>,
    /// When the change is taken back.
    deadline: Duration,
}

/// Counters of the core.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CoreStats {
    /// Appliers that failed while a value was restored or a display
    /// combination was applied (failures of a plain `set` are returned to
    /// the caller instead).
    pub apply_failures: u64,
}

/// The settings service without its bus connection.
pub struct Core {
    registry: Arc<Registry>,
    store: Store,
    clock: Arc<dyn Clock>,
    appliers: Appliers,
    defaults: Box<dyn DefaultProvider>,
    capabilities: Capabilities,
    displays: Vec<DisplayInfo>,
    combo: String,
    pending: BTreeMap<String, Pending>,
    events: Vec<Event>,
    stats: CoreStats,
}

impl fmt::Debug for Core {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Core")
            .field("store", &self.store)
            .field("combo", &self.combo)
            .field("pending", &self.pending.len())
            .finish_non_exhaustive()
    }
}

fn slot_of(scope: Scope, combo: &str) -> Slot<'_> {
    match scope {
        Scope::System => Slot::System,
        Scope::User => Slot::User,
        Scope::DisplayCombination => Slot::Combo(combo),
    }
}

impl Core {
    /// Opens the store and takes back every change that was still waiting
    /// for confirmation when the service last stopped. Returns the core and
    /// what the store found while loading.
    pub fn open(config: CoreConfig) -> Result<(Core, Vec<Notice>), SettingsError> {
        let (store, notices) =
            Store::open(config.store, &config.registry, Arc::clone(&config.clock))?;
        // Until the display service reports, the combination used last is
        // assumed; only the identifiers of its displays are known.
        let displays: Vec<DisplayInfo> = store
            .latest_combo()
            .map(|(_, combo)| {
                combo
                    .displays
                    .iter()
                    .map(|id| DisplayInfo::bare(id))
                    .collect()
            })
            .unwrap_or_default();
        let ids: Vec<&str> = displays.iter().map(|display| display.id.as_str()).collect();
        let mut core = Core {
            registry: config.registry,
            combo: combination_id(&ids),
            store,
            clock: config.clock,
            appliers: config.appliers,
            defaults: config.defaults,
            capabilities: config.capabilities,
            displays,
            pending: BTreeMap::new(),
            events: Vec::new(),
            stats: CoreStats::default(),
        };
        core.revert_interrupted()?;
        Ok((core, notices))
    }

    fn default_value(&self, def: &SettingDef, instance: Option<&str>) -> Value {
        match &def.default {
            DefaultValue::Static(value) => value.clone(),
            DefaultValue::Computed { fallback } => DisplayDefaults(&self.displays)
                .computed_default(def, instance)
                .or_else(|| self.defaults.computed_default(def, instance))
                .and_then(|value| def.value_type.accept(value).ok())
                .unwrap_or_else(|| fallback.clone()),
        }
    }

    fn stored_or_default(&self, found: &Resolved<'_>) -> Value {
        self.store
            .get(slot_of(found.def.scope, &self.combo), &found.key)
            .cloned()
            .unwrap_or_else(|| self.default_value(found.def, found.instance.as_deref()))
    }

    fn effective(&self, found: &Resolved<'_>) -> Value {
        match self.pending.get(&found.key) {
            Some(pending) => pending
                .target
                .clone()
                .unwrap_or_else(|| self.default_value(found.def, found.instance.as_deref())),
            None => self.stored_or_default(found),
        }
    }

    fn check_instance(&self, found: &Resolved<'_>) -> Result<(), SettingsError> {
        let known = match (found.def.instance, &found.instance) {
            (InstanceKind::Display, Some(id)) => self.displays.iter().any(|d| d.id == *id),
            _ => true,
        };
        known.then_some(()).ok_or(SettingsError::UnknownInstance)
    }

    fn display_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.displays.iter().map(|d| d.id.clone()).collect();
        ids.sort_unstable();
        ids
    }

    fn apply(
        &mut self,
        found: &Resolved<'_>,
        value: &Value,
        provisional: bool,
    ) -> Result<(), SettingsError> {
        let change = Change {
            key: &found.key,
            template: found.def.key,
            instance: found.instance.as_deref(),
            value,
            provisional,
        };
        Ok(self.appliers.apply(found.def.domain, &change)?)
    }

    /// The value of a setting that is in effect right now: a provisional
    /// value, else the stored one, else the default.
    pub fn get(&self, key: &str) -> Result<Value, SettingsError> {
        let found = self.registry.resolve(key)?;
        self.check_instance(&found)?;
        Ok(self.effective(&found))
    }

    /// Replaces the device capabilities (reported by the device service).
    pub fn set_capabilities(&mut self, capabilities: Capabilities) {
        self.capabilities = capabilities;
    }

    /// The device capabilities, counting displays the display service
    /// reported even when the device service has not caught up yet.
    pub fn capabilities(&self) -> Capabilities {
        let mut capabilities = self.capabilities;
        let reported = u8::try_from(self.displays.len()).unwrap_or(u8::MAX);
        capabilities.display_count = capabilities.display_count.max(reported);
        capabilities
    }

    /// The connected displays as last reported.
    pub fn displays(&self) -> &[DisplayInfo] {
        &self.displays
    }

    /// Identifier of the display combination in use.
    pub fn combination(&self) -> &str {
        &self.combo
    }

    /// The declared settings.
    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    /// Hands out the events collected since the last call. Every operation
    /// adds a bounded number (at most one per key it touches).
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    /// Writes all waiting changes to disk now.
    pub fn flush(&mut self) -> Result<usize, SettingsError> {
        Ok(self.store.flush()?)
    }

    /// Counters of the store.
    pub fn store_stats(&self) -> StoreStats {
        self.store.stats()
    }

    /// Counters of the core.
    pub fn stats(&self) -> CoreStats {
        self.stats
    }
}

#[cfg(test)]
pub(crate) mod testkit;
#[cfg(test)]
mod tests;
