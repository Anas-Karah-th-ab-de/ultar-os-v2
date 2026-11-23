//! CRC-32 as used by GPT, zlib and the slot table.

/// CRC-32 (IEEE 802.3, reflected, as used by GPT and zlib).
pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(!0, data) ^ !0
}

/// Continues a CRC-32 over `data`; start with `!0` and finish with `^ !0`. Lets a caller
/// checksum data that arrives in pieces (a GPT entry array read sector by sector).
pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn pieces_give_the_same_crc() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let (a, b) = data.split_at(17);
        assert_eq!(crc32_update(crc32_update(!0, a), b) ^ !0, crc32(data));
    }
}
