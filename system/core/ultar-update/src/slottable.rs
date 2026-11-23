//! The slot table: 64 bytes of boot control data, and the decision the boot code takes on it.
//!
//! Layout and rules: `docs/SYSTEM-UPDATE.md` §2.5. This module is pure (no I/O) so the same
//! logic can be compiled into the boot path; [`select`] is that logic.

use crate::bootctl::Slot;
use crate::error::Malformed;

/// Size of one table copy in bytes.
pub const TABLE_LEN: usize = 64;
const MAGIC: [u8; 4] = *b"USLT";
const FORMAT: u8 = 1;
const NONE: u8 = 0xFF;
const FLAG_BOOTABLE: u8 = 1;
const FLAG_SUCCESSFUL: u8 = 2;

/// Flags of one slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SlotFlags {
    /// May be started.
    pub bootable: bool,
    /// Has passed its health checks once.
    pub successful: bool,
}

/// The decoded slot table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotTable {
    /// Slot started when no trial is pending.
    pub default: Slot,
    /// Slot to be started "once" and the attempts left.
    pub trial: Option<(Slot, u8)>,
    /// Flags of slot A and B.
    pub flags: [SlotFlags; 2],
    /// Slot the boot code started last.
    pub last_booted: Option<Slot>,
    /// Sequence number; the copy with the larger one is current.
    pub seq: u32,
}

/// True when sequence number `a` is newer than `b`. Sequence numbers wrap around: `a` is
/// newer when it is ahead of `b` by less than half the number range (so `0` follows
/// `u32::MAX`). Two copies never differ by more than one step.
pub fn seq_newer(a: u32, b: u32) -> bool {
    let ahead = a.wrapping_sub(b);
    ahead != 0 && ahead < 1 << 31
}

/// CRC-32 (IEEE 802.3, reflected, as used by GPT and zlib).
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn flags_byte(flags: SlotFlags) -> u8 {
    u8::from(flags.bootable) * FLAG_BOOTABLE + u8::from(flags.successful) * FLAG_SUCCESSFUL
}

fn flags_from(byte: u8) -> Option<SlotFlags> {
    (byte & !(FLAG_BOOTABLE | FLAG_SUCCESSFUL) == 0).then_some(SlotFlags {
        bootable: byte & FLAG_BOOTABLE != 0,
        successful: byte & FLAG_SUCCESSFUL != 0,
    })
}

fn optional_slot(byte: u8) -> Option<Option<Slot>> {
    if byte == NONE {
        Some(None)
    } else {
        Slot::from_index(byte).map(Some)
    }
}

impl SlotTable {
    /// The table of a freshly installed device: slot A holds the system and has proven itself.
    pub fn factory() -> Self {
        Self {
            default: Slot::A,
            trial: None,
            flags: [
                SlotFlags {
                    bootable: true,
                    successful: true,
                },
                SlotFlags::default(),
            ],
            last_booted: Some(Slot::A),
            seq: 1,
        }
    }

    /// The 64-byte form.
    pub fn encode(&self) -> [u8; TABLE_LEN] {
        let mut out = [0u8; TABLE_LEN];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = FORMAT;
        out[5] = self.default.index() as u8;
        let (trial, tries) = self.trial.map_or((NONE, 0), |(s, t)| (s.index() as u8, t));
        out[6] = trial;
        out[7] = tries;
        out[8] = flags_byte(self.flags[0]);
        out[9] = flags_byte(self.flags[1]);
        out[10] = self.last_booted.map_or(NONE, |s| s.index() as u8);
        out[12..16].copy_from_slice(&self.seq.to_le_bytes());
        let crc = crc32(&out[..60]);
        out[60..].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Decodes one copy. Wrong length, magic, format, checksum, or a value outside its range
    /// make the copy invalid.
    pub fn decode(bytes: &[u8]) -> Result<Self, Malformed> {
        if bytes.len() != TABLE_LEN {
            return Err(Malformed::Length);
        }
        if bytes[..4] != MAGIC || bytes[4] != FORMAT {
            return Err(Malformed::Magic);
        }
        let stored = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
        if crc32(&bytes[..60]) != stored {
            return Err(Malformed::Field);
        }
        let reserved_clear = bytes[11] == 0 && bytes[16..60].iter().all(|b| *b == 0);
        let default = Slot::from_index(bytes[5]).ok_or(Malformed::Field)?;
        let trial = match optional_slot(bytes[6]).ok_or(Malformed::Field)? {
            Some(slot) => Some((slot, bytes[7])),
            None if bytes[7] == 0 => None,
            None => return Err(Malformed::Field),
        };
        let flags = [
            flags_from(bytes[8]).ok_or(Malformed::Field)?,
            flags_from(bytes[9]).ok_or(Malformed::Field)?,
        ];
        let last_booted = optional_slot(bytes[10]).ok_or(Malformed::Field)?;
        if !reserved_clear {
            return Err(Malformed::Field);
        }
        Ok(Self {
            default,
            trial,
            flags,
            last_booted,
            seq: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }

    /// Picks the current table out of the two stored copies.
    pub fn newest(copies: [Option<Self>; 2]) -> Option<Self> {
        match copies {
            [Some(a), Some(b)] => Some(if seq_newer(b.seq, a.seq) { b } else { a }),
            [Some(t), None] | [None, Some(t)] => Some(t),
            [None, None] => None,
        }
    }

    /// Flags of `slot`.
    pub fn slot(&self, slot: Slot) -> SlotFlags {
        self.flags[slot.index()]
    }

    /// Mutable flags of `slot`.
    pub fn slot_mut(&mut self, slot: Slot) -> &mut SlotFlags {
        &mut self.flags[slot.index()]
    }
}

/// What the boot code does at a start: which slot to start and the table to store **before**
/// starting it. `None`: no slot can be started (rescue mode).
///
/// 1. A pending trial with attempts left on a bootable slot: one attempt is used, that slot
///    starts.
/// 2. A pending trial otherwise (attempts used up): the trial slot becomes unbootable.
/// 3. The default slot if bootable; else the other slot if bootable (it becomes the default).
pub fn select(table: &SlotTable) -> Option<(Slot, SlotTable)> {
    let mut next = *table;
    if let Some((slot, tries)) = table.trial {
        if tries > 0 && table.slot(slot).bootable {
            next.trial = Some((slot, tries - 1));
            next.last_booted = Some(slot);
            return Some((slot, next));
        }
        next.trial = None;
        *next.slot_mut(slot) = SlotFlags::default();
    }
    if !next.slot(next.default).bootable {
        if !next.slot(next.default.other()).bootable {
            return None;
        }
        next.default = next.default.other();
    }
    next.last_booted = Some(next.default);
    Some((next.default, next))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: SlotFlags = SlotFlags {
        bootable: true,
        successful: true,
    };
    const NEW: SlotFlags = SlotFlags {
        bootable: true,
        successful: false,
    };

    #[test]
    fn crc_matches_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn encode_decode_round_trip() {
        let mut t = SlotTable::factory();
        t.trial = Some((Slot::B, 3));
        t.flags[1] = NEW;
        t.seq = 77;
        let bytes = t.encode();
        assert_eq!(SlotTable::decode(&bytes).unwrap(), t);
        assert_eq!(&bytes[..11], b"USLT\x01\x00\x01\x03\x03\x01\x00");
    }

    #[test]
    fn every_single_bit_flip_invalidates_a_copy() {
        let bytes = SlotTable::factory().encode();
        for bit in 0..TABLE_LEN * 8 {
            let mut bad = bytes;
            bad[bit / 8] ^= 1 << (bit % 8);
            assert!(SlotTable::decode(&bad).is_err(), "bit {bit}");
        }
        assert!(SlotTable::decode(&bytes[..63]).is_err());
    }

    #[test]
    fn out_of_range_values_with_valid_crc_are_refused() {
        for (offset, value) in [
            (5usize, 2u8),
            (6, 7),
            (8, 4),
            (10, 9),
            (11, 1),
            (20, 1),
            (7, 2),
        ] {
            let mut bytes = SlotTable::factory().encode();
            bytes[offset] = value;
            let crc = crc32(&bytes[..60]);
            bytes[60..].copy_from_slice(&crc.to_le_bytes());
            assert!(SlotTable::decode(&bytes).is_err(), "offset {offset}");
        }
    }

    #[test]
    fn sequence_numbers_wrap_around() {
        assert!(seq_newer(6, 5) && !seq_newer(5, 6) && !seq_newer(5, 5));
        assert!(seq_newer(0, u32::MAX) && !seq_newer(u32::MAX, 0));
        assert!(seq_newer(1, u32::MAX - 1));
        let mut old = SlotTable::factory();
        let mut new = old;
        old.seq = u32::MAX;
        new.seq = 0;
        new.default = Slot::B;
        assert_eq!(
            SlotTable::newest([Some(old), Some(new)]).unwrap().default,
            Slot::B
        );
        assert_eq!(
            SlotTable::newest([Some(new), Some(old)]).unwrap().default,
            Slot::B
        );
    }

    #[test]
    fn newest_copy_wins() {
        let mut a = SlotTable::factory();
        let mut b = a;
        a.seq = 5;
        b.seq = 6;
        assert_eq!(SlotTable::newest([Some(a), Some(b)]).unwrap().seq, 6);
        assert_eq!(SlotTable::newest([Some(b), Some(a)]).unwrap().seq, 6);
        assert_eq!(SlotTable::newest([None, Some(a)]).unwrap().seq, 5);
        assert!(SlotTable::newest([None, None]).is_none());
    }

    #[test]
    fn trial_uses_attempts_then_falls_back() {
        let mut t = SlotTable::factory();
        t.flags[1] = NEW;
        t.trial = Some((Slot::B, 2));
        let (slot, t) = select(&t).unwrap();
        assert_eq!((slot, t.trial), (Slot::B, Some((Slot::B, 1))));
        let (slot, t) = select(&t).unwrap();
        assert_eq!((slot, t.trial), (Slot::B, Some((Slot::B, 0))));
        let (slot, t) = select(&t).unwrap();
        assert_eq!(slot, Slot::A, "automatic fallback");
        assert_eq!(t.trial, None);
        assert_eq!(t.slot(Slot::B), SlotFlags::default());
        assert_eq!(t.default, Slot::A);
        assert_eq!(select(&t).unwrap().0, Slot::A);
    }

    #[test]
    fn trial_on_unbootable_slot_is_dropped() {
        let mut t = SlotTable::factory();
        t.trial = Some((Slot::B, 3));
        let (slot, t) = select(&t).unwrap();
        assert_eq!(slot, Slot::A);
        assert_eq!(t.trial, None);
    }

    #[test]
    fn default_falls_over_and_rescue_when_nothing_is_left() {
        let mut t = SlotTable::factory();
        t.flags = [SlotFlags::default(), GOOD];
        let (slot, t2) = select(&t).unwrap();
        assert_eq!(
            (slot, t2.default, t2.last_booted),
            (Slot::B, Slot::B, Some(Slot::B))
        );
        t.flags = [SlotFlags::default(); 2];
        assert!(select(&t).is_none());
        // A trial whose attempts are used up and a dead default: nothing to start.
        t.flags[1] = NEW;
        t.trial = Some((Slot::B, 0));
        assert!(select(&t).is_none());
    }
}
