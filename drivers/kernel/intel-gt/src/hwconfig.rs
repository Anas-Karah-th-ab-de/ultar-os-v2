//! The hardware configuration table the GuC hands out: a list of entries, each a key word, a
//! length word (number of value words) and the values. User-space drivers (Mesa) read it through
//! the kernel to learn the GPU's topology; the kernel passes it on unchanged.

use crate::{Error, Result};
use alloc::vec::Vec;

/// One entry of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// What the entry describes.
    pub key: u32,
    /// Its values.
    pub values: Vec<u32>,
}

fn word(table: &[u8], off: usize) -> Option<u32> {
    table
        .get(off..off + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Splits the table into its entries; fails if an entry runs past the end.
pub fn parse(table: &[u8]) -> Result<Vec<Entry>> {
    if !table.len().is_multiple_of(4) {
        return Err(Error::Invalid("hardware config table size"));
    }
    let mut entries = Vec::new();
    let mut off = 0;
    while off < table.len() {
        let (Some(key), Some(len)) = (word(table, off), word(table, off + 4)) else {
            return Err(Error::Invalid("hardware config entry cut off"));
        };
        let start = off + 8;
        let end = (len as usize)
            .checked_mul(4)
            .and_then(|n| start.checked_add(n));
        let Some(end) = end.filter(|e| *e <= table.len()) else {
            return Err(Error::Invalid(
                "hardware config entry longer than the table",
            ));
        };
        let values = (start..end)
            .step_by(4)
            .filter_map(|o| word(table, o))
            .collect();
        entries.push(Entry { key, values });
        off = end;
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    #[test]
    fn splits_entries() {
        let t = table(&[1, 1, 8, 7, 0, 9, 2, 0xaa, 0xbb]);
        let e = parse(&t).unwrap();
        assert_eq!(e.len(), 3);
        assert_eq!(
            e[0],
            Entry {
                key: 1,
                values: alloc::vec![8]
            }
        );
        assert_eq!(
            e[1],
            Entry {
                key: 7,
                values: Vec::new()
            }
        );
        assert_eq!(e[2].values, [0xaa, 0xbb]);
        assert_eq!(parse(&[]).unwrap(), []);
    }

    #[test]
    fn refuses_damaged_tables() {
        assert!(parse(&table(&[1, 2, 8])).is_err());
        assert!(parse(&table(&[1])).is_err());
        assert!(parse(&table(&[1, u32::MAX, 8])).is_err());
        assert!(parse(&[0u8; 7]).is_err());
    }
}
