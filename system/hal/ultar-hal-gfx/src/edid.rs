//! Reading a display's identification data (EDID 1.x base block): who made
//! it, which product, which unit, its name and size. The bytes come from the
//! display over a cable and are treated as hostile: every field is checked,
//! anything unexpected gives `None` (the display then counts as one without
//! data and is told apart by its connector).

/// Length of the EDID base block.
pub const BLOCK: usize = 128;
const HEADER: [u8; 8] = [0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x00];

/// What a display says about itself.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct EdidInfo {
    /// Three capital letters (`DEL`, `BOE`).
    pub manufacturer: String,
    /// Product code.
    pub product: u16,
    /// Serial number from the base block (0 = none).
    pub serial: u32,
    /// Product name from the name descriptor (printable ASCII, ≤ 13 characters).
    pub name: String,
    /// Serial number text from the serial descriptor (printable ASCII, ≤ 13).
    pub serial_text: String,
    /// Image size in millimetres (from the centimetre fields; 0 = unknown).
    pub size_mm: (u32, u32),
}

fn manufacturer(b0: u8, b1: u8) -> Option<String> {
    let word = u16::from_be_bytes([b0, b1]);
    if word & 0x8000 != 0 {
        return None;
    }
    [(word >> 10) & 0x1f, (word >> 5) & 0x1f, word & 0x1f]
        .iter()
        .map(|&v| {
            (1..=26)
                .contains(&v)
                .then(|| char::from(b'A' + v as u8 - 1))
        })
        .collect()
}

fn descriptor_text(d: &[u8]) -> String {
    d.iter()
        .take_while(|&&b| b != 0x0a && b != 0)
        .filter(|b| b.is_ascii_graphic() || **b == b' ')
        .map(|&b| char::from(b))
        .collect::<String>()
        .trim()
        .to_owned()
}

/// Parses an EDID base block (longer input: extension blocks are ignored).
/// `None` when the header, the checksum or the manufacturer is wrong.
pub fn parse(bytes: &[u8]) -> Option<EdidInfo> {
    let block = bytes.get(..BLOCK)?;
    if block[..8] != HEADER {
        return None;
    }
    if block.iter().fold(0u8, |sum, b| sum.wrapping_add(*b)) != 0 {
        return None;
    }
    let mut info = EdidInfo {
        manufacturer: manufacturer(block[8], block[9])?,
        product: u16::from_le_bytes([block[10], block[11]]),
        serial: u32::from_le_bytes([block[12], block[13], block[14], block[15]]),
        size_mm: (u32::from(block[21]) * 10, u32::from(block[22]) * 10),
        ..EdidInfo::default()
    };
    for start in [54usize, 72, 90, 108] {
        let d = &block[start..start + 18];
        if d[0] != 0 || d[1] != 0 || d[2] != 0 {
            continue; // a timing descriptor
        }
        match d[3] {
            0xfc => info.name = descriptor_text(&d[5..18]),
            0xff => info.serial_text = descriptor_text(&d[5..18]),
            _ => {}
        }
    }
    Some(info)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A valid block for "DEL" 0xa0c4 serial 0x01020304 named "DELL U2422H".
    pub fn sample() -> Vec<u8> {
        let mut b = vec![0u8; BLOCK];
        b[..8].copy_from_slice(&HEADER);
        // D=4 E=5 L=12 -> 00100 00101 01100
        let word: u16 = (4 << 10) | (5 << 5) | 12;
        b[8..10].copy_from_slice(&word.to_be_bytes());
        b[10..12].copy_from_slice(&0xa0c4u16.to_le_bytes());
        b[12..16].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        b[21] = 53;
        b[22] = 30;
        b[54] = 0x01; // a timing descriptor
        b[72..77].copy_from_slice(&[0, 0, 0, 0xfc, 0]);
        b[77..90].copy_from_slice(b"DELL U2422H\n ");
        b[90..95].copy_from_slice(&[0, 0, 0, 0xff, 0]);
        b[95..108].copy_from_slice(b"7Z1XB\n       ");
        let sum = b[..127].iter().fold(0u8, |s, v| s.wrapping_add(*v));
        b[127] = 0u8.wrapping_sub(sum);
        b
    }

    #[test]
    fn a_real_looking_block_is_read() {
        let info = parse(&sample()).unwrap();
        assert_eq!(info.manufacturer, "DEL");
        assert_eq!(info.product, 0xa0c4);
        assert_eq!(info.serial, 0x0102_0304);
        assert_eq!(info.name, "DELL U2422H");
        assert_eq!(info.serial_text, "7Z1XB");
        assert_eq!(info.size_mm, (530, 300));
    }

    #[test]
    fn broken_blocks_are_refused() {
        let good = sample();
        assert!(parse(&good[..100]).is_none());
        let mut bad = good.clone();
        bad[0] = 1;
        assert!(parse(&bad).is_none());
        let mut bad = good.clone();
        bad[30] ^= 1;
        assert!(parse(&bad).is_none(), "checksum");
        let mut bad = good.clone();
        bad[8] = 0x80;
        bad[127] = bad[127].wrapping_sub(0x80 - good[8]);
        assert!(parse(&bad).is_none(), "reserved bit");
    }

    #[test]
    fn random_bytes_never_panic_and_names_stay_printable() {
        let mut x: u64 = 0x5eed_1234;
        for _ in 0..4000 {
            let mut b = sample();
            for _ in 0..6 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let at = (x % 127) as usize;
                b[at] = (x >> 20) as u8;
            }
            let sum = b[..127].iter().fold(0u8, |s, v| s.wrapping_add(*v));
            b[127] = 0u8.wrapping_sub(sum);
            if let Some(info) = parse(&b) {
                assert!(info.name.len() <= 13 && info.serial_text.len() <= 13);
                assert!(info.name.bytes().all(|c| c.is_ascii_graphic() || c == b' '));
                assert!(info.manufacturer.bytes().all(|c| c.is_ascii_uppercase()));
            }
        }
    }
}
