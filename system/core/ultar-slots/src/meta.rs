//! The slot record: signed facts about a slot's content that the start chain needs.
//!
//! 64 bytes at [`crate::layout::META_OFFSETS`] on `ULTAR-BOOTCTL`:
//!
//! | Offset | Field |
//! |---|---|
//! | 0 | magic `"USLM"` |
//! | 4 | format = 1 |
//! | 5 | slot (0 = A, 1 = B) |
//! | 6 | 1 = a verity root hash follows, 0 = none |
//! | 8 | verity root hash (32 bytes, zero when absent) |
//! | 60 | CRC-32 (IEEE) of bytes 0 … 59 |
//!
//! All other bytes are zero. The updater writes a slot's record only while that slot is
//! unbootable (before `mark_bootable` flips the table), so a torn record is never trusted.

use crate::crc::crc32;
use crate::slot::Slot;
use crate::table::TableError;

/// Size of a record in bytes.
pub const META_LEN: usize = 64;
const MAGIC: [u8; 4] = *b"USLM";
const FORMAT: u8 = 1;

/// The decoded slot record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotRecord {
    /// The slot it describes (must match where it is stored).
    pub slot: Slot,
    /// dm-verity root hash of the slot's system image, when the release has one.
    pub verity_root: Option<[u8; 32]>,
}

impl SlotRecord {
    /// The 64-byte form.
    pub fn encode(&self) -> [u8; META_LEN] {
        let mut out = [0u8; META_LEN];
        out[..4].copy_from_slice(&MAGIC);
        out[4] = FORMAT;
        out[5] = self.slot.index() as u8;
        if let Some(root) = self.verity_root {
            out[6] = 1;
            out[8..40].copy_from_slice(&root);
        }
        let crc = crc32(&out[..60]);
        out[60..].copy_from_slice(&crc.to_le_bytes());
        out
    }

    /// Decodes a record and checks that it describes `expected`.
    pub fn decode(bytes: &[u8], expected: Slot) -> Result<Self, TableError> {
        if bytes.len() != META_LEN {
            return Err(TableError::Length);
        }
        if bytes[..4] != MAGIC || bytes[4] != FORMAT {
            return Err(TableError::Magic);
        }
        let stored = u32::from_le_bytes([bytes[60], bytes[61], bytes[62], bytes[63]]);
        let reserved_clear = bytes[7] == 0 && bytes[40..60].iter().all(|b| *b == 0);
        if crc32(&bytes[..60]) != stored || !reserved_clear {
            return Err(TableError::Field);
        }
        if Slot::from_index(bytes[5]) != Some(expected) {
            return Err(TableError::Field);
        }
        let mut root = [0u8; 32];
        root.copy_from_slice(&bytes[8..40]);
        let verity_root = match bytes[6] {
            0 if root == [0; 32] => None,
            1 => Some(root),
            _ => return Err(TableError::Field),
        };
        Ok(Self {
            slot: expected,
            verity_root,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_every_bit_flip_is_detected() {
        for rec in [
            SlotRecord {
                slot: Slot::A,
                verity_root: None,
            },
            SlotRecord {
                slot: Slot::B,
                verity_root: Some([0xA5; 32]),
            },
        ] {
            let bytes = rec.encode();
            assert_eq!(SlotRecord::decode(&bytes, rec.slot), Ok(rec));
            assert_eq!(
                SlotRecord::decode(&bytes, rec.slot.other()),
                Err(TableError::Field)
            );
            for bit in 0..META_LEN * 8 {
                let mut bad = bytes;
                bad[bit / 8] ^= 1 << (bit % 8);
                assert!(SlotRecord::decode(&bad, rec.slot).is_err(), "bit {bit}");
            }
            assert_eq!(
                SlotRecord::decode(&bytes[..63], rec.slot),
                Err(TableError::Length)
            );
        }
    }

    #[test]
    fn inconsistent_values_with_valid_crc_are_refused() {
        for (offset, value) in [(6usize, 2u8), (6, 0), (7, 1), (45, 1), (5, 3)] {
            let mut bytes = SlotRecord {
                slot: Slot::A,
                verity_root: Some([1; 32]),
            }
            .encode();
            bytes[offset] = value;
            let crc = crc32(&bytes[..60]);
            bytes[60..].copy_from_slice(&crc.to_le_bytes());
            assert!(
                SlotRecord::decode(&bytes, Slot::A).is_err(),
                "offset {offset}"
            );
        }
        let mut zero = [0u8; META_LEN];
        assert_eq!(SlotRecord::decode(&zero, Slot::A), Err(TableError::Magic));
        zero[..4].copy_from_slice(b"USLM");
        zero[4] = 2;
        assert_eq!(SlotRecord::decode(&zero, Slot::A), Err(TableError::Magic));
    }
}
