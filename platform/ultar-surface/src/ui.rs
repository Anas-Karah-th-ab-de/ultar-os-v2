//! Runs an `ultar-ui` widget tree in a compositor window: the app process
//! keeps its own `Ui`, this type turns compositor events into toolkit
//! events and the toolkit's damaged pixels into commits.

use crate::client::{Client, ClientError, Window, WindowConfig};
use crate::proto::{Configure, Event, Modifiers as WireModifiers, Rect, FIXED_ONE};
use std::time::{Duration, Instant};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Action, Event as UiEvent, Key, Modifiers, PointerButton};
use ultar_ui::ui::Ui;
use ultar_ui::{Direction, Point, Size};

/// Most unused key presses kept between two [`UiWindow::take_unhandled_keys`] calls.
pub const MAX_UNHANDLED: usize = 64;

/// An `ultar-ui` tree shown in one window.
pub struct UiWindow {
    client: Client,
    window: Window,
    ui: Ui,
    canvas: Option<Canvas>,
    modifiers: Modifiers,
    pointer: Point,
    started: Instant,
    /// Key presses the widget tree did not use (app shortcuts), oldest first.
    unhandled: Vec<UiEvent>,
    /// The last input was a key press (not a pointer or touch).
    key_input: bool,
    /// Keys that go to the app before the widget tree sees them.
    shortcuts: Option<fn(&UiEvent) -> bool>,
}

impl std::fmt::Debug for UiWindow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UiWindow")
            .field("window", &self.window)
            .finish()
    }
}

/// Linux key code → toolkit key (keys without a character).
fn named_key(code: u16) -> Option<Key> {
    Some(match code {
        15 => Key::Tab,
        28 | 96 => Key::Enter,
        57 => Key::Space,
        1 => Key::Escape,
        105 => Key::ArrowLeft,  // physical arrow key
        106 => Key::ArrowRight, // physical arrow key
        103 => Key::ArrowUp,
        108 => Key::ArrowDown,
        102 => Key::Home,
        107 => Key::End,
        14 => Key::Backspace,
        111 => Key::Delete,
        104 => Key::PageUp,
        109 => Key::PageDown,
        127 => Key::Menu,
        _ => return None,
    })
}

fn modifiers(m: WireModifiers) -> Modifiers {
    Modifiers {
        shift: m.has(WireModifiers::SHIFT),
        ctrl: m.has(WireModifiers::CTRL),
        alt: m.has(WireModifiers::ALT),
        meta: m.has(WireModifiers::SUPER),
    }
}

fn point(x: i32, y: i32) -> Point {
    Point::new(x as f32 / FIXED_ONE as f32, y as f32 / FIXED_ONE as f32)
}

fn button(code: u16) -> Option<PointerButton> {
    match code {
        0x110 => Some(PointerButton::Primary),
        0x111 => Some(PointerButton::Secondary),
        0x112 => Some(PointerButton::Middle),
        _ => None,
    }
}

/// Turns a compositor event into a toolkit event (None for events the
/// toolkit does not take).
pub fn translate(event: &Event, last_pointer: Point) -> Option<UiEvent> {
    Some(match *event {
        Event::PointerEnter { x, y, .. } | Event::PointerMotion { x, y, .. } => {
            UiEvent::PointerMove { pos: point(x, y) }
        }
        Event::PointerLeave { .. } => UiEvent::PointerLeave,
        Event::PointerButton {
            button: b, pressed, ..
        } => {
            let button = button(b)?;
            if pressed {
                UiEvent::PointerDown {
                    pos: last_pointer,
                    button,
                }
            } else {
                UiEvent::PointerUp {
                    pos: last_pointer,
                    button,
                }
            }
        }
        Event::PointerAxis { dy, .. } => UiEvent::Scroll {
            pos: last_pointer,
            lines: dy as f32 / FIXED_ONE as f32 / ultar_ui::tokens::size::SCROLL_LINE,
        },
        Event::Key {
            code,
            state,
            modifiers: m,
            character,
            ..
        } if state != 0 => {
            let key = named_key(code).or_else(|| {
                char::from_u32(character)
                    .filter(|c| !c.is_control())
                    .map(Key::Char)
            })?;
            UiEvent::Key {
                key,
                modifiers: modifiers(m),
            }
        }
        Event::TouchDown { id, x, y, .. } => UiEvent::TouchDown {
            id: u64::from(id),
            pos: point(x, y),
        },
        Event::TouchMove { id, x, y, .. } => UiEvent::TouchMove {
            id: u64::from(id),
            pos: point(x, y),
        },
        Event::TouchUp { id, .. } => UiEvent::TouchUp {
            id: u64::from(id),
            pos: last_pointer,
        },
        Event::TouchCancel { .. } => UiEvent::TouchCancel { id: 0 },
        _ => return None,
    })
}

impl UiWindow {
    /// Creates the window for `ui` and waits for its first configure.
    pub fn new(mut client: Client, config: &WindowConfig, ui: Ui) -> Result<UiWindow, ClientError> {
        let window = Window::create(&mut client, config)?;
        let mut this = UiWindow {
            client,
            window,
            ui,
            canvas: None,
            modifiers: Modifiers::default(),
            pointer: Point::new(0.0, 0.0),
            started: Instant::now(),
            unhandled: Vec::new(),
            key_input: false,
            shortcuts: None,
        };
        while this.window.configure().is_none() {
            this.dispatch(Some(super::client::WELCOME_TIMEOUT))?;
        }
        Ok(this)
    }

    /// The widget tree.
    pub fn ui_mut(&mut self) -> &mut Ui {
        &mut self.ui
    }

    /// The connection.
    pub fn client_mut(&mut self) -> &mut Client {
        &mut self.client
    }

    /// The window.
    pub fn window(&self) -> &Window {
        &self.window
    }

    /// Key presses the widget tree did not use since the last call (the app's shortcuts:
    /// Ctrl+C, Delete, F2 …), oldest first.
    pub fn take_unhandled_keys(&mut self) -> Vec<UiEvent> {
        std::mem::take(&mut self.unhandled)
    }

    /// Key presses for which `filter` is true go straight to the app (read with
    /// [`UiWindow::take_unhandled_keys`]), before the widget tree moves focus with them
    /// (Alt+arrows for back and forward).
    pub fn set_shortcuts(&mut self, filter: Option<fn(&UiEvent) -> bool>) {
        self.shortcuts = filter;
    }

    /// `true` when the last input event was a key press: an app tells "Enter on an item"
    /// from "a click on an item" with it.
    pub fn last_input_was_key(&self) -> bool {
        self.key_input
    }

    /// Waits up to `timeout` for one event, handles it and draws if the UI
    /// changed and the compositor is ready for a frame. Returns the actions
    /// of the widget tree. Times out early when an animation needs a tick.
    pub fn dispatch(&mut self, timeout: Option<Duration>) -> Result<Vec<Action>, ClientError> {
        let now_ms = self.started.elapsed().as_millis() as u64;
        let wake = self
            .ui
            .next_wake()
            .map(|w| Duration::from_millis(w.saturating_sub(now_ms)));
        let wait = match (timeout, wake) {
            (Some(t), Some(w)) => Some(t.min(w)),
            (t, w) => t.or(w),
        };
        let mut actions = Vec::new();
        if let Some(event) = self.client.next_event(wait)? {
            actions = self.handle(&event)?;
        }
        self.ui
            .advance_to(self.started.elapsed().as_millis() as u64);
        actions.extend(self.ui.take_actions());
        self.draw()?;
        Ok(actions)
    }

    fn handle(&mut self, event: &Event) -> Result<Vec<Action>, ClientError> {
        if self.window.handle(event) {
            if let Event::Configure(c) = event {
                self.configure(c)?;
            }
            return Ok(Vec::new());
        }
        if let Event::Key { modifiers: m, .. } = event {
            self.modifiers = modifiers(*m);
        }
        if let Event::PointerEnter { x, y, .. } | Event::PointerMotion { x, y, .. } = event {
            self.pointer = point(*x, *y);
        }
        match translate(event, self.pointer) {
            Some(ui_event) => {
                if self.shortcuts.is_some_and(|f| f(&ui_event))
                    && self.unhandled.len() < MAX_UNHANDLED
                {
                    self.key_input = true;
                    self.unhandled.push(ui_event);
                    return Ok(Vec::new());
                }
                if !matches!(ui_event, UiEvent::Modifiers { .. }) {
                    self.key_input = matches!(ui_event, UiEvent::Key { .. });
                }
                let response = self.ui.handle(ui_event);
                if !response.consumed && matches!(ui_event, UiEvent::Key { .. }) {
                    // An app reads them with `take_unhandled_keys`; a bounded queue.
                    if self.unhandled.len() < MAX_UNHANDLED {
                        self.unhandled.push(ui_event);
                    }
                }
                Ok(response.actions)
            }
            None => Ok(Vec::new()),
        }
    }

    fn configure(&mut self, c: &Configure) -> Result<(), ClientError> {
        let scale = c.scale_milli as f32 / 1000.0;
        self.ui
            .set_scale(scale)
            .map_err(|_| ClientError::InvalidSize)?;
        self.ui
            .set_size(Size::new(c.width as f32, c.height as f32))
            .map_err(|_| ClientError::InvalidSize)?;
        self.ui.set_direction(if c.rtl {
            Direction::Rtl
        } else {
            Direction::Ltr
        });
        let (w, h) = device_size(c);
        let keep = self
            .canvas
            .as_ref()
            .is_some_and(|k| k.width() == w && k.height() == h);
        if !keep {
            self.canvas =
                Some(Canvas::new(w, h, PixelFormat::Bgrx8).map_err(|_| ClientError::InvalidSize)?);
        }
        Ok(())
    }

    /// Paints and presents if needed.
    pub fn draw(&mut self) -> Result<(), ClientError> {
        let Some(canvas) = self.canvas.as_mut() else {
            return Ok(());
        };
        if !self.window.can_draw() || !self.ui.needs_paint() {
            return Ok(());
        }
        self.ui.paint(canvas);
        let damage = canvas.take_damage();
        let rects: Vec<Rect> = damage
            .rects()
            .iter()
            .map(|r| {
                Rect::new(
                    r.x0,
                    r.y0,
                    (r.x1 - r.x0).max(0) as u32,
                    (r.y1 - r.y0).max(0) as u32,
                )
            })
            .filter(|r| r.width > 0 && r.height > 0)
            .collect();
        let size = (canvas.width(), canvas.height());
        self.window
            .present(&mut self.client, canvas.data(), size, &rects)
    }
}

/// Device pixel size of a configure.
pub fn device_size(c: &Configure) -> (u32, u32) {
    let scale = f64::from(c.scale_milli) / 1000.0;
    let w = (f64::from(c.width) * scale).round().max(1.0) as u32;
    let h = (f64::from(c.height) * scale).round().max(1.0) as u32;
    (w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::{SurfaceId, WindowState};

    #[test]
    fn events_translate_to_toolkit_events() {
        let s = SurfaceId(1);
        let p = Point::new(1.0, 2.0);
        assert_eq!(
            translate(
                &Event::PointerMotion {
                    surface: s,
                    x: 512,
                    y: 256
                },
                p
            ),
            Some(UiEvent::PointerMove {
                pos: Point::new(2.0, 1.0)
            })
        );
        assert_eq!(
            translate(
                &Event::PointerButton {
                    surface: s,
                    button: 0x110,
                    pressed: true,
                    serial: 1
                },
                p
            ),
            Some(UiEvent::PointerDown {
                pos: p,
                button: PointerButton::Primary
            })
        );
        assert_eq!(
            translate(
                &Event::PointerButton {
                    surface: s,
                    button: 0x115,
                    pressed: true,
                    serial: 1
                },
                p
            ),
            None
        );
    }

    #[test]
    fn keys_touch_and_sizes_translate() {
        let s = SurfaceId(1);
        let p = Point::new(1.0, 2.0);
        let key = |code, character, state| Event::Key {
            surface: s,
            code,
            state,
            modifiers: WireModifiers(WireModifiers::CTRL),
            character,
            serial: 1,
        };
        assert!(
            matches!(translate(&key(15, 9, 1), p), Some(UiEvent::Key { key: Key::Tab, modifiers }) if modifiers.ctrl)
        );
        assert!(matches!(
            translate(&key(30, 'ش' as u32, 1), p),
            Some(UiEvent::Key {
                key: Key::Char('ش'),
                ..
            })
        ));
        assert_eq!(translate(&key(30, 'a' as u32, 0), p), None);
        assert_eq!(translate(&key(29, 0, 1), p), None);
        assert!(
            matches!(translate(&Event::PointerAxis { surface: s, dx: 0, dy: 48 * 256 }, p), Some(UiEvent::Scroll { lines, .. }) if (lines - 1.0).abs() < 1e-6)
        );
        assert_eq!(translate(&Event::Ping { serial: 1 }, p), None);
        assert!(translate(
            &Event::TouchDown {
                surface: s,
                id: 3,
                x: 0,
                y: 0,
                serial: 1
            },
            p
        )
        .is_some());
        let c = Configure {
            surface: s,
            serial: 1,
            width: 101,
            height: 50,
            scale_milli: 1250,
            state: WindowState::Normal,
            activated: true,
            rtl: true,
        };
        assert_eq!(device_size(&c), (126, 63));
    }
}
