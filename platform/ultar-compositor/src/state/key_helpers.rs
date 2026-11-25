//! The keyboard helpers of the accessibility page and the compose key in the key input
//! path (`Request::ConfigureKeyHelpers`, settings `accessibility.{sticky,slow,bounce,mouse}_keys`
//! and `keyboard.compose_key`). Bounce and slow keys filter the stream before a key counts;
//! what counts is thinned by the key repeat, may become a pointer action (mouse keys), and
//! otherwise goes through the compose key, sticky keys and the keyboard path (`keys.rs`).

use super::bounce_keys::BounceKeys;
use super::compose::Compose;
use super::mouse_keys::MouseKeys;
use super::slow_keys::SlowKeys;
use super::sticky_keys::StickyKeys;
use super::State;
use ultar_hal::KeyState;
use ultar_surface::proto::ComposeKey;

/// The keyboard helpers the compositor follows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct KeyHelpers {
    /// Sticky keys.
    pub sticky: StickyKeys,
    /// Slow keys.
    pub slow: SlowKeys,
    /// Bounce keys.
    pub bounce: BounceKeys,
    /// Mouse keys.
    pub mouse: MouseKeys,
    /// The compose key.
    pub compose: Compose,
}

impl State {
    /// `ConfigureKeyHelpers`: the display service passes the settings on. Values are already
    /// checked by the decoder; a press held while slow keys turn off counts at once.
    pub(super) fn configure_key_helpers(
        &mut self,
        sticky: bool,
        slow: bool,
        bounce: bool,
        mouse: bool,
        compose: ComposeKey,
    ) {
        let was_slow = self.keyboard.helpers.slow.on;
        let h = &mut self.keyboard.helpers;
        h.sticky.set_on(sticky);
        h.slow.set_on(slow);
        h.bounce.set_on(bounce);
        h.mouse.on = mouse;
        h.compose.set_key(compose);
        if was_slow && !slow {
            let held = self.keyboard.helpers.slow.drain();
            let now = self.now;
            for code in held {
                self.key_counted(code, KeyState::Pressed, now.saturating_mul(1000));
            }
        }
    }

    /// The key input path with the helpers (called from `input()`).
    pub(super) fn key_helper_input(&mut self, code: u16, state: KeyState, time_us: u64) {
        let h = &mut self.keyboard.helpers;
        if h.bounce.is_bounce(code, state, time_us) {
            return;
        }
        if h.slow.intercept(code, state, self.now) {
            return;
        }
        self.key_counted(code, state, time_us);
    }

    /// A key that counted: the key repeat thins it, mouse keys may take it, else the
    /// keyboard path (compose key, sticky keys, shortcuts, delivery).
    pub(super) fn key_counted(&mut self, code: u16, state: KeyState, time_us: u64) {
        if !self.keyboard.repeat.passes(code, state, time_us) {
            return;
        }
        if self.keyboard.helpers.mouse.on && self.mouse_key_action(code, state) {
            return;
        }
        self.key(code, state);
    }

    /// Slow keys: the presses held long enough count now (the loop's tick).
    pub fn tick_slow_keys(&mut self) {
        let now = self.now;
        for code in self.keyboard.helpers.slow.take_due(now) {
            self.key_counted(code, KeyState::Pressed, now.saturating_mul(1000));
        }
    }

    /// Earliest time a slow-keys press counts (for the loop's timeout).
    pub(super) fn slow_keys_deadline(&self) -> Option<u64> {
        self.keyboard.helpers.slow.next_due()
    }
}
