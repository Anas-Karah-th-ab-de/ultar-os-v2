//! Keyboard (layouts, shortcuts of DESKTOP-SPEC §5.4) and touch routing.

use super::input::{fixed, BTN_PRIMARY};
use super::{State, SurfaceKey};
use crate::decor::Part;
use crate::keymap::{self, Output as KeyOutput};
use crate::wm::{self, Arrow};
use ultar_hal::KeyState;
use ultar_surface::proto::{Event, Modifiers, Role};

impl State {
    pub(super) fn key(&mut self, code: u16, state: KeyState) {
        let pressed = state != KeyState::Released;
        if let Some(bit) = keymap::modifier_bit(code) {
            if pressed {
                self.keyboard.modifiers |= bit;
            } else {
                self.keyboard.modifiers &= !bit;
            }
        }
        if code == 58 && state == KeyState::Pressed {
            self.keyboard.caps_lock = !self.keyboard.caps_lock;
        }
        if state == KeyState::Pressed && self.shortcut(code) {
            return;
        }
        let Some(key) = self.key_target() else { return };
        let modifiers = Modifiers(self.keyboard.modifiers);
        let output = if pressed {
            keymap::translate(self.key_layout(), code, modifiers, self.keyboard.caps_lock)
        } else {
            KeyOutput::None
        };
        let serial = if state == KeyState::Pressed {
            self.user_serial(key.client)
        } else {
            self.next_serial()
        };
        let state_code = match state {
            KeyState::Released => 0,
            KeyState::Pressed => 1,
            KeyState::Repeated => 2,
        };
        self.deliver_key(key, code, state_code, modifiers, (output, serial));
    }

    /// The surface keys go to: the lock screen while locked, else the focus.
    fn key_target(&self) -> Option<SurfaceKey> {
        if self.locked() {
            self.layers
                .iter()
                .copied()
                .find(|k| self.surface(*k).is_some_and(|s| s.role == Role::Lock))
        } else {
            self.focus
        }
    }

    fn deliver_key(
        &mut self,
        key: SurfaceKey,
        code: u16,
        state_code: u8,
        modifiers: Modifiers,
        (output, serial): (KeyOutput, u32),
    ) {
        let character = match output {
            KeyOutput::Char(c) => u32::from(c),
            _ => 0,
        };
        self.send(
            key.client,
            Event::Key {
                surface: key.surface,
                code,
                state: state_code,
                modifiers,
                character,
                serial,
            },
        );
        if let KeyOutput::Text(text) = output {
            self.send(
                key.client,
                Event::Text {
                    surface: key.surface,
                    text: text.to_owned(),
                },
            );
        }
    }

    /// Compositor shortcuts (DESKTOP-SPEC §5.4). True if the key was used.
    fn shortcut(&mut self, code: u16) -> bool {
        let m = Modifiers(self.keyboard.modifiers);
        if self.locked() {
            return false;
        }
        let focused = self.focus.and_then(|k| self.window_index(k));
        if m.has(Modifiers::SUPER) {
            let arrow = match code {
                103 => Some(Arrow::Up),
                108 => Some(Arrow::Down),
                105 => Some(Arrow::Left),  // physical
                106 => Some(Arrow::Right), // physical
                _ => None,
            };
            if let (Some(arrow), Some(i)) = (arrow, focused) {
                let (state, snap) =
                    wm::keyboard_snap(self.windows[i].state, self.windows[i].snap, arrow);
                self.set_window_state(i, state, snap);
                return true;
            }
            if code == 57 && !self.config.layouts.is_empty() {
                self.keyboard.layout_index =
                    (self.keyboard.layout_index + 1) % self.config.layouts.len();
                return true;
            }
        }
        if m.has(Modifiers::ALT) && code == 62 {
            if let Some(k) = self.focus.filter(|k| self.window_index(*k).is_some()) {
                self.request_close(k);
            }
            return true;
        }
        if m.has(Modifiers::ALT) && code == 15 {
            self.cycle_windows();
            return true;
        }
        false
    }

    /// Alt+Tab: brings the window below the top one forward (restoring it
    /// if minimised).
    fn cycle_windows(&mut self) {
        let focus = self.focus;
        let Some(index) = self
            .windows
            .iter()
            .rposition(|w| Some(w.key) != focus && w.mapped)
        else {
            return;
        };
        let key = self.windows[index].key;
        self.unminimize(index);
        self.focus_window(key);
    }

    pub(super) fn touch_down(&mut self, id: u32, x: u16, y: u16) {
        let (lx, ly) = self.absolute(x, y);
        let Some((key, part, content)) = self.hit_test(lx, ly) else {
            return;
        };
        if part != Part::Content {
            // Decorations by touch behave like the primary button.
            self.pointer_to(lx, ly);
            self.pointer.hidden = true;
            self.button(BTN_PRIMARY, true);
            self.touches.insert(
                id,
                SurfaceKey {
                    client: u32::MAX,
                    surface: key.surface,
                },
            );
            return;
        }
        self.damage_cursor();
        self.pointer.hidden = true;
        if self.window_index(key).is_some() {
            self.focus_window(key);
        }
        self.touches.insert(id, key);
        let serial = self.user_serial(key.client);
        let (sx, sy) = (
            fixed(lx - f64::from(content.x)),
            fixed(ly - f64::from(content.y)),
        );
        self.send(
            key.client,
            Event::TouchDown {
                surface: key.surface,
                id,
                x: sx,
                y: sy,
                serial,
            },
        );
    }

    pub(super) fn touch_move(&mut self, id: u32, x: u16, y: u16) {
        let (lx, ly) = self.absolute(x, y);
        let Some(&key) = self.touches.get(&id) else {
            return;
        };
        if key.client == u32::MAX {
            self.pointer_to(lx, ly);
            self.pointer.hidden = true;
            return;
        }
        let content = match self.window_index(key) {
            Some(i) => self.windows[i].content(),
            None => match self.layer_rect(key) {
                Some(r) => r,
                None => return,
            },
        };
        let (sx, sy) = (
            fixed(lx - f64::from(content.x)),
            fixed(ly - f64::from(content.y)),
        );
        self.send(
            key.client,
            Event::TouchMove {
                surface: key.surface,
                id,
                x: sx,
                y: sy,
            },
        );
    }

    pub(super) fn touch_up(&mut self, id: u32) {
        let Some(key) = self.touches.remove(&id) else {
            return;
        };
        if key.client == u32::MAX {
            self.button(BTN_PRIMARY, false);
            return;
        }
        self.send(
            key.client,
            Event::TouchUp {
                surface: key.surface,
                id,
            },
        );
    }

    pub(super) fn touch_cancel(&mut self) {
        let keys: Vec<SurfaceKey> = self.touches.drain().map(|(_, k)| k).collect();
        let mut done = Vec::new();
        for key in keys {
            if key.client == u32::MAX {
                self.button(BTN_PRIMARY, false);
            } else if !done.contains(&key) {
                done.push(key);
                self.send(
                    key.client,
                    Event::TouchCancel {
                        surface: key.surface,
                    },
                );
            }
        }
    }
}
