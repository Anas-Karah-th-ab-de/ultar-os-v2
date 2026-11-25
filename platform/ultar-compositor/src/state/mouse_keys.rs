//! Mouse keys (`accessibility.mouse_keys`, `Request::ConfigureKeyHelpers`): the numeric
//! keypad moves the pointer and clicks. The eight direction keys move it a step at a time
//! (held keys repeat through the key-repeat path), 5 clicks, + double-clicks, - clicks the
//! secondary button, * the middle one, 0 holds the primary button and . (or releasing 0)
//! lets it go. The pointer's primary-button swap applies to these clicks too.

use super::input::{BTN_PRIMARY, BTN_SECONDARY};
use super::State;
use ultar_hal::KeyState;

/// How far the pointer moves per keypad step, in logical points.
pub const MOUSE_KEYS_STEP: f64 = 20.0;
/// The middle button's Linux code.
const BTN_MIDDLE: u16 = 0x112;

/// Mouse keys: whether the numeric keypad drives the pointer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseKeys {
    /// The setting is on.
    pub on: bool,
}

/// What a keypad key does to the pointer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MouseAction {
    /// Move by this many logical points.
    Move(f64, f64),
    /// Press and release this button.
    Click(u16),
    /// Two primary clicks.
    DoubleClick,
    /// Press the primary button and keep it held.
    Hold,
    /// Let the primary button go.
    Release,
}

/// The pointer action of a keypad key, if it is one. Directions act on press and repeat;
/// clicks on press only; 0 also releases on its way up.
pub fn action(code: u16, state: KeyState) -> Option<MouseAction> {
    let pressed = state == KeyState::Pressed;
    let moving = pressed || state == KeyState::Repeated;
    let s = MOUSE_KEYS_STEP;
    Some(match code {
        71 if moving => MouseAction::Move(-s, -s),
        72 if moving => MouseAction::Move(0.0, -s),
        73 if moving => MouseAction::Move(s, -s),
        75 if moving => MouseAction::Move(-s, 0.0),
        77 if moving => MouseAction::Move(s, 0.0),
        79 if moving => MouseAction::Move(-s, s),
        80 if moving => MouseAction::Move(0.0, s),
        81 if moving => MouseAction::Move(s, s),
        76 if pressed => MouseAction::Click(BTN_PRIMARY),
        74 if pressed => MouseAction::Click(BTN_SECONDARY),
        55 if pressed => MouseAction::Click(BTN_MIDDLE),
        78 if pressed => MouseAction::DoubleClick,
        82 if pressed => MouseAction::Hold,
        82 if state == KeyState::Released => MouseAction::Release,
        83 if pressed => MouseAction::Release,
        _ => return None,
    })
}

impl State {
    /// A numeric keypad key as a pointer action. True if the key was used (not delivered).
    pub(super) fn mouse_key_action(&mut self, code: u16, state: KeyState) -> bool {
        let Some(action) = action(code, state) else {
            return false;
        };
        match action {
            MouseAction::Move(dx, dy) => {
                let (x, y) = (self.pointer.x + dx, self.pointer.y + dy);
                self.pointer_to(x, y);
            }
            MouseAction::Click(button) => {
                let button = self.mouse_button(button);
                self.button(button, true);
                self.button(button, false);
            }
            MouseAction::DoubleClick => {
                let button = self.mouse_button(BTN_PRIMARY);
                for _ in 0..2 {
                    self.button(button, true);
                    self.button(button, false);
                }
            }
            MouseAction::Hold => {
                let button = self.mouse_button(BTN_PRIMARY);
                self.button(button, true);
            }
            MouseAction::Release => {
                if self.pointer.held > 0 {
                    let button = self.mouse_button(BTN_PRIMARY);
                    self.button(button, false);
                }
            }
        }
        true
    }

    /// The pointer's primary-button swap applied to a mouse-keys button.
    fn mouse_button(&self, code: u16) -> u16 {
        match (self.pointer.swap_buttons, code) {
            (true, BTN_PRIMARY) => BTN_SECONDARY,
            (true, BTN_SECONDARY) => BTN_PRIMARY,
            _ => code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_eight_directions_move_a_step() {
        let s = MOUSE_KEYS_STEP;
        for (code, dx, dy) in [
            (71, -s, -s),
            (72, 0.0, -s),
            (73, s, -s),
            (75, -s, 0.0),
            (77, s, 0.0),
            (79, -s, s),
            (80, 0.0, s),
            (81, s, s),
        ] {
            assert_eq!(
                action(code, KeyState::Pressed),
                Some(MouseAction::Move(dx, dy))
            );
            assert_eq!(
                action(code, KeyState::Repeated),
                Some(MouseAction::Move(dx, dy)),
                "held directions repeat"
            );
            assert_eq!(action(code, KeyState::Released), None);
        }
    }

    #[test]
    fn the_click_keys_and_hold_release() {
        assert_eq!(
            action(76, KeyState::Pressed),
            Some(MouseAction::Click(BTN_PRIMARY))
        );
        assert_eq!(
            action(74, KeyState::Pressed),
            Some(MouseAction::Click(BTN_SECONDARY))
        );
        assert_eq!(
            action(55, KeyState::Pressed),
            Some(MouseAction::Click(BTN_MIDDLE))
        );
        assert_eq!(action(78, KeyState::Pressed), Some(MouseAction::DoubleClick));
        assert_eq!(action(82, KeyState::Pressed), Some(MouseAction::Hold));
        assert_eq!(action(82, KeyState::Released), Some(MouseAction::Release));
        assert_eq!(action(83, KeyState::Pressed), Some(MouseAction::Release));
        // Clicks do not fire on repeat or release.
        assert_eq!(action(76, KeyState::Repeated), None);
        assert_eq!(action(76, KeyState::Released), None);
    }

    #[test]
    fn other_keys_are_not_mouse_keys() {
        for code in [30, 2, 57, 69, 96, 98] {
            assert_eq!(action(code, KeyState::Pressed), None, "code {code}");
        }
    }
}
