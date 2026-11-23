//! Input routing: pointer focus and grabs, keyboard focus and shortcuts,
//! touch contacts. A client only ever gets events for its own surfaces,
//! with coordinates relative to them.

use super::{ClientId, Placed, State, SurfaceKey};
use crate::decor::{self, Button, Part};
use crate::geom::Rect;

use ultar_hal::{InputEvent, InputEventKind, PointerButton};
use ultar_surface::proto::{Event, Modifiers, FIXED_ONE};

/// The primary button's Linux code.
pub const BTN_PRIMARY: u16 = 0x110;
/// Points the pointer must travel before a title press becomes a move.
const DRAG_START: f64 = 4.0;

/// What the pointer is doing with a button held.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PointerGrab {
    /// Pressed on content: events go to that surface until release.
    Implicit(SurfaceKey),
    /// Pressed on a title bar, not moved far yet.
    TitlePress {
        /// The window.
        key: SurfaceKey,
        /// Where the press happened.
        start: (f64, f64),
    },
    /// Moving a window; offset from pointer to frame origin.
    Move {
        /// The window.
        key: SurfaceKey,
        /// Offset.
        offset: (f64, f64),
    },
    /// Resizing a window.
    Resize {
        /// The window.
        key: SurfaceKey,
        /// Edges (bits of `ResizeEdges`).
        edges: u8,
        /// Pointer at the start.
        start: (f64, f64),
        /// Frame at the start.
        frame: Rect,
    },
    /// Pressed on a title bar button.
    Button {
        /// The window.
        key: SurfaceKey,
        /// The button.
        button: Button,
    },
    /// Dragging data offered by a client.
    Drag(ClientId),
}

impl PointerGrab {
    /// The client the grab belongs to.
    pub fn client(&self) -> Option<ClientId> {
        Some(match self {
            PointerGrab::Implicit(k)
            | PointerGrab::TitlePress { key: k, .. }
            | PointerGrab::Move { key: k, .. }
            | PointerGrab::Resize { key: k, .. }
            | PointerGrab::Button { key: k, .. } => k.client,
            PointerGrab::Drag(c) => *c,
        })
    }
}

/// The surface under the pointer and what part of it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerFocus {
    /// The surface.
    pub key: SurfaceKey,
    /// Content or decoration part.
    pub part: Part,
}

/// Pointer state.
#[derive(Clone, Debug, Default)]
pub struct Pointer {
    /// Position (logical points).
    pub x: f64,
    /// Position (logical points).
    pub y: f64,
    /// What is under it.
    pub focus: Option<PointerFocus>,
    /// Buttons held.
    pub held: u8,
    /// Active grab.
    pub grab: Option<PointerGrab>,
    /// Last title bar press (for double click).
    pub(super) last_title: Option<(SurfaceKey, u64)>,
    /// Where a dragged window would snap.
    pub snap_preview: Option<Rect>,
    /// Last input was touch (pointer hidden).
    pub hidden: bool,
    /// Touch contacts and the surface each one belongs to.
    pub(super) touches: std::collections::HashMap<u32, SurfaceKey>,
}

pub(super) fn fixed(v: f64) -> i32 {
    (v * f64::from(FIXED_ONE))
        .round()
        .clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32
}

fn button_code(b: PointerButton) -> u16 {
    match b {
        PointerButton::Primary => BTN_PRIMARY,
        PointerButton::Secondary => 0x111,
        PointerButton::Middle => 0x112,
        PointerButton::Other(c) => c,
    }
}

impl State {
    /// Routes one input event.
    pub fn input(&mut self, event: &InputEvent) {
        match event.kind {
            InputEventKind::PointerMotion { dx, dy } => {
                let (x, y) = (
                    self.pointer.x + f64::from(dx),
                    self.pointer.y + f64::from(dy),
                );
                self.pointer_to(x, y);
            }
            InputEventKind::PointerAbsolute { x, y } => {
                let (x, y) = self.absolute(x, y);
                self.pointer_to(x, y);
            }
            InputEventKind::PointerButton { button, pressed } => {
                self.button(button_code(button), pressed)
            }
            InputEventKind::Scroll { dx, dy } => self.scroll(f64::from(dx), f64::from(dy)),
            InputEventKind::Key { code, state } => self.key(code, state),
            InputEventKind::TouchDown { slot, x, y } => self.touch_down(u32::from(slot), x, y),
            InputEventKind::TouchMove { slot, x, y } => self.touch_move(u32::from(slot), x, y),
            InputEventKind::TouchUp { slot } => self.touch_up(u32::from(slot)),
            InputEventKind::TouchCancel => self.touch_cancel(),
            InputEventKind::DeviceAdded | InputEventKind::DeviceRemoved => {}
        }
    }

    /// Maps a normalised absolute position onto the primary output.
    pub(super) fn absolute(&self, x: u16, y: u16) -> (f64, f64) {
        let r = self
            .layout
            .primary()
            .map_or(Rect::new(0, 0, 1, 1), |o| o.logical_rect());
        (
            f64::from(r.x) + f64::from(x) * f64::from(r.w) / 65536.0,
            f64::from(r.y) + f64::from(y) * f64::from(r.h) / 65536.0,
        )
    }

    /// The topmost surface at a logical point and the part hit. Fully
    /// transparent pixels are not hit (they belong to what is below);
    /// nothing below a system dialog is hit (it is modal).
    pub fn hit_test(&self, x: f64, y: f64) -> Option<(SurfaceKey, Part, Rect)> {
        let (px, py) = (x.floor() as i32, y.floor() as i32);
        let rtl = self.rtl();
        for placed in self.scene().iter().rev() {
            if let Some(i) = placed.window {
                let w = &self.windows[i];
                let f = w.frame;
                let resizable = (w.floating() || w.snap.is_some()) && !w.secure;
                let part = decor::hit(f.w, f.h, px - f.x, py - f.y, w.decorated(), resizable, rtl);
                let part = match part {
                    Some(Part::Button(b)) if w.secure && b != Button::Close => Some(Part::Title),
                    other => other,
                };
                if let Some(part) = part {
                    if part != Part::Content || self.solid_at(placed, x, y) {
                        return Some((w.key, part, placed.content));
                    }
                }
                if w.secure {
                    return None;
                }
            } else if placed.content.contains(px, py) && self.solid_at(placed, x, y) {
                return Some((placed.key, Part::Content, placed.content));
            }
        }
        None
    }

    /// False where the surface's pixel at a logical point is fully
    /// transparent.
    fn solid_at(&self, placed: &Placed, x: f64, y: f64) -> bool {
        let Some(s) = self.surface(placed.key) else {
            return false;
        };
        if s.opaque {
            return true;
        }
        let Some(img) = s.image.as_ref() else {
            return false;
        };
        let c = placed.content;
        let ix = (x - f64::from(c.x)) * f64::from(img.width()) / f64::from(c.w.max(1));
        let iy = (y - f64::from(c.y)) * f64::from(img.height()) / f64::from(c.h.max(1));
        img.pixel(ix.floor() as i32, iy.floor() as i32)
            .is_some_and(|p| p >> 24 != 0)
    }

    fn surface_point(&self, key: SurfaceKey) -> Option<(i32, i32)> {
        let content = match self.window_index(key) {
            Some(i) => self.windows[i].content(),
            None => self.layer_rect(key)?,
        };
        Some((
            fixed(self.pointer.x - f64::from(content.x)),
            fixed(self.pointer.y - f64::from(content.y)),
        ))
    }

    pub(super) fn damage_cursor(&mut self) {
        if self.config.cursor && !self.pointer.hidden {
            let r = crate::render::cursor_rect(self.pointer.x, self.pointer.y);
            self.damage_logical(r);
        }
    }

    /// Moves the pointer to a logical position (clamped to the outputs).
    pub fn pointer_to(&mut self, x: f64, y: f64) {
        let b = self.layout.bounds();
        self.damage_cursor();
        self.pointer.hidden = false;
        self.pointer.x = x.clamp(f64::from(b.x), f64::from(b.max_x() - 1).max(f64::from(b.x)));
        self.pointer.y = y.clamp(f64::from(b.y), f64::from(b.max_y() - 1).max(f64::from(b.y)));
        self.damage_cursor();
        match self.pointer.grab {
            Some(PointerGrab::Implicit(key)) => self.send_motion(key),
            Some(PointerGrab::TitlePress { key, start }) => {
                let moved = (self.pointer.x - start.0).hypot(self.pointer.y - start.1);
                if moved >= DRAG_START {
                    self.begin_move_at(key, Some(start));
                }
            }
            Some(PointerGrab::Move { key, offset }) => self.drag_window(key, offset),
            Some(PointerGrab::Resize {
                key,
                edges,
                start,
                frame,
            }) => self.drag_resize(key, edges, start, frame),
            Some(PointerGrab::Button { key, button }) => self.hover_button(key, Some(button)),
            Some(PointerGrab::Drag(_)) => {}
            None => self.update_hover(),
        }
    }

    fn send_motion(&mut self, key: SurfaceKey) {
        if let Some((x, y)) = self.surface_point(key) {
            self.send(
                key.client,
                Event::PointerMotion {
                    surface: key.surface,
                    x,
                    y,
                },
            );
        }
    }

    pub(super) fn update_hover(&mut self) {
        let hit = self.hit_test(self.pointer.x, self.pointer.y);
        let new = hit.map(|(key, part, _)| super::input::PointerFocus { key, part });
        let old = self.pointer.focus;
        let old_content = old.filter(|f| f.part == Part::Content).map(|f| f.key);
        let new_content = new.filter(|f| f.part == Part::Content).map(|f| f.key);
        if old_content != new_content {
            if let Some(k) = old_content {
                self.send(k.client, Event::PointerLeave { surface: k.surface });
            }
            if let Some(k) = new_content {
                if let Some((x, y)) = self.surface_point(k) {
                    self.send(
                        k.client,
                        Event::PointerEnter {
                            surface: k.surface,
                            x,
                            y,
                        },
                    );
                }
            }
        } else if let Some(k) = new_content {
            self.send_motion(k);
        }
        let hover = |f: Option<PointerFocus>| match f.map(|f| f.part) {
            Some(Part::Button(b)) => f.map(|f| (f.key, b)),
            _ => None,
        };
        if hover(old) != hover(new) {
            if let Some((k, _)) = hover(old) {
                self.hover_button(k, None);
            }
            if let Some((k, b)) = hover(new) {
                self.hover_button(k, Some(b));
            }
        }
        self.pointer.focus = new;
    }

    pub(super) fn hover_button(&mut self, key: SurfaceKey, button: Option<Button>) {
        let over = self.hit_test(self.pointer.x, self.pointer.y);
        let Some(i) = self.window_index(key) else {
            return;
        };
        let shown = match (button, over) {
            (Some(b), Some((k, Part::Button(h), _))) if k == key && h == b => Some(b),
            _ => None,
        };
        if self.windows[i].hover != shown {
            self.windows[i].hover = shown;
            let f = self.windows[i].frame;
            self.damage_logical(Rect::new(f.x, f.y, f.w, decor::TITLEBAR));
        }
    }

    /// Records a user action serial for a client and pings it if needed.
    pub(super) fn user_serial(&mut self, client: ClientId) -> u32 {
        let serial = self.next_serial();
        let now = self.now;
        let ping = self.next_serial();
        let mut send_ping = false;
        if let Some(c) = self.clients.get_mut(&client) {
            c.remember_serial(serial, now);
            if c.ping.is_none() {
                c.ping = Some((ping, now));
                send_ping = true;
            }
        }
        if send_ping {
            self.send(client, Event::Ping { serial: ping });
        }
        serial
    }

    pub(super) fn button(&mut self, code: u16, pressed: bool) {
        if pressed {
            self.pointer.held = self.pointer.held.saturating_add(1);
            if self.pointer.held == 1 && self.pointer.grab.is_none() {
                self.press(code);
                return;
            }
        } else {
            self.pointer.held = self.pointer.held.saturating_sub(1);
        }
        let grab = self.pointer.grab;
        if let Some(PointerGrab::Implicit(key)) = grab {
            let serial = self.user_serial(key.client);
            self.send(
                key.client,
                Event::PointerButton {
                    surface: key.surface,
                    button: code,
                    pressed,
                    serial,
                },
            );
        }
        if self.pointer.held == 0 {
            self.release(grab);
        }
    }

    fn press(&mut self, code: u16) {
        let Some((key, part, _)) = self.hit_test(self.pointer.x, self.pointer.y) else {
            return;
        };
        let on_top = self.windows.last().map(|w| w.key) == Some(key);
        if self.window_index(key).is_some() && (self.focus != Some(key) || !on_top) {
            self.focus_window(key);
        }
        let super_held = self.keyboard.modifiers & Modifiers::SUPER != 0;
        let grab = match part {
            Part::Content if super_held && self.window_index(key).is_some() => {
                self.begin_move(key);
                return;
            }
            Part::Content => {
                let serial = self.user_serial(key.client);
                self.send(
                    key.client,
                    Event::PointerButton {
                        surface: key.surface,
                        button: code,
                        pressed: true,
                        serial,
                    },
                );
                PointerGrab::Implicit(key)
            }
            Part::Title if code == BTN_PRIMARY => {
                if self.title_double_click(key) {
                    return;
                }
                PointerGrab::TitlePress {
                    key,
                    start: (self.pointer.x, self.pointer.y),
                }
            }
            Part::Button(button) if code == BTN_PRIMARY => {
                self.hover_button(key, Some(button));
                PointerGrab::Button { key, button }
            }
            Part::Border(edges) if code == BTN_PRIMARY => self.resize_grab(key, edges),
            _ => return,
        };
        self.pointer.grab = Some(grab);
    }

    fn scroll(&mut self, dx: f64, dy: f64) {
        let target = match self.pointer.grab {
            Some(PointerGrab::Implicit(k)) => Some(k),
            _ => self
                .pointer
                .focus
                .filter(|f| f.part == Part::Content)
                .map(|f| f.key),
        };
        if let Some(k) = target {
            let line = f64::from(ultar_ui::tokens::size::SCROLL_LINE);
            let event = Event::PointerAxis {
                surface: k.surface,
                dx: fixed(dx * line),
                dy: fixed(dy * line),
            };
            self.send(k.client, event);
        }
    }
}
