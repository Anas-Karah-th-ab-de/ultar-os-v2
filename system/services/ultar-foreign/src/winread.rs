//! Bounded reading of Windows files (`pe.rs`, `msi.rs`): reads at an offset that never
//! go past the end, little-endian numbers, UTF-16 text.

/// Something to read at offsets: a file or bytes in memory.
pub trait ReadAt {
    /// Its length in bytes.
    fn len(&self) -> u64;
    /// Whether it has no bytes.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// `n` bytes at `at`; `None` when they are not all there (or `n` is unreasonable).
    fn bytes(&self, at: u64, n: usize) -> Option<Vec<u8>>;

    /// A little-endian `u16` at `at`.
    fn u16_at(&self, at: u64) -> Option<u16> {
        let b = self.bytes(at, 2)?;
        Some(u16::from_le_bytes([b[0], b[1]]))
    }

    /// A little-endian `u32` at `at`.
    fn u32_at(&self, at: u64) -> Option<u32> {
        let b = self.bytes(at, 4)?;
        Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}

/// Most bytes one read may ask for.
const MAX_READ: usize = 32 << 20;

impl ReadAt for &[u8] {
    fn len(&self) -> u64 {
        <[u8]>::len(self) as u64
    }

    fn bytes(&self, at: u64, n: usize) -> Option<Vec<u8>> {
        let at = usize::try_from(at).ok()?;
        (n <= MAX_READ).then_some(())?;
        self.get(at..at.checked_add(n)?).map(<[u8]>::to_vec)
    }
}

impl ReadAt for std::fs::File {
    fn len(&self) -> u64 {
        self.metadata().map(|m| m.len()).unwrap_or(0)
    }

    fn bytes(&self, at: u64, n: usize) -> Option<Vec<u8>> {
        use std::os::unix::fs::FileExt;
        if n > MAX_READ || at.checked_add(n as u64)? > ReadAt::len(self) {
            return None;
        }
        let mut buf = vec![0u8; n];
        self.read_exact_at(&mut buf, at).ok()?;
        Some(buf)
    }
}

/// A zero-terminated UTF-16LE text at byte `at` of `b`, at most `max` characters; the
/// text and the byte index after its terminator (or after the last character read when
/// `b` ends first).
pub fn utf16_z(b: &[u8], at: usize, max: usize) -> Option<(String, usize)> {
    let mut units = Vec::new();
    let mut i = at;
    while let Some(pair) = b.get(i..i.checked_add(2)?) {
        i += 2;
        let unit = u16::from_le_bytes([pair[0], pair[1]]);
        if unit == 0 {
            break;
        }
        if units.len() == max {
            return None;
        }
        units.push(unit);
    }
    Some((String::from_utf16_lossy(&units), i))
}

/// A tiny fixed-seed generator for the fuzz tests (xorshift).
#[cfg(test)]
pub struct Rng(pub u64);

#[cfg(test)]
impl Rng {
    /// The next number.
    pub fn step(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    /// A number below `n` (`n` > 0).
    pub fn below(&mut self, n: usize) -> usize {
        (self.step() % n.max(1) as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_stay_inside() {
        let b: &[u8] = &[1, 2, 3, 4, 5];
        assert_eq!(b.u16_at(0), Some(0x0201));
        assert_eq!(b.u32_at(1), Some(0x0504_0302));
        assert_eq!(b.u32_at(2), None);
        assert_eq!(b.bytes(u64::MAX, 2), None);
        assert_eq!(b.bytes(0, MAX_READ + 1), None);
        let text: Vec<u8> = "Hi"
            .encode_utf16()
            .chain([0, 9])
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(utf16_z(&text, 0, 8), Some(("Hi".into(), 6)));
        assert_eq!(utf16_z(&text, 0, 1), None);
        assert_eq!(utf16_z(&text[..3], 0, 8), Some(("H".into(), 2)));
    }

    #[test]
    fn a_file_reads_like_bytes() {
        let path = std::env::temp_dir().join(format!("ufw-{}", std::process::id()));
        std::fs::write(&path, [9u8, 8, 7]).unwrap();
        let f = std::fs::File::open(&path).unwrap();
        assert_eq!(ReadAt::len(&f), 3);
        assert_eq!(f.bytes(1, 2), Some(vec![8, 7]));
        assert_eq!(f.bytes(2, 2), None);
        let _ = std::fs::remove_file(&path);
    }
}
