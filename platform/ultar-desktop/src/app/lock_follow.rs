//! The lock screen follows the settings (`users.*`, `emergency.*`): read at start and on
//! `settings.changed`, automatic sign-in at a fresh start, and the emergency call.
//! Split from [`super::lock`] so that file stays the loop and the events.

use super::lock::LockApp;
use super::AppError;
use crate::lock::prefs;
use crate::ports::{AuthPort, LockCall, SettingsPort};
use std::sync::mpsc::channel;
use ultar_surface::proto::Request;

/// How often the settings are read again until the service answers (ms).
const SETTINGS_RETRY_MS: u64 = 2000;

impl LockApp {
    /// Reads the lock screen's settings once (at start, and again until the settings
    /// service answered – it may start after the lock screen).
    pub(super) fn read_settings(&mut self) {
        self.feeds.next_settings_ms = self.clock.now_ms().saturating_add(SETTINGS_RETRY_MS);
        let Some(ports) = self.ports.clone() else {
            return;
        };
        let mut answered = false;
        for key in prefs::KEYS {
            match ports.get(key) {
                Ok(value) => {
                    answered = true;
                    self.state.prefs.on_changed(key, &value);
                }
                // No settings service (yet): try again later, not eleven timeouts.
                Err(_) => break,
            }
        }
        if answered {
            self.feeds.settings_known = true;
            self.after_prefs();
        }
    }

    /// `settings.changed`: the values take effect at once; until the settings service
    /// answered once, the values are read again every little while.
    pub(super) fn follow_settings(&mut self) {
        let mut changed = false;
        while let Some(Ok(Some(c))) = self.feeds.settings.as_ref().map(|s| s.try_recv()) {
            changed |= self.state.prefs.on_changed(&c.key, &c.value);
        }
        if !self.feeds.settings_known && self.clock.now_ms() >= self.feeds.next_settings_ms {
            self.read_settings();
            changed = true;
            // The settings came late (settingsd starts after the lock screen): the
            // automatic sign-in of a fresh start still happens.
            if self.feeds.settings_known && self.state.owner.is_none() {
                self.try_auto_login();
            }
        }
        if changed {
            self.after_prefs();
            let _ = self.show();
        }
    }

    /// What a settings change means for the screen: the user list comes or goes, and
    /// the emergency panel closes when its button is switched off.
    fn after_prefs(&mut self) {
        if !self.state.prefs.emergency_on {
            self.state.emergency.open = false;
        }
        self.refresh_user();
        self.apply_look();
    }

    /// `users.auto_login`: at a fresh start (nobody signed in) the chosen user signs in
    /// without a password – never into a locked session of somebody else, never outside
    /// the parental window. The first frame is drawn, then lifted like a checked sign-in.
    pub(super) fn try_auto_login(&mut self) {
        let name = self.state.prefs.auto_login.clone();
        if name.is_empty() || self.user.is_some() || self.pending.auto {
            return;
        }
        let Some(ports) = self.ports.clone() else {
            return;
        };
        // A running session (also a restarted lock screen) keeps its owner.
        if !ports.session_users().is_ok_and(|users| users.is_empty()) {
            return;
        }
        let Ok(users) = ports.users() else {
            return;
        };
        let Some(info) = users.iter().find(|u| u.name == name).cloned() else {
            return;
        };
        let now = self.clock.now_ms();
        if !self.state.prefs.allows(self.local_min(now), info.admin) {
            return;
        }
        self.state.user = Some(info);
        self.state.users = users;
        self.state.owner = Some(name.clone());
        self.user = Some(name);
        self.pending.auto = true;
        self.state.locked();
        self.state.show_sign_in();
    }

    /// The first frame of the automatic sign-in is there: tell the session service and
    /// lift the lock. When it refuses, the normal sign-in shows instead.
    pub(super) fn finish_auto_login(&mut self) -> Result<(), AppError> {
        if !self.pending.auto || !self.drawn() {
            return Ok(());
        }
        self.pending.auto = false;
        let Some(name) = self.state.owner.clone() else {
            return Ok(());
        };
        let signed_in = self
            .ports
            .as_ref()
            .is_some_and(|p| p.lock_call(&LockCall::SignedIn(name)) == Ok(true));
        if signed_in {
            if let Some(surface) = self.surface {
                self.host.client.send(&Request::Unlock { surface })?;
            }
            self.state.unlocked();
        } else {
            self.state.owner = None;
            self.user = None;
        }
        self.show()
    }

    /// The emergency button's call: `call.emergency` on a worker thread (the call
    /// service reads and validates the number itself; the lock screen sends none).
    pub(super) fn start_emergency_call(&mut self) {
        if self.state.emergency.sos_active() || self.feeds.emergency.is_some() {
            return;
        }
        let now = self.clock.now_ms();
        self.state.emergency.calling(now);
        let Some(ports) = self.ports.clone() else {
            self.state.emergency.answered(None);
            return;
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(ports.emergency_call());
        });
        self.feeds.emergency = Some(rx);
    }

    /// The call's answer and its end (`call.state`); the SOS light runs meanwhile.
    pub(super) fn follow_emergency(&mut self) {
        if let Some(rx) = &self.feeds.emergency {
            if let Ok(id) = rx.try_recv() {
                self.feeds.emergency = None;
                self.state.emergency.answered(id);
                let _ = self.show();
            }
        }
        let before = self.state.emergency.phase;
        while let Some(Ok(Some(state))) = self.feeds.call.as_ref().map(|s| s.try_recv()) {
            self.state.emergency.follow_call(&state);
        }
        if self.state.emergency.phase != before {
            let _ = self.show();
        }
    }
}
