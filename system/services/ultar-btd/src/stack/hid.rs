//! HID boot protocol (HID 1.11 Appendix B) for keyboards and mice, from both transports:
//! classic HID (L2CAP PSM 0x11/0x13, `DATA | Input` frames with report id 1 = keyboard,
//! 2 = mouse) and HID over GATT (boot input report notifications). The reports become
//! Linux input events (`EV_KEY`, `EV_REL`, `SYN_REPORT`) for the input sink.
//!
//! Report protocol (report maps) is not parsed in this round: devices are switched to
//! boot protocol, which every HID keyboard and mouse must support.

/// `EV_SYN`, `EV_KEY`, `EV_REL`.
pub const EV_SYN: u16 = 0;
/// Key event.
pub const EV_KEY: u16 = 1;
/// Relative axis event.
pub const EV_REL: u16 = 2;
/// Mouse buttons left, right, middle.
pub const BTN_LEFT: u16 = 0x110;
/// Wheel axis.
pub const REL_WHEEL: u16 = 8;

/// One input event (as `struct input_event` without the time).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InputEvent {
    /// `EV_*`.
    pub kind: u16,
    /// Key or axis code.
    pub code: u16,
    /// 1 down / 0 up, or the relative movement.
    pub value: i32,
}

/// What a device is to the input subsystem.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InputKind {
    /// A keyboard.
    Keyboard,
    /// A mouse.
    Mouse,
}

/// Where input goes (Linux: uinput; Ultar: see the integration document).
pub trait InputSink: Send {
    /// A device appeared; `id` is unique while it exists.
    fn added(&mut self, id: u32, kind: InputKind, name: &str);
    /// Events of a device, ending with `SYN_REPORT`.
    fn events(&mut self, id: u32, events: &[InputEvent]);
    /// The device is gone (keys still down are released by the sink).
    fn removed(&mut self, id: u32);
}

/// An input sink that drops everything (no input interface on this system).
pub struct NoInput;

impl InputSink for NoInput {
    fn added(&mut self, _: u32, _: InputKind, _: &str) {}
    fn events(&mut self, _: u32, _: &[InputEvent]) {}
    fn removed(&mut self, _: u32) {}
}

/// HID keyboard usage → Linux key code, usages 0x00..=0x65 (as Linux' `usbkbd`, the same
/// table as the Ultar kernel's `input.rs`).
const HID_KEYS: [u8; 0x66] = [
    0, 0, 0, 0, 30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22,
    47, 17, 45, 21, 44, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 28, 1, 14, 15, 57, 12, 13, 26, 27, 43, 43,
    39, 40, 41, 51, 52, 53, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 87, 88, 99, 70, 119, 110,
    102, 104, 111, 107, 109, 106, 105, 108, 103, 69, 98, 55, 74, 78, 96, 79, 80, 81, 75, 76, 77,
    71, 72, 73, 82, 83, 86, 127,
];
/// Modifier bits 0..7 → left ctrl, shift, alt, meta, right ctrl, shift, alt, meta.
const MODIFIERS: [u16; 8] = [29, 42, 56, 125, 97, 54, 100, 126];

fn key(usage: u8) -> Option<u16> {
    HID_KEYS
        .get(usize::from(usage))
        .copied()
        .filter(|&k| k != 0)
        .map(u16::from)
}

fn syn() -> InputEvent {
    InputEvent {
        kind: EV_SYN,
        code: 0,
        value: 0,
    }
}

/// The state of one boot keyboard (keys held).
#[derive(Default, Clone)]
pub struct Keyboard {
    last: [u8; 8],
}

impl Keyboard {
    /// One boot report (8 octets: modifiers, reserved, six usages) → events.
    pub fn report(&mut self, r: &[u8]) -> Vec<InputEvent> {
        let Some(r) = r.get(..8) else {
            return Vec::new();
        };
        if r[2..].iter().all(|&u| u == 0x01) {
            return Vec::new(); // rollover error: keep the state
        }
        let mut ev = Vec::new();
        for (bit, &code) in MODIFIERS.iter().enumerate() {
            let (was, is) = (self.last[0] >> bit & 1, r[0] >> bit & 1);
            if was != is {
                ev.push(InputEvent {
                    kind: EV_KEY,
                    code,
                    value: i32::from(is),
                });
            }
        }
        for &u in &self.last[2..] {
            if u > 3 && !r[2..].contains(&u) {
                if let Some(code) = key(u) {
                    ev.push(InputEvent {
                        kind: EV_KEY,
                        code,
                        value: 0,
                    });
                }
            }
        }
        for &u in &r[2..] {
            if u > 3 && !self.last[2..].contains(&u) {
                if let Some(code) = key(u) {
                    ev.push(InputEvent {
                        kind: EV_KEY,
                        code,
                        value: 1,
                    });
                }
            }
        }
        self.last.copy_from_slice(r);
        if !ev.is_empty() {
            ev.push(syn());
        }
        ev
    }
}

/// The state of one boot mouse (buttons held).
#[derive(Default, Clone)]
pub struct Mouse {
    buttons: u8,
}

impl Mouse {
    /// One boot report (buttons, dx, dy[, wheel]) → events.
    pub fn report(&mut self, r: &[u8]) -> Vec<InputEvent> {
        if r.len() < 3 {
            return Vec::new();
        }
        let mut ev = Vec::new();
        for bit in 0..3u8 {
            let (was, is) = (self.buttons >> bit & 1, r[0] >> bit & 1);
            if was != is {
                ev.push(InputEvent {
                    kind: EV_KEY,
                    code: BTN_LEFT + u16::from(bit),
                    value: i32::from(is),
                });
            }
        }
        self.buttons = r[0] & 7;
        let axes = [
            (0u16, r[1]),
            (1, r[2]),
            (REL_WHEEL, r.get(3).copied().unwrap_or(0)),
        ];
        for (code, v) in axes {
            if v != 0 {
                ev.push(InputEvent {
                    kind: EV_REL,
                    code,
                    value: i32::from(v as i8),
                });
            }
        }
        if !ev.is_empty() {
            ev.push(syn());
        }
        ev
    }
}

/// Classic HID: `SET_PROTOCOL(boot)` for the control channel.
pub const SET_PROTOCOL_BOOT: u8 = 0x70;

/// Classic HID interrupt frame → (report id, report) of an input report.
pub fn classic_input(frame: &[u8]) -> Option<(u8, &[u8])> {
    match frame {
        [0xA1, id, rest @ ..] => Some((*id, rest)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(ev: &[InputEvent]) -> Vec<(u16, i32)> {
        ev.iter()
            .filter(|e| e.kind == EV_KEY)
            .map(|e| (e.code, e.value))
            .collect()
    }

    #[test]
    fn keyboard_reports_become_key_downs_and_ups() {
        let mut k = Keyboard::default();
        // Shift + "a"
        let ev = k.report(&[0x02, 0, 0x04, 0, 0, 0, 0, 0]);
        assert_eq!(keys(&ev), vec![(42, 1), (30, 1)]);
        assert_eq!(ev.last(), Some(&syn()));
        // "a" held, "b" added, shift released
        assert_eq!(
            keys(&k.report(&[0, 0, 0x04, 0x05, 0, 0, 0, 0])),
            vec![(42, 0), (48, 1)]
        );
        assert!(
            k.report(&[0, 0, 1, 1, 1, 1, 1, 1]).is_empty(),
            "rollover keeps state"
        );
        assert_eq!(keys(&k.report(&[0; 8])), vec![(30, 0), (48, 0)]);
        assert!(k.report(&[0; 3]).is_empty(), "short report ignored");
        assert!(
            k.report(&[0, 0, 0xff, 0, 0, 0, 0, 0]).is_empty(),
            "unknown usage ignored"
        );
    }

    #[test]
    fn mouse_reports_become_buttons_and_motion() {
        let mut m = Mouse::default();
        let ev = m.report(&[0x01, 0xfe, 0x03, 0x01]);
        assert_eq!(
            ev[0],
            InputEvent {
                kind: EV_KEY,
                code: BTN_LEFT,
                value: 1
            }
        );
        assert_eq!(
            ev[1],
            InputEvent {
                kind: EV_REL,
                code: 0,
                value: -2
            }
        );
        assert_eq!(
            ev[2],
            InputEvent {
                kind: EV_REL,
                code: 1,
                value: 3
            }
        );
        assert_eq!(
            ev[3],
            InputEvent {
                kind: EV_REL,
                code: REL_WHEEL,
                value: 1
            }
        );
        assert_eq!(m.report(&[0, 0, 0]).len(), 2, "release + syn");
        assert!(m.report(&[0, 0, 0]).is_empty());
        assert_eq!(
            classic_input(&[0xA1, 2, 1, 2, 3]),
            Some((2, &[1u8, 2, 3][..]))
        );
        assert_eq!(classic_input(&[0xA2, 2]), None);
    }
}
