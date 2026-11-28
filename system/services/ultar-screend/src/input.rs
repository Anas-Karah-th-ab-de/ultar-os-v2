//! The viewer's pointer and keys on this device: translated to input events and handed to
//! virtual devices (`uinput`, kernel right `input_inject`). Keys and buttons that are down
//! are remembered so they are all released when the session ends, for whatever reason.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};
use ultar_bus::messages::mesh_view::{Button, RemoteInput};
use ultar_hal_linux::uinput::{self, Shape, VirtualInput};

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const REL_WHEEL: u16 = 8;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;

/// One `input_event`: type, code, value.
pub type Event = (u16, u16, i32);

/// A virtual device that takes input events.
pub trait Emit: Send {
    /// Sends `events` (they end with a `SYN_REPORT`).
    fn emit(&mut self, events: &[Event]);
}

/// What the screen service injects with.
pub trait Inject: Send {
    /// Carries out one event of the viewer.
    fn apply(&mut self, input: RemoteInput);
    /// Releases every key and button still held.
    fn release_all(&mut self);
}

impl Inject for Box<dyn Inject> {
    fn apply(&mut self, input: RemoteInput) {
        (**self).apply(input);
    }
    fn release_all(&mut self) {
        (**self).release_all();
    }
}

/// An injector that does nothing (no input interface on this system: view only).
pub struct ViewOnly;

impl Inject for ViewOnly {
    fn apply(&mut self, _input: RemoteInput) {}
    fn release_all(&mut self) {}
}

/// Events recorded for a test.
#[derive(Clone, Default)]
pub struct Recorder(pub Arc<Mutex<Vec<Event>>>);

impl Recorder {
    /// Everything emitted so far.
    pub fn events(&self) -> Vec<Event> {
        self.0.lock().map(|v| v.clone()).unwrap_or_default()
    }
}

impl Emit for Recorder {
    fn emit(&mut self, events: &[Event]) {
        if let Ok(mut all) = self.0.lock() {
            all.extend_from_slice(events);
        }
    }
}

/// A real virtual device.
pub struct Device(VirtualInput);

impl Emit for Device {
    fn emit(&mut self, events: &[Event]) {
        // A device that cannot be written to has nothing better to do than lose the event.
        let _ = self.0.send(events);
    }
}

const fn button_code(b: Button) -> u16 {
    match b {
        Button::Left => BTN_LEFT,
        Button::Right => BTN_RIGHT,
        Button::Middle => BTN_MIDDLE,
    }
}

/// Translates the viewer's events for two virtual devices: an absolute pointer and a keyboard.
pub struct Injector<P: Emit, K: Emit> {
    pointer: P,
    keyboard: K,
    size: (u32, u32),
    keys: BTreeSet<u16>,
    buttons: BTreeSet<u16>,
}

impl<P: Emit, K: Emit> Injector<P, K> {
    /// Injects into `pointer` (positions inside `size`) and `keyboard`.
    pub fn new(pointer: P, keyboard: K, size: (u32, u32)) -> Self {
        Self {
            pointer,
            keyboard,
            size,
            keys: BTreeSet::new(),
            buttons: BTreeSet::new(),
        }
    }

    fn place(&mut self, x: u32, y: u32) -> [Event; 2] {
        let (w, h) = self.size;
        let clamp = |v: u32, max: u32| i32::try_from(v.min(max.saturating_sub(1))).unwrap_or(0);
        [(EV_ABS, ABS_X, clamp(x, w)), (EV_ABS, ABS_Y, clamp(y, h))]
    }
}

impl<P: Emit, K: Emit> Inject for Injector<P, K> {
    fn apply(&mut self, input: RemoteInput) {
        match input {
            RemoteInput::Move { x, y } => {
                let [ax, ay] = self.place(x, y);
                self.pointer.emit(&[ax, ay, (EV_SYN, 0, 0)]);
            }
            RemoteInput::Button { x, y, button, down } => {
                let [ax, ay] = self.place(x, y);
                let code = button_code(button);
                if down {
                    self.buttons.insert(code);
                } else {
                    self.buttons.remove(&code);
                }
                self.pointer
                    .emit(&[ax, ay, (EV_KEY, code, i32::from(down)), (EV_SYN, 0, 0)]);
            }
            RemoteInput::Wheel { x, y, lines } => {
                let [ax, ay] = self.place(x, y);
                // Positive lines scroll further down: the wheel axis counts the other way.
                let notches = -i32::from(lines);
                self.pointer
                    .emit(&[ax, ay, (EV_REL, REL_WHEEL, notches), (EV_SYN, 0, 0)]);
            }
            RemoteInput::Key { code, down } => {
                if down {
                    self.keys.insert(code);
                } else {
                    self.keys.remove(&code);
                }
                self.keyboard
                    .emit(&[(EV_KEY, code, i32::from(down)), (EV_SYN, 0, 0)]);
            }
        }
    }

    fn release_all(&mut self) {
        for code in std::mem::take(&mut self.keys) {
            self.keyboard.emit(&[(EV_KEY, code, 0), (EV_SYN, 0, 0)]);
        }
        for code in std::mem::take(&mut self.buttons) {
            self.pointer.emit(&[(EV_KEY, code, 0), (EV_SYN, 0, 0)]);
        }
    }
}

/// The real injector below `root` for a screen of `size`; `None` when the system offers no
/// `uinput` or the devices cannot be created (the session is then view only).
pub fn open(root: &std::path::Path, size: (u32, u32)) -> Option<Injector<Device, Device>> {
    let node = uinput::node(root)?;
    let (w, h) = (u16::try_from(size.0).ok()?, u16::try_from(size.1).ok()?);
    let pointer = VirtualInput::create_at(
        &node,
        Shape::Tablet {
            width: w,
            height: h,
        },
        "Ultar remote pointer",
    )
    .ok()?;
    let keyboard = VirtualInput::create_at(&node, Shape::Keyboard, "Ultar remote keyboard").ok()?;
    Some(Injector::new(Device(pointer), Device(keyboard), size))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn rig() -> (Injector<Recorder, Recorder>, Recorder, Recorder) {
        let (p, k) = (Recorder::default(), Recorder::default());
        (Injector::new(p.clone(), k.clone(), (1920, 1080)), p, k)
    }

    #[test]
    fn the_pointer_is_placed_at_absolute_positions_inside_the_screen() {
        let (mut inj, p, _) = rig();
        inj.apply(RemoteInput::Move { x: 100, y: 200 });
        inj.apply(RemoteInput::Move { x: 5000, y: 7000 });
        assert_eq!(
            p.events(),
            vec![
                (EV_ABS, ABS_X, 100),
                (EV_ABS, ABS_Y, 200),
                (EV_SYN, 0, 0),
                (EV_ABS, ABS_X, 1919),
                (EV_ABS, ABS_Y, 1079),
                (EV_SYN, 0, 0),
            ]
        );
    }

    #[test]
    fn buttons_wheel_and_keys_arrive_as_the_right_events() {
        let (mut inj, p, k) = rig();
        inj.apply(RemoteInput::Button {
            x: 1,
            y: 2,
            button: Button::Right,
            down: true,
        });
        inj.apply(RemoteInput::Wheel {
            x: 1,
            y: 2,
            lines: 3,
        });
        inj.apply(RemoteInput::Key {
            code: 30,
            down: true,
        });
        let events = p.events();
        assert!(events.contains(&(EV_KEY, BTN_RIGHT, 1)));
        assert!(
            events.contains(&(EV_REL, REL_WHEEL, -3)),
            "down is a negative wheel"
        );
        assert_eq!(k.events(), vec![(EV_KEY, 30, 1), (EV_SYN, 0, 0)]);
    }

    #[test]
    fn everything_held_is_released_when_the_session_ends() {
        let (mut inj, p, k) = rig();
        inj.apply(RemoteInput::Key {
            code: 29,
            down: true,
        });
        inj.apply(RemoteInput::Key {
            code: 56,
            down: true,
        });
        inj.apply(RemoteInput::Key {
            code: 56,
            down: false,
        });
        inj.apply(RemoteInput::Button {
            x: 0,
            y: 0,
            button: Button::Left,
            down: true,
        });
        inj.release_all();
        assert!(k.events().contains(&(EV_KEY, 29, 0)), "ctrl released");
        assert_eq!(
            k.events().iter().filter(|e| **e == (EV_KEY, 56, 0)).count(),
            1,
            "alt was up already"
        );
        assert!(p.events().contains(&(EV_KEY, BTN_LEFT, 0)));
        // Nothing is released twice.
        let before = (p.events().len(), k.events().len());
        inj.release_all();
        assert_eq!((p.events().len(), k.events().len()), before);
    }

    #[test]
    fn without_uinput_the_session_is_view_only() {
        let root = std::env::temp_dir().join(format!("screend-nouinput-{}", std::process::id()));
        assert!(open(&root, (800, 600)).is_none());
        let mut only = ViewOnly;
        only.apply(RemoteInput::Key {
            code: 1,
            down: true,
        });
        only.release_all();
    }
}
