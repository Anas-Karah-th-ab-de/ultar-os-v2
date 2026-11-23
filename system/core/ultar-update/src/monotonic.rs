//! A number that only grows, kept in two redundant files.
//!
//! Used for the rollback floor, the time floor and the key-set serial. Each file holds
//! `<decimal value> <sha256 hex of the decimal value>\n`. Reading takes the larger valid copy;
//! a single damaged or missing copy is tolerated and repaired by the next write. Two damaged
//! copies are an error: the caller must then refuse updates (fail closed) instead of assuming 0.

use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const MAX_FILE: u64 = 128;

/// A monotonic counter stored as `<dir>/<name>.a` and `<dir>/<name>.b`.
#[derive(Debug, Clone)]
pub struct MonotonicStore {
    first: PathBuf,
    second: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Copy {
    Missing,
    Damaged,
    Valid(u64),
}

fn encode(value: u64) -> String {
    let text = value.to_string();
    let digest = hex::encode(Sha256::digest(text.as_bytes()));
    format!("{text} {digest}\n")
}

fn decode(bytes: &[u8]) -> Option<u64> {
    let text = std::str::from_utf8(bytes).ok()?;
    let line = text.strip_suffix('\n')?;
    let (number, digest) = line.split_once(' ')?;
    let value: u64 = number.parse().ok()?;
    // Only the canonical spelling is accepted, so "007" or "+7" are damage.
    (encode(value) == text && digest.len() == 64).then_some(value)
}

fn read_copy(path: &Path) -> Result<Copy, UpdateError> {
    match read_bounded(path, MAX_FILE) {
        Ok(None) => Ok(Copy::Missing),
        Ok(Some(bytes)) => Ok(decode(&bytes).map_or(Copy::Damaged, Copy::Valid)),
        Err(e) if e.kind() == std::io::ErrorKind::InvalidData => Ok(Copy::Damaged),
        Err(e) => Err(e.into()),
    }
}

impl MonotonicStore {
    /// The store `name` inside `dir` (nothing is created until the first write).
    pub fn new(dir: &Path, name: &str) -> Self {
        Self {
            first: dir.join(format!("{name}.a")),
            second: dir.join(format!("{name}.b")),
        }
    }

    /// The stored value. `0` when neither file exists (first start);
    /// [`UpdateError::StateCorrupt`] when no valid copy is left although files exist.
    pub fn get(&self) -> Result<u64, UpdateError> {
        match (read_copy(&self.first)?, read_copy(&self.second)?) {
            (Copy::Valid(a), Copy::Valid(b)) => Ok(a.max(b)),
            (Copy::Valid(v), _) | (_, Copy::Valid(v)) => Ok(v),
            (Copy::Missing, Copy::Missing) => Ok(0),
            _ => Err(UpdateError::StateCorrupt),
        }
    }

    /// Raises the value to `value` if that is larger; never lowers it. Returns the value in
    /// force afterwards. Both copies are rewritten (the first, then the second), so a power
    /// loss in between leaves one copy with the new and one with the old value – reading then
    /// gives the new one.
    pub fn raise(&self, value: u64) -> Result<u64, UpdateError> {
        let current = self.get()?;
        let target = current.max(value);
        let text = encode(target);
        for path in [&self.first, &self.second] {
            if read_copy(path)? != Copy::Valid(target) {
                write_atomic(path, text.as_bytes())?;
            }
        }
        Ok(target)
    }

    /// The two file paths (for tests and diagnostics).
    pub fn paths(&self) -> [&Path; 2] {
        [&self.first, &self.second]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn store() -> (tempfile::TempDir, MonotonicStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = MonotonicStore::new(dir.path(), "rollback");
        (dir, store)
    }

    #[test]
    fn starts_at_zero_and_only_grows() {
        let (_dir, s) = store();
        assert_eq!(s.get().unwrap(), 0);
        assert_eq!(s.raise(5).unwrap(), 5);
        assert_eq!(s.raise(3).unwrap(), 5);
        assert_eq!(s.get().unwrap(), 5);
        assert_eq!(s.raise(u64::MAX).unwrap(), u64::MAX);
        assert_eq!(s.raise(1).unwrap(), u64::MAX);
    }

    #[test]
    fn one_damaged_copy_is_tolerated_and_repaired() {
        for victim in 0..2 {
            let (_dir, s) = store();
            s.raise(9).unwrap();
            fs::write(s.paths()[victim], b"0 deadbeef\n").unwrap();
            assert_eq!(s.get().unwrap(), 9, "never falls back to a lower value");
            assert_eq!(s.raise(4).unwrap(), 9);
            assert_eq!(fs::read(s.paths()[victim]).unwrap(), encode(9).as_bytes());
        }
    }

    #[test]
    fn one_missing_copy_is_tolerated() {
        let (_dir, s) = store();
        s.raise(7).unwrap();
        fs::remove_file(s.paths()[0]).unwrap();
        assert_eq!(s.get().unwrap(), 7);
    }

    #[test]
    fn stale_copy_never_wins() {
        let (_dir, s) = store();
        s.raise(2).unwrap();
        let old = fs::read(s.paths()[1]).unwrap();
        s.raise(8).unwrap();
        // Power loss between the two writes, or an attacker restoring one old file.
        fs::write(s.paths()[1], old).unwrap();
        assert_eq!(s.get().unwrap(), 8);
    }

    #[test]
    fn two_damaged_copies_fail_closed() {
        let (_dir, s) = store();
        s.raise(7).unwrap();
        fs::write(s.paths()[0], b"").unwrap();
        fs::write(s.paths()[1], b"7 00\n").unwrap();
        assert!(matches!(s.get(), Err(UpdateError::StateCorrupt)));
        assert!(matches!(s.raise(100), Err(UpdateError::StateCorrupt)));
        fs::remove_file(s.paths()[0]).unwrap();
        assert!(matches!(s.get(), Err(UpdateError::StateCorrupt)));
    }

    #[test]
    fn non_canonical_or_oversized_content_is_damage() {
        for bad in [&b"07 x\n"[..], b"7", b"\xff\xfe", &[b'1'; 400]] {
            let (_dir, s) = store();
            fs::write(s.paths()[0], bad).unwrap();
            s.raise(0).unwrap_err();
        }
        assert_eq!(decode(encode(42).as_bytes()), Some(42));
        let mut wrong = encode(42).into_bytes();
        wrong[0] = b'5';
        assert_eq!(decode(&wrong), None);
    }
}
