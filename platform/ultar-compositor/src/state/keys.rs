//! Keyboard (layouts, shortcuts of DESKTOP-SPEC §5.4) and touch routing.

use super::input::{fixed, BTN_PRIMARY};
use super::{State, SurfaceKey};
use crate::decor::Part;
use crate::keymap::{self, Output as KeyOutput};
use crate::wm::{self, Arrow};
use ultar_hal::KeyState;
use ultar_surface::proto::{Event, Modifiers};

impl State {
    pub(super) fn key(&mut self, code: u16, state: KeyState) {
        // The compose key takes the key (and its sequence) before anything else.
        if self.compose_event(code, state) {
            return;
        }
        let pressed = state != KeyState::Released;
        let bit = keymap::modifier_bit(code);
        // Sticky keys: a latched modifier applies to this key; a non-modifier press consumes it.
        let sticky = self.keyboard.helpers.sticky.on_key(bit, state);
        if let Some(bit) = bit {
            if pressed {
                self.keyboard.modifiers |= bit;
            } else {
                self.keyboard.modifiers &= !bit;
            }
        }
        let mods = self.keyboard.modifiers | sticky;
        if code == 58 && state == KeyState::Pressed {
            self.keyboard.caps_lock = !self.keyboard.caps_lock;
        }
        if self.secure_attention(code, state) {
            return;
        }
        if self.layout_keys(code, state) {
            self.session.shell.super_tap_used();
            return;
        }
        if state == KeyState::Pressed && self.shortcut(code, Modifiers(mods)) {
            self.session.shell.super_tap_used();
            return;
        }
        if self.shell_key(code, state) {
            return;
        }
        // Repeats and the release go where the press went (focus changes
        // and the lock do not split a key between two surfaces).
        let target = match state {
            KeyState::Pressed => self.text_target(),
            KeyState::Repeated => self.session.pressed.get(&code).copied(),
            KeyState::Released => self.session.pressed.remove(&code),
        };
        let Some(key) = target else { return };
        if state == KeyState::Pressed {
            self.session.pressed.insert(code, key);
        }
        let modifiers = Modifiers(mods | self.caps_bit());
        let output = if pressed {
            keymap::translate(self.key_layout(), code, Modifiers(mods), self.keyboard.caps_lock)
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

    pub(super) fn deliver_key(
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

    /// Layout switching (COMPOSITOR.md §7, DESKTOP-SPEC `shell.kbd`), also on the lock
    /// screen: Super+Space at once (the key is used); Ctrl+Shift and Alt+Shift (as on Windows
    /// and Ubuntu) when the second modifier of the pair is released and no other key was
    /// pressed meanwhile, so Ctrl+Shift+N and Alt+Shift+Tab stay shortcuts. The modifier keys
    /// themselves still reach the client. True if the key was used.
    fn layout_keys(&mut self, code: u16, state: KeyState) -> bool {
        let held = self.keyboard.modifiers & !Modifiers::ALT_GR;
        match keymap::modifier_bit(code) {
            Some(bit) if bit != Modifiers::ALT_GR && bit != Modifiers::SUPER => {
                if state == KeyState::Pressed {
                    let pair = [
                        Modifiers::SHIFT | Modifiers::CTRL,
                        Modifiers::SHIFT | Modifiers::ALT,
                    ];
                    self.keyboard.layout_chord = pair.contains(&held);
                } else if state == KeyState::Released
                    && std::mem::take(&mut self.keyboard.layout_chord)
                {
                    self.next_layout();
                }
                false
            }
            Some(_) => {
                self.keyboard.layout_chord = false;
                false
            }
            None => {
                self.keyboard.layout_chord = false;
                let only_super = held == Modifiers::SUPER;
                if code == 57 && only_super && state != KeyState::Released {
                    if state == KeyState::Pressed {
                        self.next_layout();
                    }
                    return true;
                }
                false
            }
        }
    }

    /// Makes the next layout of the list active (the bar and the lock screen hear of it).
    pub(super) fn next_layout(&mut self) {
        if !self.config.layouts.is_empty() {
            self.keyboard.layout_index =
                (self.keyboard.layout_index + 1) % self.config.layouts.len();
        }
    }

    /// Compositor shortcuts (DESKTOP-SPEC §5.4), with the modifiers in effect (sticky keys
    /// included). True if the key was used.
    fn shortcut(&mut self, code: u16, m: Modifiers) -> bool {
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
            .rposition(|w| Some(w.key) != focus && w.mapped && w.place.shown)
        else {
            return;
        };
        let key = self.windows[index].key;
        self.unminimize(index);
        self.user_focus(key);
    }

    pub(super) fn touch_down(&mut self, id: u32, x: u16, y: u16) {
        let (lx, ly) = self.absolute(x, y);
        if self.edge_begin(super::phone::Contact::Touch(id), lx, ly) {
            return;
        }
        let hit = self.hit_test(lx, ly);
        self.shell_press(hit.map(|h| h.0));
        let Some((key, part, content)) = hit else {
            return;
        };
        if part != Part::Content {
            // Decorations by touch behave like the primary button.
            self.pointer_to(lx, ly);
            self.pointer.hidden = true;
            self.button(BTN_PRIMARY, true);
            self.pointer.touches.insert(
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
            self.user_focus(key);
        }
        self.pointer.touches.insert(id, key);
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
        if self.edge_move(super::phone::Contact::Touch(id), lx, ly) {
            return;
        }
        let Some(&key) = self.pointer.touches.get(&id) else {
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
        if self.edge_end(super::phone::Contact::Touch(id)) {
            return;
        }
        let Some(key) = self.pointer.touches.remove(&id) else {
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
        self.edge_cancel();
        let keys: Vec<SurfaceKey> = self.pointer.touches.drain().map(|(_, k)| k).collect();
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
