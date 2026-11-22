//! Changing values: `set` and `reset`.

use super::{slot_of, Core, Event, Pending, MAX_PENDING};
use crate::access::Caller;
use crate::error::SettingsError;
use crate::schema::{Resolved, Risk, Scope, TextFormat, ValueError, ValueType};
use crate::store::MAX_VALUES;
use crate::value::Value;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::PendingOutcome;

impl Core {
    /// Values that name displays must name connected ones.
    fn check_references(&self, found: &Resolved<'_>, value: &Value) -> Result<(), SettingsError> {
        let connected = |id: &String| id.is_empty() || self.displays.iter().any(|d| d.id == *id);
        let valid = match (&found.def.value_type, value) {
            (ValueType::Text(TextFormat::DisplayId), Value::Str(id)) => connected(id),
            (
                ValueType::List {
                    item: TextFormat::DisplayId,
                    ..
                },
                Value::List(ids),
            ) => ids.iter().all(connected),
            _ => true,
        };
        valid
            .then_some(())
            .ok_or(SettingsError::Value(ValueError::BadFormat {
                format: TextFormat::DisplayId.name(),
            }))
    }

    /// Checks everything that does not depend on the current value.
    fn admit(
        &self,
        caller: &Caller,
        found: &Resolved<'_>,
        value: Option<Value>,
    ) -> Result<Option<Value>, SettingsError> {
        self.check_instance(found)?;
        if !self.capabilities.flags.contains_all(found.def.requires) {
            return Err(SettingsError::Unavailable);
        }
        if found.def.risk == Risk::AdminOnly && !caller.admin {
            return Err(SettingsError::NotPermitted);
        }
        let Some(value) = value else {
            return Ok(None);
        };
        let value = found.def.value_type.accept(value)?;
        self.check_references(found, &value)?;
        Ok(Some(value))
    }

    /// Changes one setting. Returns `false` when the value was already in
    /// effect. Settings of the confirm-or-revert class take effect
    /// provisionally and announce a deadline.
    pub fn set(&mut self, caller: &Caller, key: &str, value: Value) -> Result<bool, SettingsError> {
        let registry = Arc::clone(&self.registry);
        let found = registry.resolve(key)?;
        let target = self.admit(caller, &found, Some(value))?;
        self.change(&found, target)
    }

    /// Returns one setting to its default. Returns `false` when the default
    /// was already in effect.
    pub fn reset(&mut self, caller: &Caller, key: &str) -> Result<bool, SettingsError> {
        let registry = Arc::clone(&self.registry);
        let found = registry.resolve(key)?;
        self.admit(caller, &found, None)?;
        self.change(&found, None)
    }

    fn change(
        &mut self,
        found: &Resolved<'_>,
        target: Option<Value>,
    ) -> Result<bool, SettingsError> {
        let old = self.effective(found);
        let new = match &target {
            Some(value) => value.clone(),
            None => self.default_value(found.def, found.instance.as_deref()),
        };
        // Going back to the value from before the provisional change is a
        // revert, not a second change that needs confirmation.
        let back_to_previous = self
            .pending
            .get(&found.key)
            .is_some_and(|pending| pending.previous == new);
        if back_to_previous {
            self.revert_keys(std::slice::from_ref(&found.key), PendingOutcome::Reverted)?;
            return Ok(true);
        }
        match found.def.risk {
            Risk::ConfirmRevert { seconds } => {
                self.change_provisional(found, target, new, old, seconds)
            }
            Risk::Safe | Risk::AdminOnly => self.change_now(found, target, new, old),
        }
    }

    /// Makes sure the active display combination has an entry in the store.
    pub(super) fn ensure_slot(&mut self, scope: Scope) {
        if scope == Scope::DisplayCombination && self.store.combo(&self.combo).is_none() {
            let ids = self.display_ids();
            self.store.use_combo(&self.combo, &ids);
        }
    }

    fn change_now(
        &mut self,
        found: &Resolved<'_>,
        target: Option<Value>,
        new: Value,
        old: Value,
    ) -> Result<bool, SettingsError> {
        self.ensure_slot(found.def.scope);
        let slot = slot_of(found.def.scope, &self.combo);
        if new == old {
            if target.is_none() {
                // Stop pinning a value that equals the default.
                self.store.remove(slot, &found.key);
            }
            return Ok(false);
        }
        let is_new_entry = target.is_some() && self.store.get(slot, &found.key).is_none();
        if is_new_entry && self.store.len(slot) >= MAX_VALUES {
            return Err(SettingsError::Limit);
        }
        self.apply(found, &new, false)?;
        let slot = slot_of(found.def.scope, &self.combo);
        match target {
            Some(value) => self.store.set(slot, &found.key, value),
            None => self.store.remove(slot, &found.key),
        };
        self.events.push(Event::Changed {
            key: found.key.clone(),
            value: new,
        });
        Ok(true)
    }

    fn change_provisional(
        &mut self,
        found: &Resolved<'_>,
        target: Option<Value>,
        new: Value,
        old: Value,
        seconds: u32,
    ) -> Result<bool, SettingsError> {
        if new == old {
            return Ok(false);
        }
        let fresh = !self.pending.contains_key(&found.key);
        if fresh {
            if self.pending.len() >= MAX_PENDING {
                return Err(SettingsError::Limit);
            }
            // Durable before the value takes effect: after a crash the
            // service knows what to take back.
            self.store.journal_add(&found.key)?;
        }
        if let Err(error) = self.apply(found, &new, true) {
            if fresh {
                let _ = self.store.journal_remove(std::slice::from_ref(&found.key));
            }
            return Err(error);
        }
        let previous = match self.pending.get(&found.key) {
            Some(pending) => pending.previous.clone(),
            None => old,
        };
        let deadline = self
            .clock
            .now()
            .saturating_add(Duration::from_secs(u64::from(seconds)));
        let pending = Pending {
            previous: previous.clone(),
            target,
            deadline,
        };
        self.pending.insert(found.key.clone(), pending);
        self.events.push(Event::Changed {
            key: found.key.clone(),
            value: new.clone(),
        });
        self.events.push(Event::Pending {
            key: found.key.clone(),
            value: new,
            previous,
            seconds,
        });
        Ok(true)
    }
}
