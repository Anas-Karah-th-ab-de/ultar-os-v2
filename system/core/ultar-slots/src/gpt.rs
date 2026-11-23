//! A strict reader of GUID partition tables (UEFI 2.10 §5.3), without allocation.
//!
//! The boot chain finds its partitions by GPT name. A table that is damaged, inconsistent or
//! names one of our partitions twice is refused: starting a system from the wrong partition is
//! worse than not starting.

use crate::crc::{crc32, crc32_update};

/// Size of the GPT header that this reader checksums (revision 1.0).
pub const HEADER_LEN: usize = 92;
/// Largest entry array accepted (bytes); the usual table is 128 × 128 = 16 KiB.
pub const MAX_ENTRY_BYTES: usize = 64 * 1024;
/// Characters of a partition name.
pub const NAME_UNITS: usize = 36;

/// Why a partition table was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GptError {
    /// No `EFI PART` signature or an unknown revision.
    Signature,
    /// Header checksum, header size or its own position is wrong.
    Header,
    /// Entry size, count or table position is outside what the standard allows.
    Geometry,
    /// The entry array does not match its checksum.
    EntryCrc,
    /// An entry lies outside the usable area or ends before it starts.
    Entry,
    /// Two used entries carry the name that was looked for.
    DuplicateName,
}

/// The fields of a valid GPT header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// LBA of this header.
    pub my_lba: u64,
    /// LBA of the other header.
    pub alternate_lba: u64,
    /// First LBA a partition may use.
    pub first_usable: u64,
    /// Last LBA a partition may use.
    pub last_usable: u64,
    /// LBA of the entry array.
    pub entries_lba: u64,
    /// Number of entries.
    pub entry_count: u32,
    /// Size of one entry in bytes.
    pub entry_size: u32,
    /// CRC-32 of the entry array.
    pub entries_crc: u32,
    /// Disk GUID (on-disk byte order): names the disk the loader started from.
    pub disk_guid: [u8; 16],
}

/// One used partition entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// Partition type GUID (on-disk byte order).
    pub type_guid: [u8; 16],
    /// Unique partition GUID (on-disk byte order).
    pub unique_guid: [u8; 16],
    /// First LBA.
    pub first_lba: u64,
    /// Last LBA (inclusive).
    pub last_lba: u64,
    /// Name, UTF-16, zero padded.
    pub name: [u16; NAME_UNITS],
}

fn le32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

fn le64(b: &[u8], o: usize) -> u64 {
    u64::from(le32(b, o)) | u64::from(le32(b, o + 4)) << 32
}

impl Header {
    /// Checks the header read from `lba` (`block` = the whole block, any size ≥ 92).
    pub fn parse(block: &[u8], lba: u64) -> Result<Self, GptError> {
        if block.len() < HEADER_LEN || &block[..8] != b"EFI PART" || le32(block, 8) != 0x0001_0000 {
            return Err(GptError::Signature);
        }
        let size = le32(block, 12) as usize;
        if size < HEADER_LEN || size > block.len() {
            return Err(GptError::Header);
        }
        let stored = le32(block, 16);
        let crc = crc32_update(
            crc32_update(crc32_update(!0, &block[..16]), &[0; 4]),
            &block[20..size],
        ) ^ !0;
        if crc != stored || le64(block, 24) != lba {
            return Err(GptError::Header);
        }
        let h = Self {
            my_lba: lba,
            alternate_lba: le64(block, 32),
            first_usable: le64(block, 40),
            last_usable: le64(block, 48),
            entries_lba: le64(block, 72),
            entry_count: le32(block, 80),
            entry_size: le32(block, 84),
            entries_crc: le32(block, 88),
            disk_guid: [0; 16],
        };
        let mut h = h;
        h.disk_guid.copy_from_slice(&block[56..72]);
        let size_ok =
            h.entry_size >= 128 && h.entry_size.is_multiple_of(128) && h.entry_size <= 1024;
        let bytes = u64::from(h.entry_count) * u64::from(h.entry_size);
        if !size_ok
            || h.entry_count == 0
            || bytes > MAX_ENTRY_BYTES as u64
            || h.first_usable > h.last_usable
        {
            return Err(GptError::Geometry);
        }
        Ok(h)
    }

    /// Size of the entry array in bytes (at most [`MAX_ENTRY_BYTES`]).
    pub fn entries_len(&self) -> usize {
        self.entry_count as usize * self.entry_size as usize
    }

    /// Checks the entry array (exactly [`Header::entries_len`] bytes) against its checksum.
    pub fn check_entries(&self, entries: &[u8]) -> Result<(), GptError> {
        if entries.len() != self.entries_len() || crc32(entries) != self.entries_crc {
            return Err(GptError::EntryCrc);
        }
        Ok(())
    }

    /// The used entry at `index`, checked against the usable area. `Ok(None)` for an unused
    /// entry or an index past the end.
    pub fn entry(&self, entries: &[u8], index: usize) -> Result<Option<Entry>, GptError> {
        let size = self.entry_size as usize;
        let Some(raw) = entries.get(index * size..index * size + 128) else {
            return Ok(None);
        };
        if raw[..16].iter().all(|b| *b == 0) {
            return Ok(None);
        }
        let mut e = Entry {
            type_guid: [0; 16],
            unique_guid: [0; 16],
            first_lba: le64(raw, 32),
            last_lba: le64(raw, 40),
            name: [0; NAME_UNITS],
        };
        e.type_guid.copy_from_slice(&raw[..16]);
        e.unique_guid.copy_from_slice(&raw[16..32]);
        for (i, unit) in e.name.iter_mut().enumerate() {
            *unit = u16::from_le_bytes([raw[56 + 2 * i], raw[57 + 2 * i]]);
        }
        if e.first_lba > e.last_lba
            || e.first_lba < self.first_usable
            || e.last_lba > self.last_usable
        {
            return Err(GptError::Entry);
        }
        Ok(Some(e))
    }

    /// The one used entry called `name`. Two entries with that name are an error.
    pub fn find(&self, entries: &[u8], name: &str) -> Result<Option<Entry>, GptError> {
        let mut found = None;
        for index in 0..self.entry_count as usize {
            if let Some(e) = self.entry(entries, index)? {
                if e.name_is(name) {
                    if found.is_some() {
                        return Err(GptError::DuplicateName);
                    }
                    found = Some(e);
                }
            }
        }
        Ok(found)
    }
}

impl Entry {
    /// Whether the name equals `name` exactly.
    pub fn name_is(&self, name: &str) -> bool {
        let mut units = name.encode_utf16();
        for stored in self.name {
            match (units.next(), stored) {
                (Some(u), s) if u == s => {}
                (None, 0) => return true,
                _ => return false,
            }
        }
        units.next().is_none()
    }

    /// Number of blocks.
    pub fn blocks(&self) -> u64 {
        self.last_lba - self.first_lba + 1
    }
}

/// Writes a header + entry array pair for tests and tools (`entries` already filled).
/// Returns the header block (512 bytes).
pub fn build_header(h: &Header, entries: &[u8]) -> [u8; 512] {
    let mut b = [0u8; 512];
    b[..8].copy_from_slice(b"EFI PART");
    b[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    b[12..16].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
    b[24..32].copy_from_slice(&h.my_lba.to_le_bytes());
    b[32..40].copy_from_slice(&h.alternate_lba.to_le_bytes());
    b[40..48].copy_from_slice(&h.first_usable.to_le_bytes());
    b[48..56].copy_from_slice(&h.last_usable.to_le_bytes());
    b[56..72].copy_from_slice(&h.disk_guid);
    b[72..80].copy_from_slice(&h.entries_lba.to_le_bytes());
    b[80..84].copy_from_slice(&h.entry_count.to_le_bytes());
    b[84..88].copy_from_slice(&h.entry_size.to_le_bytes());
    b[88..92].copy_from_slice(&crc32(entries).to_le_bytes());
    let crc = crc32(&b[..HEADER_LEN]);
    b[16..20].copy_from_slice(&crc.to_le_bytes());
    b
}

/// Fills one 128-byte entry (for the installer and tests).
pub fn build_entry(
    out: &mut [u8],
    type_guid: [u8; 16],
    unique_guid: [u8; 16],
    first: u64,
    last: u64,
    name: &str,
) {
    out[..128].fill(0);
    out[..16].copy_from_slice(&type_guid);
    out[16..32].copy_from_slice(&unique_guid);
    out[32..40].copy_from_slice(&first.to_le_bytes());
    out[40..48].copy_from_slice(&last.to_le_bytes());
    for (i, u) in name.encode_utf16().take(NAME_UNITS).enumerate() {
        out[56 + 2 * i..58 + 2 * i].copy_from_slice(&u.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: [u8; 16] = [0xAF; 16];

    fn table(names: &[(&str, u64, u64)]) -> (Header, std::vec::Vec<u8>, [u8; 512]) {
        let mut entries = std::vec![0u8; 128 * 128];
        for (i, (n, f, l)) in names.iter().enumerate() {
            build_entry(&mut entries[i * 128..], T, [i as u8 + 1; 16], *f, *l, n);
        }
        let h = Header {
            my_lba: 1,
            alternate_lba: 9999,
            first_usable: 34,
            last_usable: 9966,
            entries_lba: 2,
            entry_count: 128,
            entry_size: 128,
            entries_crc: crc32(&entries),
            disk_guid: [7; 16],
        };
        let block = build_header(&h, &entries);
        (h, entries, block)
    }

    #[test]
    fn finds_partitions_by_exact_name() {
        let (h, entries, block) =
            table(&[("ULTAR-EFI", 2048, 4095), ("ultar-system-a", 4096, 8191)]);
        let parsed = Header::parse(&block, 1).unwrap();
        assert_eq!(parsed, h);
        parsed.check_entries(&entries).unwrap();
        let e = parsed.find(&entries, "ultar-system-a").unwrap().unwrap();
        assert_eq!((e.first_lba, e.blocks()), (4096, 4096));
        assert!(parsed.find(&entries, "ultar-system").unwrap().is_none());
        assert!(parsed.find(&entries, "ultar-system-a2").unwrap().is_none());
        assert!(parsed.find(&entries, "ULTAR-EFI").unwrap().is_some());
        assert!(parsed.entry(&entries, 500).unwrap().is_none());
        let full = "abcdefghijklmnopqrstuvwxyz0123456789";
        let (_, entries, block) = table(&[(full, 100, 200)]);
        let parsed = Header::parse(&block, 1).unwrap();
        assert!(parsed.find(&entries, full).unwrap().is_some());
        assert!(parsed
            .find(&entries, "abcdefghijklmnopqrstuvwxyz0123456789x")
            .unwrap()
            .is_none());
    }

    #[test]
    fn duplicates_and_bad_entries_are_refused() {
        let (_, entries, block) =
            table(&[("ultar-system-a", 100, 200), ("ultar-system-a", 300, 400)]);
        let h = Header::parse(&block, 1).unwrap();
        assert_eq!(
            h.find(&entries, "ultar-system-a"),
            Err(GptError::DuplicateName)
        );
        for (f, l) in [(200, 100), (10, 100), (100, 10_000)] {
            let (_, entries, block) = table(&[("x", f, l)]);
            let h = Header::parse(&block, 1).unwrap();
            assert_eq!(h.find(&entries, "x"), Err(GptError::Entry));
        }
    }

    #[test]
    fn damage_is_detected() {
        let (_, mut entries, block) = table(&[("ULTAR-BOOTCTL", 100, 200)]);
        assert_eq!(Header::parse(&block, 2), Err(GptError::Header));
        for byte in 0..HEADER_LEN {
            let mut bad = block;
            bad[byte] ^= 0x10;
            assert!(Header::parse(&bad, 1).is_err(), "byte {byte}");
        }
        let h = Header::parse(&block, 1).unwrap();
        entries[300] ^= 1;
        assert_eq!(h.check_entries(&entries), Err(GptError::EntryCrc));
        assert_eq!(h.check_entries(&entries[..100]), Err(GptError::EntryCrc));
        assert_eq!(Header::parse(&block[..50], 1), Err(GptError::Signature));
    }

    #[test]
    fn geometry_limits() {
        let entries = std::vec![0u8; 128];
        for (count, size) in [(0u32, 128u32), (1, 100), (1, 2048), (1024, 128)] {
            let h = Header {
                my_lba: 1,
                alternate_lba: 2,
                first_usable: 34,
                last_usable: 99,
                entries_lba: 2,
                entry_count: count,
                entry_size: size,
                entries_crc: 0,
                disk_guid: [0; 16],
            };
            let block = build_header(&h, &entries);
            assert_eq!(
                Header::parse(&block, 1),
                Err(GptError::Geometry),
                "{count} x {size}"
            );
        }
    }
}
