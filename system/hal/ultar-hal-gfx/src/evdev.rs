//! Input devices that speak the Linux `input_event` protocol: Linux evdev
//! (`/dev/input/event*`) and the Ultar kernel (`event0` = mouse with absolute
//! pixel axes, `event1` = keyboard with Linux key codes).
//!
//! No libinput, no C: records are read raw (24 bytes each), decoded by
//! [`Decoder`] into [`ultar_hal::InputEvent`]s at every `SYN_REPORT`.
//! Absolute positions are normalised to `0..=65535` of the device's axis
//! range; the compositor maps them onto an output.

use crate::GfxError;
use std::fs::{File, OpenOptions};
use std::io::Read;
use std::os::fd::OwnedFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use ultar_hal::{
    HalError, HalResult, Input, InputDeviceId, InputDeviceInfo, InputDeviceKind, InputEvent,
    InputEventKind, KeyState, PointerButton,
};

/// Folder of the event devices.
pub const INPUT_DIR: &str = "/dev/input";
/// Most event devices opened.
pub const MAX_DEVICES: usize = 32;
/// Size of one `struct input_event` on 64-bit targets.
pub const RECORD: usize = 24;
/// Events kept between two `SYN_REPORT`s before the rest is dropped.
const MAX_PENDING: usize = 64;
/// `O_NONBLOCK`.
const O_NONBLOCK: i32 = 0o4000;

const EV_SYN: u16 = 0;
const EV_KEY: u16 = 1;
const EV_REL: u16 = 2;
const EV_ABS: u16 = 3;
const SYN_REPORT: u16 = 0;
const SYN_DROPPED: u16 = 3;
const BTN_LEFT: u16 = 0x110;
const BTN_RIGHT: u16 = 0x111;
const BTN_MIDDLE: u16 = 0x112;
const BTN_TOUCH: u16 = 0x14a;
const ABS_X: u16 = 0;
const ABS_Y: u16 = 1;
const ABS_MT_SLOT: u16 = 0x2f;
const ABS_MT_POSITION_X: u16 = 0x35;
const ABS_MT_POSITION_Y: u16 = 0x36;
const ABS_MT_TRACKING_ID: u16 = 0x39;
const REL_X: u16 = 0;
const REL_Y: u16 = 1;
const REL_HWHEEL: u16 = 6;
const REL_WHEEL: u16 = 8;
/// Keys and buttons remembered as held.
const MAX_HELD: usize = 32;
/// Touch slots tracked.
const SLOTS: usize = 10;

/// Range of an absolute axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AxisRange {
    /// Smallest value.
    pub min: i32,
    /// Largest value.
    pub max: i32,
}

impl AxisRange {
    /// Maps a raw value onto `0..=65535`.
    pub fn normalise(&self, value: i32) -> u16 {
        let span = i64::from(self.max) - i64::from(self.min);
        if span <= 0 {
            return 0;
        }
        let v = (i64::from(value) - i64::from(self.min)).clamp(0, span);
        (v * 65535 / span) as u16
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct Slot {
    active: bool,
    x: i32,
    y: i32,
    changed: bool,
    began: bool,
    ended: bool,
}

/// Turns raw records of one device into HAL events.
#[derive(Debug)]
pub struct Decoder {
    device: InputDeviceId,
    x_range: AxisRange,
    y_range: AxisRange,
    multitouch: bool,
    pending: Vec<InputEvent>,
    rel: (i32, i32),
    abs: Option<(i32, i32)>,
    abs_changed: bool,
    slot: usize,
    slots: [Slot; SLOTS],
    held: Vec<u16>,
}

impl Decoder {
    /// A decoder for `device` with the given axis ranges; `multitouch` turns
    /// absolute positions into touch slots instead of a pointer.
    pub fn new(
        device: InputDeviceId,
        x_range: AxisRange,
        y_range: AxisRange,
        multitouch: bool,
    ) -> Decoder {
        Decoder {
            device,
            x_range,
            y_range,
            multitouch,
            pending: Vec::new(),
            rel: (0, 0),
            abs: None,
            abs_changed: false,
            slot: 0,
            slots: [Slot::default(); SLOTS],
            held: Vec::new(),
        }
    }

    fn push(&mut self, time_us: u64, kind: InputEventKind) {
        if self.pending.len() < MAX_PENDING {
            self.pending.push(InputEvent {
                device: self.device,
                time_us,
                kind,
            });
        }
    }

    /// Feeds one 24-byte record; complete frames are appended to `out`.
    pub fn feed(&mut self, record: &[u8; RECORD], out: &mut Vec<InputEvent>) {
        let sec = u64::from_ne_bytes(record[0..8].try_into().unwrap_or([0; 8]));
        let usec = u64::from_ne_bytes(record[8..16].try_into().unwrap_or([0; 8]));
        let time_us = sec.saturating_mul(1_000_000).saturating_add(usec);
        let kind = u16::from_ne_bytes([record[16], record[17]]);
        let code = u16::from_ne_bytes([record[18], record[19]]);
        let value = i32::from_ne_bytes([record[20], record[21], record[22], record[23]]);
        match kind {
            EV_KEY => self.key(time_us, code, value),
            EV_REL => match code {
                REL_X => self.rel.0 = self.rel.0.saturating_add(value),
                REL_Y => self.rel.1 = self.rel.1.saturating_add(value),
                REL_WHEEL => self.push(
                    time_us,
                    InputEventKind::Scroll {
                        dx: 0.0,
                        dy: -value as f32,
                    },
                ),
                REL_HWHEEL => self.push(
                    time_us,
                    InputEventKind::Scroll {
                        dx: value as f32,
                        dy: 0.0,
                    },
                ),
                _ => {}
            },
            EV_ABS => self.abs_axis(code, value),
            EV_SYN if code == SYN_REPORT => self.flush(time_us, out),
            EV_SYN if code == SYN_DROPPED => self.dropped(time_us, out),
            _ => {}
        }
    }

    /// Remembers which keys and buttons are down (released at `SYN_DROPPED`).
    fn track(&mut self, code: u16, value: i32) {
        if value == 0 {
            self.held.retain(|&c| c != code);
        } else if value == 1 && !self.held.contains(&code) && self.held.len() < MAX_HELD {
            self.held.push(code);
        }
    }

    fn key(&mut self, time_us: u64, code: u16, value: i32) {
        self.track(code, value);
        let button = match code {
            BTN_LEFT => Some(PointerButton::Primary),
            BTN_RIGHT => Some(PointerButton::Secondary),
            BTN_MIDDLE => Some(PointerButton::Middle),
            0x113..=0x11f => Some(PointerButton::Other(code)),
            _ => None,
        };
        if let Some(button) = button {
            let pressed = value != 0;
            self.push(time_us, InputEventKind::PointerButton { button, pressed });
        } else if code == BTN_TOUCH && !self.multitouch {
            // Single-touch screens: BTN_TOUCH + ABS_X/Y drive slot 0.
            let slot = &mut self.slots[0];
            if value != 0 && !slot.active {
                slot.active = true;
                slot.began = true;
            } else if value == 0 && slot.active {
                slot.ended = true;
            }
        } else if code < 0x100 || (0x160..0x300).contains(&code) {
            let state = match value {
                0 => KeyState::Released,
                2 => KeyState::Repeated,
                _ => KeyState::Pressed,
            };
            self.push(time_us, InputEventKind::Key { code, state });
        }
    }

    fn abs_axis(&mut self, code: u16, value: i32) {
        let slot = self.slot;
        match code {
            ABS_X | ABS_Y if !self.multitouch => {
                let (x, y) = self.abs.unwrap_or((0, 0));
                self.abs = Some(if code == ABS_X {
                    (value, y)
                } else {
                    (x, value)
                });
                self.abs_changed = true;
                let s = &mut self.slots[0];
                (s.x, s.y) = self.abs.unwrap_or((0, 0));
                s.changed = true;
            }
            ABS_MT_SLOT => self.slot = (value.max(0) as usize).min(SLOTS - 1),
            ABS_MT_TRACKING_ID => {
                let s = &mut self.slots[slot];
                if value < 0 {
                    if s.active {
                        s.ended = true;
                    }
                } else if !s.active {
                    s.active = true;
                    s.began = true;
                }
            }
            ABS_MT_POSITION_X => {
                self.slots[slot].x = value;
                self.slots[slot].changed = true;
            }
            ABS_MT_POSITION_Y => {
                self.slots[slot].y = value;
                self.slots[slot].changed = true;
            }
            _ => {}
        }
    }

    fn flush(&mut self, time_us: u64, out: &mut Vec<InputEvent>) {
        if self.rel != (0, 0) {
            let (dx, dy) = std::mem::take(&mut self.rel);
            self.push(
                time_us,
                InputEventKind::PointerMotion {
                    dx: dx as f32,
                    dy: dy as f32,
                },
            );
        }
        let touch = self.multitouch || self.slots[0].began || self.slots[0].active;
        if self.abs_changed && !touch {
            if let Some((x, y)) = self.abs {
                let (x, y) = (self.x_range.normalise(x), self.y_range.normalise(y));
                self.push(time_us, InputEventKind::PointerAbsolute { x, y });
            }
        }
        self.abs_changed = false;
        for index in 0..SLOTS {
            let s = self.slots[index];
            let (x, y) = (self.x_range.normalise(s.x), self.y_range.normalise(s.y));
            let slot = index as u8;
            if s.began {
                self.push(time_us, InputEventKind::TouchDown { slot, x, y });
            } else if s.changed && s.active && !s.ended {
                self.push(time_us, InputEventKind::TouchMove { slot, x, y });
            }
            if s.ended {
                self.push(time_us, InputEventKind::TouchUp { slot });
                self.slots[index].active = false;
            }
            let s = &mut self.slots[index];
            (s.began, s.ended, s.changed) = (false, false, false);
        }
        out.append(&mut self.pending);
    }

    /// Events were lost: nothing is held any more (a release may have been
    /// among the lost events), touches are cancelled.
    fn dropped(&mut self, time_us: u64, out: &mut Vec<InputEvent>) {
        self.pending.clear();
        self.rel = (0, 0);
        for code in std::mem::take(&mut self.held) {
            let kind = match code {
                BTN_LEFT => InputEventKind::PointerButton {
                    button: PointerButton::Primary,
                    pressed: false,
                },
                BTN_RIGHT => InputEventKind::PointerButton {
                    button: PointerButton::Secondary,
                    pressed: false,
                },
                BTN_MIDDLE => InputEventKind::PointerButton {
                    button: PointerButton::Middle,
                    pressed: false,
                },
                0x113..=0x11f => InputEventKind::PointerButton {
                    button: PointerButton::Other(code),
                    pressed: false,
                },
                BTN_TOUCH => continue,
                _ => InputEventKind::Key {
                    code,
                    state: KeyState::Released,
                },
            };
            self.push(time_us, kind);
        }
        if self.slots.iter().any(|s| s.active) {
            self.push(time_us, InputEventKind::TouchCancel);
        }
        self.slots = [Slot::default(); SLOTS];
        out.append(&mut self.pending);
    }
}

#[path = "evdev_devices.rs"]
mod devices;
pub use devices::Evdev;

#[cfg(test)]
#[path = "evdev_tests.rs"]
mod tests;
