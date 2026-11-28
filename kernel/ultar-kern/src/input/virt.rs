//! Capabilities of a virtual input device (`/dev/ultar/uinput`, `svcdev/uinput.rs`): which event
//! types and codes it declared, its name and id, the ranges of its absolute axes – what its
//! `/dev/input/eventN` node answers to `EVIOCG*`, and the filter its events pass.

use alloc::string::String;
use alloc::vec::Vec;

/// Event types: synchronisation, keys/buttons, relative axes, absolute axes.
pub const EV_SYN: u16 = 0;
/// Keys and buttons.
pub const EV_KEY: u16 = 1;
/// Relative axes.
pub const EV_REL: u16 = 2;
/// Absolute axes.
pub const EV_ABS: u16 = 3;
/// Highest key code (`KEY_MAX`).
pub const KEY_MAX: u16 = 0x2ff;
/// Highest relative axis (`REL_MAX`).
pub const REL_MAX: u16 = 0x0f;
/// Highest absolute axis (`ABS_MAX`).
pub const ABS_MAX: u16 = 0x3f;
/// Longest name kept (octets; the rest is cut at a character boundary).
pub const NAME_MAX: usize = 79;

/// What a virtual device may send.
#[derive(Clone)]
pub struct Caps {
    /// Name (`EVIOCGNAME`), printable characters only.
    pub name: String,
    /// `struct input_id`: bus, vendor, product, version.
    pub id: [u16; 4],
    /// Bit per event type (bits 1..=3).
    pub ev: u8,
    /// Bit per key code.
    pub keys: [u8; 96],
    /// Bit per relative axis.
    pub rel: u16,
    /// Bit per absolute axis.
    pub abs: u64,
    /// Minimum and maximum per absolute axis.
    pub ranges: [(i32, i32); 64],
    /// Effective user id of the process that created the device (set by the kernel at
    /// `UI_DEV_CREATE`, never by the service): the seat admits a service's devices by it.
    pub creator: u32,
}

impl Default for Caps {
    fn default() -> Self {
        Caps { name: String::new(), id: [6, 0, 0, 1], ev: 0, keys: [0; 96], rel: 0, abs: 0, ranges: [(0, 0); 64], creator: 0 }
    }
}

impl Caps {
    /// Declare event type `t` (`UI_SET_EVBIT`); only keys and the two kinds of axes exist.
    pub fn set_ev(&mut self, t: u16) -> bool {
        match t {
            EV_SYN => true,
            EV_KEY | EV_REL | EV_ABS => {
                self.ev |= 1 << t;
                true
            }
            _ => false,
        }
    }

    /// Declare code `code` of type `t` (`UI_SET_KEYBIT`, `UI_SET_RELBIT`, `UI_SET_ABSBIT`).
    pub fn set_code(&mut self, t: u16, code: u16) -> bool {
        match t {
            EV_KEY if (1..=KEY_MAX).contains(&code) => self.keys[code as usize / 8] |= 1 << (code % 8),
            EV_REL if code <= REL_MAX => self.rel |= 1 << code,
            EV_ABS if code <= ABS_MAX => self.abs |= 1 << code,
            _ => return false,
        }
        true
    }

    /// Range of absolute axis `code`; refused if minimum > maximum.
    pub fn set_range(&mut self, code: u16, min: i32, max: i32) -> bool {
        if code > ABS_MAX || min > max {
            return false;
        }
        self.ranges[code as usize] = (min, max);
        true
    }

    /// Keep `raw` (octets of a C string) as the name: up to the first NUL, invalid UTF-8 and
    /// control characters dropped, at most [`NAME_MAX`] octets.
    pub fn set_name(&mut self, raw: &[u8]) {
        let raw = raw.split(|b| *b == 0).next().unwrap_or(&[]);
        let mut name = String::new();
        for c in String::from_utf8_lossy(raw).chars().filter(|c| !c.is_control() && *c != '\u{fffd}') {
            if name.len() + c.len_utf8() > NAME_MAX {
                break;
            }
            name.push(c);
        }
        self.name = name;
    }

    fn key(&self, code: u16) -> bool {
        code <= KEY_MAX && self.keys[code as usize / 8] & (1 << (code % 8)) != 0
    }

    /// Whether the device can be created: some type with some code declared, ranges valid.
    pub fn complete(&self) -> bool {
        let keys = self.ev & (1 << EV_KEY) != 0 && self.keys.iter().any(|b| *b != 0);
        let rel = self.ev & (1 << EV_REL) != 0 && self.rel != 0;
        let abs = self.ev & (1 << EV_ABS) != 0 && self.abs != 0;
        keys || rel || abs
    }

    /// The event `(type, code, value)` as the node delivers it, or `None` if the device did not
    /// declare it (absolute values are held to their range).
    pub fn filter(&self, t: u16, code: u16, value: i32) -> Option<i32> {
        let declared = |bit: u16| self.ev & (1 << bit) != 0;
        match t {
            EV_SYN => (code == 0).then_some(0),
            EV_KEY if declared(EV_KEY) && self.key(code) && (0..=2).contains(&value) => Some(value),
            EV_REL if declared(EV_REL) && code <= REL_MAX && self.rel & (1 << code) != 0 => Some(value),
            EV_ABS if declared(EV_ABS) && code <= ABS_MAX && self.abs & (1 << code) != 0 => {
                let (min, max) = self.ranges[code as usize];
                Some(value.clamp(min, max))
            }
            _ => None,
        }
    }

    /// Bitmap for `EVIOCGBIT(ev)`.
    pub fn bits(&self, ev: u8) -> Vec<u8> {
        match ev {
            0 => alloc::vec![self.ev | 1, 0, 0, 0],
            1 => self.keys.to_vec(),
            2 => self.rel.to_le_bytes().to_vec(),
            3 => self.abs.to_le_bytes().to_vec(),
            _ => alloc::vec![0u8; 8],
        }
    }

    /// `struct input_absinfo` of axis `code` (if declared).
    pub fn absinfo(&self, code: u8) -> Option<Vec<u8>> {
        if code as u16 > ABS_MAX || self.abs & (1 << code) == 0 {
            return None;
        }
        let (min, max) = self.ranges[code as usize];
        Some([0, min, max, 0, 0, 0].iter().flat_map(|v: &i32| v.to_le_bytes()).collect())
    }
}
