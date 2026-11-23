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

    fn key(&mut self, time_us: u64, code: u16, value: i32) {
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

    fn dropped(&mut self, time_us: u64, out: &mut Vec<InputEvent>) {
        self.pending.clear();
        self.rel = (0, 0);
        if self.slots.iter().any(|s| s.active) {
            self.push(time_us, InputEventKind::TouchCancel);
        }
        self.slots = [Slot::default(); SLOTS];
        out.append(&mut self.pending);
    }
}

struct Device {
    info: InputDeviceInfo,
    file: File,
    decoder: Decoder,
    partial: Vec<u8>,
}

/// All event devices of the machine.
pub struct Evdev {
    devices: Mutex<Vec<Device>>,
}

impl std::fmt::Debug for Evdev {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Evdev")
    }
}

const fn ioc_read(nr: u32, len: u32) -> rustix::ioctl::Opcode {
    (0x8000_0000 | (len << 16) | (0x45 << 8) | nr) as rustix::ioctl::Opcode
}

/// Issues one reading `ioctl` whose opcode is a constant.
macro_rules! read_ioctl {
    ($file:expr, $nr:expr, $len:expr, $value:expr) => {
        // SAFETY: an `EVIOCG*` request of `$len` bytes; the kernel writes at
        // most that many bytes into `$value`, whose type has exactly that size.
        unsafe {
            rustix::ioctl::ioctl(
                $file,
                rustix::ioctl::Updater::<{ ioc_read($nr, $len) }, _>::new($value),
            )
        }
        .is_ok()
    };
}

fn bits(file: &File, event_type: u16, out: &mut [u8; 96]) -> bool {
    match event_type {
        0 => read_ioctl!(file, 0x20, 96, out),
        EV_KEY => read_ioctl!(file, 0x21, 96, out),
        _ => read_ioctl!(file, 0x23, 96, out),
    }
}

fn axis(file: &File, code: u16) -> Option<AxisRange> {
    let mut info = [0i32; 6];
    let ok = match code {
        ABS_X => read_ioctl!(file, 0x40, 24, &mut info),
        ABS_Y => read_ioctl!(file, 0x41, 24, &mut info),
        ABS_MT_POSITION_X => read_ioctl!(file, 0x75, 24, &mut info),
        ABS_MT_POSITION_Y => read_ioctl!(file, 0x76, 24, &mut info),
        _ => false,
    };
    ok.then_some(AxisRange {
        min: info[1],
        max: info[2],
    })
}

fn has(bits: &[u8; 96], bit: u16) -> bool {
    bits.get(usize::from(bit / 8))
        .is_some_and(|b| b & (1 << (bit % 8)) != 0)
}

/// Decides the kind of a device from its capability bits; `None` when the
/// kernel does not answer `EVIOCGBIT` (the Ultar kernel).
fn classify(file: &File) -> Option<(InputDeviceKind, bool)> {
    let (mut types, mut keys, mut abs) = ([0u8; 96], [0u8; 96], [0u8; 96]);
    if !bits(file, 0, &mut types) {
        return None;
    }
    bits(file, EV_KEY, &mut keys);
    bits(file, EV_ABS, &mut abs);
    let kind = if has(&types, EV_ABS) && has(&abs, ABS_MT_SLOT) {
        (InputDeviceKind::Touchscreen, true)
    } else if has(&types, EV_ABS) && has(&keys, BTN_TOUCH) {
        (InputDeviceKind::Touchscreen, false)
    } else if has(&types, EV_REL) || (has(&types, EV_ABS) && has(&keys, BTN_LEFT)) {
        (InputDeviceKind::Mouse, false)
    } else if has(&keys, 30) {
        (InputDeviceKind::Keyboard, false)
    } else {
        (InputDeviceKind::Buttons, false)
    };
    Some(kind)
}

impl Evdev {
    /// Opens every readable `event*` device below [`INPUT_DIR`].
    pub fn open_all() -> Result<Evdev, GfxError> {
        let mut paths: Vec<PathBuf> = Vec::new();
        for index in 0..MAX_DEVICES {
            let path = Path::new(INPUT_DIR).join(format!("event{index}"));
            if path.exists() {
                paths.push(path);
            }
        }
        Evdev::open_paths(&paths)
    }

    /// Opens the given event devices; unreadable ones are skipped.
    pub fn open_paths(paths: &[PathBuf]) -> Result<Evdev, GfxError> {
        let mut devices = Vec::new();
        for path in paths.iter().take(MAX_DEVICES) {
            let Ok(file) = OpenOptions::new()
                .read(true)
                .custom_flags(O_NONBLOCK)
                .open(path)
            else {
                continue;
            };
            let id = InputDeviceId(devices.len() as u32);
            let x_axis = axis(&file, ABS_X);
            let (kind, multitouch) = classify(&file).unwrap_or(if x_axis.is_some() {
                (InputDeviceKind::Mouse, false)
            } else {
                (InputDeviceKind::Keyboard, false)
            });
            let (x_code, y_code) = if multitouch {
                (ABS_MT_POSITION_X, ABS_MT_POSITION_Y)
            } else {
                (ABS_X, ABS_Y)
            };
            let unit = AxisRange { min: 0, max: 65535 };
            let x = axis(&file, x_code).unwrap_or(unit);
            let y = axis(&file, y_code).unwrap_or(unit);
            let decoder = Decoder::new(id, x, y, multitouch);
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            devices.push(Device {
                info: InputDeviceInfo {
                    id,
                    kind,
                    name,
                    internal: true,
                },
                file,
                decoder,
                partial: Vec::new(),
            });
        }
        if devices.is_empty() {
            return Err(GfxError::NoDevice);
        }
        Ok(Evdev {
            devices: Mutex::new(devices),
        })
    }

    /// Duplicated descriptors of the devices for the caller's `poll` loop
    /// (readable = [`Input::poll`] has events).
    pub fn poll_handles(&self) -> Vec<OwnedFd> {
        let Ok(devices) = self.devices.lock() else {
            return Vec::new();
        };
        devices
            .iter()
            .filter_map(|d| d.file.try_clone().ok().map(OwnedFd::from))
            .collect()
    }
}

impl Input for Evdev {
    fn devices(&self) -> HalResult<Vec<InputDeviceInfo>> {
        let devices = self.devices.lock().map_err(|_| HalError::Busy)?;
        Ok(devices.iter().map(|d| d.info.clone()).collect())
    }

    /// Reads what is there without blocking (`timeout_ms` is not waited:
    /// the compositor waits on [`Evdev::poll_handles`]).
    fn poll(&self, _timeout_ms: u32) -> HalResult<Vec<InputEvent>> {
        let mut devices = self.devices.lock().map_err(|_| HalError::Busy)?;
        let mut out = Vec::new();
        let mut buf = [0u8; RECORD * 64];
        for device in devices.iter_mut() {
            loop {
                match device.file.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        device.partial.extend_from_slice(&buf[..n]);
                        let whole = device.partial.len() / RECORD * RECORD;
                        for chunk in device.partial[..whole].chunks_exact(RECORD) {
                            let mut record = [0u8; RECORD];
                            record.copy_from_slice(chunk);
                            device.decoder.feed(&record, &mut out);
                        }
                        device.partial.drain(..whole);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => break,
                }
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
#[path = "evdev_tests.rs"]
mod tests;
