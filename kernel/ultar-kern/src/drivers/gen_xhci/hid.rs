//! HID report descriptors of pointing devices (HID 1.11 §6.2.2): where buttons, X, Y and the wheel
//! sit in an input report. Enough for mice (boot or report protocol) and absolute tablets such as
//! QEMU's `usb-tablet`; anything without X and Y is not a pointer.

use alloc::vec::Vec;

/// One field of an input report.
#[derive(Clone, Copy, Debug, Default)]
pub struct Field {
    /// First bit after the report id byte.
    pub bit: u32,
    /// Size in bits (1..=32).
    pub size: u32,
    /// Logical minimum (negative: the field is signed).
    pub min: i32,
    /// Logical maximum.
    pub max: i32,
    /// Absolute (else relative).
    pub abs: bool,
}

impl Field {
    /// Its value in `data` (the report without the id byte); 0 when the report is too short.
    pub fn read(&self, data: &[u8]) -> i32 {
        let mut v: u64 = 0;
        for i in 0..self.size {
            let b = self.bit + i;
            let byte = data.get((b / 8) as usize).copied().unwrap_or(0);
            v |= u64::from(byte >> (b % 8) & 1) << i;
        }
        if self.min < 0 && self.size < 64 && v >> (self.size - 1) & 1 == 1 {
            v |= !0u64 << self.size;
        }
        v as i64 as i32
    }
}

/// The pointer fields of one report.
#[derive(Clone, Debug, Default)]
pub struct Layout {
    /// Report id (0: the device uses none).
    pub id: u8,
    pub x: Field,
    pub y: Field,
    pub wheel: Option<Field>,
    /// Button 1, 2, 3 (left, right, middle).
    pub buttons: Vec<Field>,
}

/// Parser state: globals, locals, the bit position per report.
#[derive(Default)]
struct State {
    page: u32,
    min: i32,
    max: i32,
    size: u32,
    count: u32,
    id: u8,
    bits: u32,
    usages: Vec<u32>,
    range: Option<(u32, u32)>,
}

fn value(d: &[u8], signed: bool) -> i32 {
    match (d.len(), signed) {
        (1, true) => d[0] as i8 as i32,
        (2, true) => i16::from_le_bytes([d[0], d[1]]) as i32,
        (1, false) => d[0] as i32,
        (2, false) => u16::from_le_bytes([d[0], d[1]]) as i32,
        (4, _) => i32::from_le_bytes([d[0], d[1], d[2], d[3]]),
        _ => 0,
    }
}

/// Record the fields of one `Input` main item.
fn input(st: &mut State, flags: u32, found: &mut Layout, have: &mut bool) {
    let variable = flags & 2 != 0 && flags & 1 == 0;
    for i in 0..st.count {
        let usage = match st.range {
            Some((lo, hi)) => (lo + i).min(hi),
            None => st
                .usages
                .get(i as usize)
                .or(st.usages.last())
                .copied()
                .unwrap_or(0),
        };
        let usage = if usage > 0xffff {
            usage
        } else {
            st.page << 16 | usage
        };
        let f = Field {
            bit: st.bits,
            size: st.size.min(32),
            min: st.min,
            max: st.max,
            abs: flags & 4 == 0,
        };
        st.bits += st.size;
        if !variable || (*have && found.id != st.id) {
            continue;
        }
        match usage {
            0x1_0030 => (found.x, *have, found.id) = (f, true, st.id),
            0x1_0031 => found.y = f,
            0x1_0038 => found.wheel = Some(f),
            0x9_0001..=0x9_0003 if found.buttons.len() < 3 => found.buttons.push(f),
            _ => {}
        }
    }
}

/// The pointer layout described by `desc`; `None` without X and Y.
pub fn parse(desc: &[u8]) -> Option<Layout> {
    let (mut st, mut found, mut have) = (State::default(), Layout::default(), false);
    let mut at = 0usize;
    while at < desc.len() {
        let prefix = desc[at];
        if prefix == 0xfe {
            // long item: skipped
            at += 3 + *desc.get(at + 1)? as usize;
            continue;
        }
        let len = [0usize, 1, 2, 4][(prefix & 3) as usize];
        let data = desc.get(at + 1..at + 1 + len)?;
        at += 1 + len;
        let unsigned = value(data, false) as u32;
        match prefix & 0xfc {
            0x80 => input(&mut st, unsigned, &mut found, &mut have),
            0x90 | 0xb0 | 0xa0 | 0xc0 => {}
            0x04 => st.page = unsigned,
            0x14 => st.min = value(data, true),
            0x24 => {
                st.max = if st.min < 0 {
                    value(data, true)
                } else {
                    unsigned as i32
                }
            }
            0x74 => st.size = unsigned,
            0x94 => st.count = unsigned,
            0x84 => (st.id, st.bits) = (unsigned as u8, 0),
            0x08 => st.usages.push(unsigned),
            0x18 => st.range = Some((unsigned, st.range.map_or(unsigned, |r| r.1))),
            0x28 => st.range = Some((st.range.map_or(0, |r| r.0), unsigned)),
            _ => {}
        }
        if prefix & 0x0c == 0 {
            // a main item ends the local items
            st.usages.clear();
            st.range = None;
        }
    }
    (have && found.y.size > 0).then_some(found)
}
