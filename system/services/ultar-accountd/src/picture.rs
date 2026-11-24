//! Checks of an own picture: only PNG and JPEG with a sane size are kept.
//! The bytes come from an app and are hostile; nothing here decodes pixels
//! (the shell's image decoder does, inside its own limits).

/// Largest side accepted (pixels).
pub const MAX_SIDE: u32 = 4096;

fn png(b: &[u8]) -> bool {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if b.len() < 33 || b[..8] != SIGNATURE || &b[12..16] != b"IHDR" {
        return false;
    }
    let w = u32::from_be_bytes([b[16], b[17], b[18], b[19]]);
    let h = u32::from_be_bytes([b[20], b[21], b[22], b[23]]);
    (1..=MAX_SIDE).contains(&w)
        && (1..=MAX_SIDE).contains(&h)
        && b.ends_with(&[0xae, 0x42, 0x60, 0x82])
}

fn jpeg(b: &[u8]) -> bool {
    b.len() >= 4 && b.starts_with(&[0xff, 0xd8, 0xff]) && b.ends_with(&[0xff, 0xd9])
}

/// True for a whole PNG (signature, header within bounds, end chunk) or a
/// whole JPEG (start and end markers).
pub fn valid(bytes: &[u8]) -> bool {
    png(bytes) || jpeg(bytes)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tiny PNG-shaped byte string (header 2×2, end chunk).
    pub fn png_bytes() -> Vec<u8> {
        let mut b = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 13];
        b.extend_from_slice(b"IHDR");
        b.extend_from_slice(&2u32.to_be_bytes());
        b.extend_from_slice(&2u32.to_be_bytes());
        b.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        b.extend_from_slice(&[0, 0, 0, 0]);
        b.extend_from_slice(b"IEND");
        b.extend_from_slice(&[0xae, 0x42, 0x60, 0x82]);
        b
    }

    #[test]
    fn pictures_are_checked() {
        assert!(valid(&png_bytes()));
        assert!(valid(&[0xff, 0xd8, 0xff, 0xe0, 0, 0, 0xff, 0xd9]));
        let mut huge = png_bytes();
        huge[16..20].copy_from_slice(&5000u32.to_be_bytes());
        assert!(!valid(&huge));
        let cut = png_bytes();
        assert!(!valid(&cut[..cut.len() - 1]));
        assert!(!valid(b"<svg/>") && !valid(&[]) && !valid(&[0xff, 0xd8, 0xff]));
        let mut x = 7u64;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let mut b = png_bytes();
            let at = (x % b.len() as u64) as usize;
            b[at] = (x >> 32) as u8;
            let _ = valid(&b);
        }
    }
}
