//! Display combinations: switching to the arrangement remembered for the
//! set of connected displays.

use super::{Core, Event};
use crate::access::Caller;
use crate::displays::{combination_id, identify};
use crate::error::SettingsError;
use crate::schema::{InstanceKind, Resolved, Scope};
use crate::value::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use ultar_bus::messages::settings::{DisplayDescriptor, PendingOutcome, SetDisplaysReply};

impl Core {
    /// Every concrete key of the display-combination scope for the
    /// connected displays, in declaration order.
    fn combination_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        for def in self.registry.defs() {
            if def.scope != Scope::DisplayCombination {
                continue;
            }
            let Some(template) = crate::schema::KeyTemplate::parse(def.key) else {
                continue;
            };
            match def.instance {
                InstanceKind::Display => keys.extend(
                    self.display_ids()
                        .iter()
                        .map(|id| template.instantiate(Some(id))),
                ),
                _ => keys.push(def.key.to_owned()),
            }
        }
        keys
    }

    fn combination_values(&self) -> BTreeMap<String, Value> {
        self.combination_keys()
            .into_iter()
            .filter_map(|key| {
                let found = self.registry.resolve(&key).ok()?;
                Some((key, self.effective(&found)))
            })
            .collect()
    }

    /// Drops provisional display changes without applying anything: they
    /// belonged to the old combination, and the whole arrangement of the
    /// new one is applied right after.
    fn drop_pending_display_changes(&mut self) -> Result<(), SettingsError> {
        let registry = Arc::clone(&self.registry);
        let keys: Vec<String> = self
            .pending
            .keys()
            .filter(|key| {
                registry
                    .resolve(key)
                    .is_ok_and(|found| found.def.scope == Scope::DisplayCombination)
            })
            .cloned()
            .collect();
        for key in &keys {
            self.pending.remove(key);
            let Ok(found) = registry.resolve(key) else {
                continue;
            };
            let value = self.stored_or_default(&found);
            self.events.push(Event::Resolved {
                key: key.clone(),
                outcome: PendingOutcome::DisplaysChanged,
                value,
            });
        }
        Ok(self.store.journal_remove(&keys)?)
    }

    fn apply_combination(&mut self, before: &BTreeMap<String, Value>) {
        let registry = Arc::clone(&self.registry);
        for key in self.combination_keys() {
            let Ok(found) = registry.resolve(&key) else {
                continue;
            };
            let found: Resolved<'_> = found;
            let value = self.effective(&found);
            if self.apply(&found, &value, false).is_err() {
                self.stats.apply_failures += 1;
            }
            if before.get(&key) != Some(&value) {
                self.events.push(Event::Changed { key, value });
            }
        }
    }

    /// Takes note of the displays that are connected now and puts the
    /// arrangement remembered for exactly this combination into effect; a
    /// combination seen for the first time gets the defaults (extend, the
    /// built-in display is the primary one, element size from the pixel
    /// density). Needs administrator rights (the display service has them).
    pub fn set_displays(
        &mut self,
        caller: &Caller,
        descriptors: &[DisplayDescriptor],
    ) -> Result<SetDisplaysReply, SettingsError> {
        if !caller.admin {
            return Err(SettingsError::NotPermitted);
        }
        let displays = identify(descriptors).map_err(|_| SettingsError::BadRequest)?;
        let ids: Vec<String> = displays.iter().map(|display| display.id.clone()).collect();
        let combo = combination_id(&ids);
        if combo != self.combo {
            self.drop_pending_display_changes()?;
        }
        let before = self.combination_values();
        self.displays = displays;
        self.combo = combo;
        let sorted = self.display_ids();
        let known = self
            .store
            .combo(&self.combo)
            .is_some_and(|combo| combo.displays == sorted);
        self.store.use_combo(&self.combo, &sorted);
        self.apply_combination(&before);
        Ok(SetDisplaysReply {
            combination: self.combo.clone(),
            ids,
            first_time: !known,
        })
    }
}
