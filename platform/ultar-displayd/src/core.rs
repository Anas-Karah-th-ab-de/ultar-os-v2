//! The service's logic, without threads or sockets: it follows the
//! compositor's displays and the settings, sends setups, and keeps the
//! "Keep?" window safe.
//!
//! Three layers take a display change back when nobody confirms it:
//! 1. `settingsd` takes the setting back after 15 s (also when the asking
//!    app ended) and announces the old value – the setup follows it;
//! 2. every setup sent while a change waits is *provisional* in the
//!    compositor with the remaining time plus [`GRACE_MS`]: if this service
//!    or `settingsd` hangs or ends, the compositor goes back by itself (and
//!    at once when this service's connection ends);
//! 3. at start, display changes still waiting in `settingsd` (this service
//!    ended during the window) are taken back.

use crate::ids;
use crate::plan::{plan, same_arrangement, upright, Connected};
use crate::ports::{CompositorPort, PortError, Publisher, SettingsPort};
use crate::values::DisplayValues;
use std::collections::{BTreeMap, BTreeSet};
use ultar_bus::messages::display_service::Outputs;
use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{OutputDetails, OutputId, Request, Setup, SetupOutcome, SetupRefusal};

/// Extra time the compositor waits beyond the settings service's deadline.
pub const GRACE_MS: u64 = 3_000;
/// Confirmation time of the settings service (seconds).
pub const KEEP_SECONDS: u32 = 15;
/// Pause before the settings service is asked again after it failed.
pub const RETRY_MS: u64 = 1_000;
/// How long a changed display setting waits for its companion event
/// (`settings.pending` for a new provisional value, `pending_resolved`
/// for one taken back) before the arrangement follows: the three topics
/// arrive on separate subscriptions, and a provisional change must never
/// reach the compositor as a final one.
pub const COMPANION_MS: u64 = 200;

/// The ports of one step.
pub struct Ports<'a> {
    /// `settingsd`.
    pub settings: &'a dyn SettingsPort,
    /// The compositor.
    pub compositor: &'a dyn CompositorPort,
    /// `display.outputs`.
    pub publish: &'a dyn Publisher,
}

/// Something worth a log record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// The compositor refused a setup (its outcome).
    SetupRefused(SetupOutcome),
    /// The compositor went back by itself.
    CompositorReverted(SetupOutcome),
    /// A turned display is shown upright (not drawn turned yet).
    Upright,
    /// Waiting display changes were taken back at start.
    RevertedAtStart(u32),
    /// The settings service did not answer.
    SettingsUnavailable,
}

/// The state.
#[derive(Debug, Default)]
pub struct Core {
    details: BTreeMap<OutputId, OutputDetails>,
    connected: Vec<Connected>,
    reported: Vec<OutputDetails>,
    values: DisplayValues,
    raw: BTreeMap<String, SettingValue>,
    pending: BTreeMap<String, u64>,
    serial: u32,
    sent: Option<Setup>,
    published: Option<Outputs>,
    retry_at: Option<u64>,
    started: bool,
    rearm: bool,
    awaiting: BTreeMap<String, u64>,
    held: bool,
    notes: Vec<Note>,
}

fn key_of_display(key: &str) -> bool {
    key.starts_with("display.")
}

impl Core {
    /// An empty state (no displays known yet).
    pub fn new() -> Core {
        Core::default()
    }

    /// The displays with their settings ids.
    pub fn connected(&self) -> &[Connected] {
        &self.connected
    }

    /// The arrangement settings as followed.
    pub fn values(&self) -> &DisplayValues {
        &self.values
    }

    /// Display keys waiting for "Keep?" (with their deadline, ms).
    pub fn pending(&self) -> &BTreeMap<String, u64> {
        &self.pending
    }

    /// The last setup sent to the compositor.
    pub fn sent(&self) -> Option<&Setup> {
        self.sent.as_ref()
    }

    /// The current value of a followed display setting.
    pub fn raw(&self, key: &str) -> Option<&SettingValue> {
        self.raw.get(key)
    }

    /// Log-worthy things since the last call.
    pub fn take_notes(&mut self) -> Vec<Note> {
        std::mem::take(&mut self.notes)
    }

    /// When [`Core::settle`] must run again (retry, compositor deadline).
    pub fn next_deadline(&self) -> Option<u64> {
        [self.retry_at, self.awaiting.values().min().copied()]
            .into_iter()
            .flatten()
            .min()
    }

    /// The compositor reported a display (new, or changed).
    pub fn on_details(&mut self, d: OutputDetails) {
        self.details.insert(d.id, d);
    }

    /// The compositor removed a display.
    pub fn on_removed(&mut self, id: OutputId) {
        self.details.remove(&id);
    }

    /// The compositor connection ended: what it had is gone.
    pub fn on_compositor_gone(&mut self) {
        self.details.clear();
        self.sent = None;
    }

    /// `settings.changed` for a display key.
    pub fn on_changed(&mut self, key: &str, value: SettingValue) {
        if key_of_display(key) {
            self.values.take(key, &value);
            self.raw.insert(key.to_owned(), value);
        }
    }

    /// `settings.changed` as announced on the bus: the value, and a short
    /// wait for the event that says whether it is provisional.
    pub fn on_announced(&mut self, key: &str, value: SettingValue, now: u64) {
        if key_of_display(key) {
            self.on_changed(key, value);
            self.awaiting.insert(key.to_owned(), now + COMPANION_MS);
        }
    }

    /// `settings.pending` for a display key.
    pub fn on_pending(&mut self, key: &str, seconds: u32, now: u64) {
        self.awaiting.remove(key);
        if key_of_display(key) {
            // A new waiting change is a new attempt: an earlier refusal or
            // step back of the compositor no longer holds the arrangement.
            self.held = false;
            self.pending
                .insert(key.to_owned(), now + u64::from(seconds) * 1000);
        }
    }

    /// `settings.pending_resolved` for a display key.
    pub fn on_resolved(&mut self, key: &str) {
        self.awaiting.remove(key);
        self.pending.remove(key);
        // The settings service answered for the change: its value is what
        // the arrangement follows again.
        self.held = false;
    }

    /// The compositor's answer to a setup.
    pub fn on_setup_done(&mut self, serial: u32, outcome: SetupOutcome, ports: &Ports<'_>) {
        if self.sent.as_ref().map(|s| s.serial) != Some(serial) {
            return;
        }
        match outcome {
            SetupOutcome::Applied | SetupOutcome::Confirmed => {}
            SetupOutcome::Refused(SetupRefusal::UnknownOutput) if self.sent_is_stale() => {
                // Planned while the compositor's display list was still
                // arriving (or changed since): not the person's change but
                // an old view – the next settle plans for the displays now
                // known, nothing is held or taken back.
                self.sent = None;
            }
            SetupOutcome::Refused(_) => {
                self.notes.push(Note::SetupRefused(outcome));
                self.sent = None;
                self.held = true;
                // The waiting change cannot be shown: take it back in the
                // settings too, so both agree.
                self.revert_all_pending(ports);
            }
            SetupOutcome::RevertedTimeout | SetupOutcome::RevertedHotplug => {
                self.notes.push(Note::CompositorReverted(outcome));
                self.sent = None;
                self.held = true;
                self.revert_all_pending(ports);
            }
        }
    }

    /// The last setup names other displays than the compositor reported
    /// since it was sent.
    fn sent_is_stale(&self) -> bool {
        self.sent.as_ref().is_some_and(|sent| {
            let planned: BTreeSet<OutputId> = sent.outputs.iter().map(|o| o.output).collect();
            planned.len() != self.details.len()
                || planned.iter().any(|id| !self.details.contains_key(id))
        })
    }

    /// Until the settings service resolves a change the compositor refused
    /// or took back by itself, no setup is sent: the settings still hold the
    /// change, and sending it would undo the compositor's safe step.
    pub fn held(&self) -> bool {
        self.held
    }

    fn revert_all_pending(&mut self, ports: &Ports<'_>) {
        let keys: Vec<String> = self.pending.keys().cloned().collect();
        for key in keys {
            if ports.settings.revert(&key).is_ok() {
                self.pending.remove(&key);
            }
        }
    }

    /// Brings settings and compositor in line after a batch of events.
    pub fn settle(&mut self, now: u64, ports: &Ports<'_>) {
        if self.retry_at.is_some_and(|t| t > now) {
            return;
        }
        self.retry_at = None;
        let current: Vec<OutputDetails> = self.details.values().cloned().collect();
        if current.is_empty() {
            // No display (or no compositor): nothing to arrange.
            self.connected.clear();
            self.reported.clear();
            self.publish(ports);
            return;
        }
        let changed_set = ids_of(&current) != ids_of(&self.reported) || !self.started;
        if changed_set {
            if let Err(e) = self.sync_displays(&current, now, ports) {
                if e == PortError::Unavailable {
                    self.notes.push(Note::SettingsUnavailable);
                }
                self.retry_at = Some(now + RETRY_MS);
                return;
            }
        }
        for c in &mut self.connected {
            if let Some(d) = self.details.get(&c.details.id) {
                c.details = d.clone();
            }
        }
        self.publish(ports);
        self.awaiting.retain(|_, t| *t > now);
        if !self.awaiting.is_empty() {
            return;
        }
        self.apply(now, ports);
    }

    fn sync_displays(
        &mut self,
        current: &[OutputDetails],
        now: u64,
        ports: &Ports<'_>,
    ) -> Result<(), PortError> {
        let descriptors: Vec<_> = current.iter().map(ids::descriptor).collect();
        let ids = ports.settings.set_displays(&descriptors)?;
        let items = ports.settings.display_items()?;
        self.connected = current
            .iter()
            .zip(ids)
            .map(|(d, id)| Connected {
                details: d.clone(),
                id,
            })
            .collect();
        self.reported = current.to_vec();
        self.values = DisplayValues::default();
        self.raw.clear();
        let mut waiting = Vec::new();
        for item in items {
            if item.pending {
                waiting.push(item.key.clone());
            }
            self.on_changed(&item.key, item.value);
        }
        if !self.started {
            self.started = true;
            self.revert_at_start(&waiting, ports);
        } else {
            // Changes of the old combination were dropped by settingsd.
            self.pending.retain(|k, _| waiting.contains(k));
            if waiting.is_empty() {
                self.held = false;
            }
            for key in waiting {
                self.pending
                    .entry(key)
                    .or_insert(now + u64::from(KEEP_SECONDS) * 1000);
            }
        }
        Ok(())
    }

    /// Display changes that waited when this service started belong to a
    /// window nobody can answer any more: they go back.
    fn revert_at_start(&mut self, waiting: &[String], ports: &Ports<'_>) {
        let mut reverted = 0;
        for key in waiting {
            if ports.settings.revert(key).is_ok() {
                reverted += 1;
            }
        }
        if reverted > 0 {
            self.notes.push(Note::RevertedAtStart(reverted));
            if let Ok(items) = ports.settings.display_items() {
                for item in items {
                    self.on_changed(&item.key, item.value);
                }
            }
        }
    }

    fn publish(&mut self, ports: &Ports<'_>) {
        let outputs = Outputs {
            outputs: self
                .connected
                .iter()
                .map(|c| ids::output(&c.details, &c.id))
                .collect(),
        };
        if self.published.as_ref() != Some(&outputs) {
            ports.publish.outputs(&outputs);
            self.published = Some(outputs);
        }
    }

    fn apply(&mut self, now: u64, ports: &Ports<'_>) {
        if self.connected.is_empty() || self.held {
            return;
        }
        let remaining = self.pending.values().map(|d| d.saturating_sub(now)).max();
        let provisional_ms = remaining.map_or(0, |r| (r + GRACE_MS).min(120_000) as u32);
        let mut next = plan(
            &self.connected,
            &self.values,
            self.serial.wrapping_add(1),
            provisional_ms,
        );
        if upright(&mut next) {
            self.notes.push(Note::Upright);
        }
        if let Some(sent) = &self.sent {
            let same = same_arrangement(sent, &next);
            match (sent.provisional_ms > 0, provisional_ms > 0) {
                (false, false) | (true, true) if same && !self.rearm => return,
                (true, false) if same => {
                    let serial = sent.serial;
                    if ports.compositor.send(Request::ConfirmOutputs { serial }) {
                        if let Some(s) = self.sent.as_mut() {
                            s.provisional_ms = 0;
                        }
                    }
                    return;
                }
                _ => {}
            }
        }
        self.rearm = false;
        self.serial = next.serial;
        if ports.compositor.send(Request::ConfigureOutputs {
            setup: next.clone(),
        }) {
            self.sent = Some(next);
        }
    }

    /// Events were lost: the next settle reads the settings again.
    pub fn resync(&mut self) {
        self.reported.clear();
    }

    /// "Give me more time": the next settle sends the setup again with the
    /// new deadline.
    pub fn rearm(&mut self) {
        self.rearm = true;
    }

    /// Display ids in the order of their numbers (`display.identify`):
    /// the given order, else the main display first and then by connector.
    pub fn numbers(&self, order: &[String]) -> Vec<(OutputId, u8)> {
        let mut list: Vec<&Connected> = order
            .iter()
            .filter_map(|id| self.connected.iter().find(|c| &c.id == id))
            .collect();
        if list.is_empty() {
            list = self.connected.iter().collect();
        }
        list.iter()
            .take(16)
            .enumerate()
            .map(|(i, c)| (c.details.id, i as u8 + 1))
            .collect()
    }
}

fn ids_of(list: &[OutputDetails]) -> Vec<(OutputId, &ultar_surface::proto::OutputIdentity, &str)> {
    list.iter()
        .map(|d| (d.id, &d.identity, d.connector.as_str()))
        .collect()
}

#[cfg(test)]
#[path = "core_tests.rs"]
mod tests;
