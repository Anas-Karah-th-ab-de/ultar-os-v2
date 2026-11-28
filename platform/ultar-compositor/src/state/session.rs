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
use ultar_surface::proto::{ErrorCode, Event, Modifiers, Rights, Role, SurfaceId, WindowEntry};

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
    /// Outstanding activation tokens.
    tokens: Vec<Grant>,
    /// Counts focus changes the user made (click, touch, Alt+Tab): a
    /// token made before one is void.
    pub(super) user_epoch: u64,
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
    /// The desktop shell's keyboard, list and shortcut state.
    pub shell: super::ShellSession,
    /// Output management of the display service (provisional setup,
    /// display numbers).
    pub manage: super::output_config::OutputManagement,
    /// The accessibility view: magnifier, colour filter and the alert flash.
    pub visual: super::visual::Visual,
}

/// What an activation token allows: focus until `expiry`, as long as the
/// user has not moved the focus since it was made (`epoch`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The token.
    pub token: u64,
    /// Valid before this time.
    pub expiry: u64,
    /// [`Session::user_epoch`] when it was made.
    pub epoch: u64,
}

impl Session {
    fn new_token(&mut self, now: u64) -> u64 {
        self.tokens.retain(|g| g.expiry > now);
        if self.tokens.len() >= MAX_TOKENS {
            self.tokens.remove(0);
        }
        self.token_count += 1;
        let mut h = self.token_seed.build_hasher();
        h.write_u64(self.token_count);
        h.write_u64(now);
        let token = h.finish().max(1);
        self.tokens.push(Grant {
            token,
            expiry: now + TOKEN_MS,
            epoch: self.user_epoch,
        });
        token
    }

    /// Takes a token (single use); `None` if unknown, expired or void.
    fn take_token(&mut self, token: u64, now: u64) -> Option<Grant> {
        let i = self.tokens.iter().position(|g| g.token == token)?;
        let grant = self.tokens.remove(i);
        self.valid(&grant, now).then_some(grant)
    }

    /// A grant still allows focus.
    pub(super) fn valid(&self, grant: &Grant, now: u64) -> bool {
        grant.expiry > now && grant.epoch == self.user_epoch
    }
}

impl State {
    /// `ListWindows` (capture right): every toplevel, bottom to top, with
    /// whether it is shown and how its pixels arrived.
    pub(super) fn list_windows(&mut self, client: ClientId) -> Result<(), ErrorCode> {
        let c = self.clients.get(&client).ok_or(ErrorCode::UnknownObject)?;
        if !c.rights.contains(Rights::CAPTURE) {
            return Err(ErrorCode::Permission);
        }
        let windows = self
            .windows
            .iter()
            .filter_map(|w| {
                let s = self.surface(w.key)?;
                Some(WindowEntry {
                    app_id: self.clients.get(&w.key.client)?.app_id.clone(),
                    title: s.title.clone(),
                    mapped: w.mapped,
                    shared_memory: s.shared,
                })
            })
            .collect();
        self.send(client, Event::WindowList { windows });
        Ok(())
    }

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
        self.system_dialog()
            .or(self.session.shell.focus)
            .or(self.focus)
    }

    /// True if a client that can show a lock screen is connected (holds
    /// `ui.lock-screen` and said `Hello`).
    pub fn lock_provider(&self) -> bool {
        self.clients
            .values()
            .any(|c| c.hello && c.rights.contains(Rights::LOCK_SCREEN))
    }

    /// Locks the session (lock surface, Super+L, Ctrl+Alt+Del). Grabs end,
    /// touches are cancelled, the shell and lock clients are told. Refused
    /// (false) when no lock screen client is connected: a lock nobody can
    /// ever lift would end the session (fail safe, not fail dead). Once
    /// locked, the lock stays even if that client dies.
    pub fn lock(&mut self) -> bool {
        if self.session.locked {
            return true;
        }
        if !self.lock_provider() {
            return false;
        }
        self.session.locked = true;
        self.configure_call_screens();
        self.drop_shell_focus();
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
        true
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
        self.configure_call_screens();
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
    /// the client (token 0 = refused). The serial is used up: one user
    /// action buys one token.
    pub(super) fn request_activation(&mut self, client: ClientId, serial: u32) {
        let max_age = self.config.limits.serial_ms;
        let now = self.now;
        let ok = self.clients.get_mut(&client).is_some_and(|c| {
            let fresh = c.recent_serial(serial, now, max_age);
            c.serials.retain(|&(s, _)| s != serial);
            fresh
        });
        let token = if ok { self.session.new_token(now) } else { 0 };
        self.send(client, Event::ActivationToken { token });
    }

    /// `Activate`: a valid token focuses the window now, or when it maps
    /// before the token would have expired and before the user moves the
    /// focus.
    pub(super) fn activate(
        &mut self,
        client: ClientId,
        surface: SurfaceId,
        token: u64,
    ) -> Result<(), ErrorCode> {
        let key = SurfaceKey { client, surface };
        let index = self.window_index(key).ok_or(ErrorCode::UnknownObject)?;
        let Some(grant) = self.session.take_token(token, self.now) else {
            return Ok(());
        };
        if self.windows[index].visible() {
            self.focus_window(key);
        } else {
            self.windows[index].activate_on_map = Some(grant);
        }
        Ok(())
    }

    /// The user moved the focus to `key` (click, touch, Alt+Tab): earlier
    /// activation tokens and parked activations become void.
    pub(super) fn user_focus(&mut self, key: SurfaceKey) {
        if self.focus != Some(key) {
            self.session.user_epoch += 1;
        }
        self.focus_window(key);
    }

    /// Secure attention keys (Ctrl+Alt+Del, Super+L): only the compositor
    /// sees them; they lock the session if a lock screen client is
    /// connected, else they do nothing (still never forwarded). True if
    /// `code` was one.
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
            let _ = self.lock();
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
        assert!(s.take_token(t, 10).is_some());
        assert!(s.take_token(t, 10).is_none(), "single use");
        let late = s.new_token(0);
        assert!(s.take_token(late, TOKEN_MS).is_none(), "expired");
        let all: Vec<u64> = (0..MAX_TOKENS + 4).map(|i| s.new_token(i as u64)).collect();
        assert!(s.tokens.len() <= MAX_TOKENS);
        assert!(s.take_token(all[0], 100).is_none(), "oldest dropped");
        assert!(s.take_token(all[MAX_TOKENS + 3], 100).is_some());
        assert!(s.take_token(12345, 100).is_none());
        let before = s.new_token(200);
        s.user_epoch += 1;
        assert!(
            s.take_token(before, 201).is_none(),
            "void after a user focus change"
        );
    }
}
