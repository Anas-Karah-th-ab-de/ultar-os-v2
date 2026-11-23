//! The session and the trusted path: the lock (fail closed), secure
//! attention keys, system dialogs, focus activation tokens and the
//! recording indicator.
//!
//! The lock is compositor state. It is set by the first lock surface, by
//! Super+L and by Ctrl+Alt+Del, and cleared only by `Unlock` from a client
//! with the lock screen right that shows a lock surface (it checked the
//! password with the authentication service). A lock client that dies
//! leaves the session locked: the screen shows the ground colour and no
//! input goes anywhere until a new lock client attaches.

use super::{ClientId, State, SurfaceKey};
use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::hash::{BuildHasher, Hasher};
use ultar_hal::KeyState;
use ultar_surface::proto::{ErrorCode, Event, Modifiers, Rights, Role, SurfaceId};

/// Activation tokens outstanding at most (the oldest is dropped).
const MAX_TOKENS: usize = 16;
/// Milliseconds an activation token stays valid.
const TOKEN_MS: u64 = 30_000;
/// Linux code of the Delete key.
const KEY_DELETE: u16 = 111;
/// Linux code of the L key.
const KEY_L: u16 = 38;
/// Milliseconds the recording indicator stays after the last capture data.
pub const INDICATOR_MS: u64 = 2_000;

/// Session state of the compositor.
#[derive(Debug, Default)]
pub struct Session {
    /// The session is locked.
    pub locked: bool,
    /// Outstanding activation tokens (token, expiry).
    tokens: Vec<(u64, u64)>,
    /// Keyed hasher for tokens (keys from the OS random source).
    token_seed: RandomState,
    /// Tokens made so far.
    token_count: u64,
    /// Serial at the last change of the keyboard focus.
    pub(super) focus_serial: u32,
    /// The recording indicator shows until this time.
    pub recording_until: u64,
    /// The recording indicator is drawn now.
    pub indicator_shown: bool,
    /// Key codes pressed and the surface that got the press.
    pub(super) pressed: HashMap<u16, SurfaceKey>,
    /// Key codes whose release the compositor keeps (secure attention).
    pub(super) swallowed: Vec<u16>,
    /// Translucent surfaces left out of the scene (overdraw budget).
    pub culled: Vec<SurfaceKey>,
}

impl Session {
    fn new_token(&mut self, now: u64) -> u64 {
        self.tokens.retain(|&(_, expiry)| expiry > now);
        if self.tokens.len() >= MAX_TOKENS {
            self.tokens.remove(0);
        }
        self.token_count += 1;
        let mut h = self.token_seed.build_hasher();
        h.write_u64(self.token_count);
        h.write_u64(now);
        let token = h.finish().max(1);
        self.tokens.push((token, now + TOKEN_MS));
        token
    }

    fn take_token(&mut self, token: u64, now: u64) -> bool {
        let found = self
            .tokens
            .iter()
            .position(|&(t, expiry)| t == token && expiry > now);
        if let Some(i) = found {
            self.tokens.remove(i);
        }
        found.is_some()
    }
}

impl State {
    /// True while the session is locked: only the lock screen and the
    /// on-screen keyboard are drawn and get input.
    pub fn locked(&self) -> bool {
        self.session.locked
    }

    /// The lock surface that shows (the newest one with content).
    pub fn lock_surface(&self) -> Option<SurfaceKey> {
        self.layers.iter().rev().copied().find(|k| {
            self.surface(*k)
                .is_some_and(|s| s.role == Role::Lock && s.image.is_some())
        })
    }

    /// The topmost visible system dialog.
    pub fn system_dialog(&self) -> Option<SurfaceKey> {
        self.windows
            .iter()
            .rev()
            .find(|w| w.secure && w.visible())
            .map(|w| w.key)
    }

    /// Where keys and input-method text go: the lock surface while locked
    /// (nowhere if none), else a system dialog, else the keyboard focus.
    pub fn text_target(&self) -> Option<SurfaceKey> {
        if self.locked() {
            return self.lock_surface();
        }
        self.system_dialog().or(self.focus)
    }

    /// Locks the session (lock surface, Super+L, Ctrl+Alt+Del). Grabs end,
    /// touches are cancelled, the shell and lock clients are told.
    pub fn lock(&mut self) {
        if self.session.locked {
            return;
        }
        self.session.locked = true;
        self.pointer.grab = None;
        if let Some(r) = self.pointer.snap_preview.take() {
            self.damage_logical(r);
        }
        self.transfers.cancel_drag();
        self.touch_cancel();
        self.damage_all();
        self.notify_session();
        self.update_hover();
        self.update_ime();
    }

    /// Unlocks the session: only a lock screen client showing `surface`.
    pub(super) fn unlock(&mut self, client: ClientId, surface: SurfaceId) -> Result<(), ErrorCode> {
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        if !c.rights.contains(Rights::LOCK_SCREEN) {
            return Err(ErrorCode::Permission);
        }
        let s = c.surfaces.get(&surface).ok_or(ErrorCode::UnknownObject)?;
        if s.role != Role::Lock || s.image.is_none() || !self.session.locked {
            return Err(ErrorCode::Permission);
        }
        self.session.locked = false;
        self.damage_all();
        self.notify_session();
        self.update_hover();
        self.update_ime();
        Ok(())
    }

    fn notify_session(&mut self) {
        let locked = self.session.locked;
        let wanted = Rights::SHELL.union(Rights::LOCK_SCREEN);
        let ids: Vec<ClientId> = self
            .clients
            .iter()
            .filter(|(_, c)| c.rights.0 & wanted.0 != 0)
            .map(|(&id, _)| id)
            .collect();
        for id in ids {
            self.send(id, Event::SessionLocked { locked });
        }
    }

    /// May `client` take the keyboard focus without a token? Yes if nothing
    /// has it, if the client has it, or if the user acted on the client
    /// since the focus last moved (within the serial lifetime).
    pub(super) fn may_take_focus(&self, client: ClientId) -> bool {
        let Some(focus) = self.focus else {
            return true;
        };
        if focus.client == client {
            return true;
        }
        let since = self.session.focus_serial;
        let max_age = self.config.limits.serial_ms;
        self.clients.get(&client).is_some_and(|c| {
            c.serials
                .iter()
                .any(|&(s, t)| s > since && self.now.saturating_sub(t) <= max_age)
        })
    }

    /// Answers `RequestActivation`: a token for a recent user action of
    /// the client (token 0 = refused).
    pub(super) fn request_activation(&mut self, client: ClientId, serial: u32) {
        let max_age = self.config.limits.serial_ms;
        let now = self.now;
        let ok = self
            .clients
            .get(&client)
            .is_some_and(|c| c.recent_serial(serial, now, max_age));
        let token = if ok { self.session.new_token(now) } else { 0 };
        self.send(client, Event::ActivationToken { token });
    }

    /// `Activate`: a valid token focuses the window now, or when it maps.
    pub(super) fn activate(
        &mut self,
        client: ClientId,
        surface: SurfaceId,
        token: u64,
    ) -> Result<(), ErrorCode> {
        let key = SurfaceKey { client, surface };
        let index = self.window_index(key).ok_or(ErrorCode::UnknownObject)?;
        if !self.session.take_token(token, self.now) {
            return Ok(());
        }
        if self.windows[index].visible() {
            self.focus_window(key);
        } else {
            self.windows[index].activate_on_map = true;
        }
        Ok(())
    }

    /// Secure attention keys (Ctrl+Alt+Del, Super+L): only the compositor
    /// sees them; they lock the session. True if `code` was one.
    pub(super) fn secure_attention(&mut self, code: u16, state: KeyState) -> bool {
        if let Some(i) = self.session.swallowed.iter().position(|&c| c == code) {
            if state == KeyState::Released {
                self.session.swallowed.remove(i);
            }
            return true;
        }
        let m = Modifiers(self.keyboard.modifiers);
        let sak = state == KeyState::Pressed
            && ((m.has(Modifiers::CTRL) && m.has(Modifiers::ALT) && code == KEY_DELETE)
                || (m.has(Modifiers::SUPER) && code == KEY_L));
        if sak {
            self.session.swallowed.push(code);
            self.lock();
        }
        sak
    }

    /// True if the recording indicator should show now.
    pub fn recording(&self) -> bool {
        self.now < self.session.recording_until
            || self.clients.values().any(|c| c.capture.is_some())
    }

    /// Shows or hides the recording indicator (damage where it is drawn).
    pub fn tick_indicator(&mut self) {
        let shown = self.recording();
        if shown != self.session.indicator_shown {
            self.session.indicator_shown = shown;
            let rtl = self.rtl();
            let rects: Vec<_> = self
                .layout
                .enabled()
                .map(|o| crate::cursor::indicator_rect(o, rtl))
                .collect();
            for r in rects {
                self.damage_logical(r);
            }
        }
    }

    /// Removes what the session keeps about a client.
    pub(super) fn session_forget(&mut self, client: ClientId) {
        self.session.pressed.retain(|_, k| k.client != client);
        self.session.culled.retain(|k| k.client != client);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_single_use_bounded_and_expire() {
        let mut s = Session::default();
        let t = s.new_token(0);
        assert_ne!(t, 0);
        assert!(s.take_token(t, 10));
        assert!(!s.take_token(t, 10), "single use");
        let late = s.new_token(0);
        assert!(!s.take_token(late, TOKEN_MS), "expired");
        let all: Vec<u64> = (0..MAX_TOKENS + 4).map(|i| s.new_token(i as u64)).collect();
        assert!(s.tokens.len() <= MAX_TOKENS);
        assert!(!s.take_token(all[0], 100), "oldest dropped");
        assert!(s.take_token(all[MAX_TOKENS + 3], 100));
        assert!(!s.take_token(12345, 100));
    }
}
