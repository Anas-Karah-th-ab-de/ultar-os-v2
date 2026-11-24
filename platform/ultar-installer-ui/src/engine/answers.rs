//! The user's choices, handed to the first start of the installed system: a small record in
//! `ULTAR-BOOTCTL` after the slot records (offset [`OFFSET`]): `"UIAN"`, format 1, length,
//! `key=value` lines (UTF-8, values with `\n`/`\\` escaped), CRC-32 of everything before.

use ultar_slots::crc32;

/// Offset of the record inside `ULTAR-BOOTCTL` (after the two tables and two slot records).
pub const OFFSET: usize = 128 * 1024;
/// Largest record.
pub const MAX_BYTES: usize = 16 * 1024;
/// Magic.
pub const MAGIC: &[u8; 4] = b"UIAN";
/// Longest single value in bytes.
pub const MAX_VALUE: usize = 256;

/// What the user chose.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Answers {
    /// Pairs in a fixed order.
    pub pairs: Vec<(String, String)>,
}

/// Why a record could not be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswersError {
    /// Not a record.
    Magic,
    /// Length beyond the limit or the buffer.
    Length,
    /// Checksum wrong.
    Checksum,
    /// Text not UTF-8 or a line without `=`.
    Body,
}

fn escape(v: &str) -> String {
    v.chars()
        .take(MAX_VALUE)
        .flat_map(|c| match c {
            '\\' => vec!['\\', '\\'],
            '\n' => vec!['\\', 'n'],
            c if c.is_control() => vec![],
            c => vec![c],
        })
        .collect()
}

fn unescape(v: &str) -> String {
    let mut out = String::new();
    let mut chars = v.chars();
    while let Some(c) = chars.next() {
        match (c, c == '\\') {
            (_, true) => match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => {}
            },
            (c, false) => out.push(c),
        }
    }
    out
}

impl Answers {
    /// Adds one choice (keys are ASCII identifiers chosen by the installer).
    pub fn set(&mut self, key: &str, value: &str) {
        self.pairs.retain(|(k, _)| k != key);
        self.pairs.push((key.to_owned(), value.to_owned()));
    }

    /// The value of a key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The record (at most [`MAX_BYTES`]; pairs that do not fit are left out).
    pub fn encode(&self) -> Vec<u8> {
        let mut body = String::new();
        for (k, v) in &self.pairs {
            let line = format!("{}={}\n", escape(k), escape(v));
            if 12 + body.len() + line.len() + 4 > MAX_BYTES {
                break;
            }
            body.push_str(&line);
        }
        let mut out = Vec::with_capacity(body.len() + 16);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(body.as_bytes());
        let crc = crc32(&out);
        out.extend_from_slice(&crc.to_le_bytes());
        out
    }

    /// Reads a record (hostile input: every length checked).
    pub fn decode(bytes: &[u8]) -> Result<Answers, AnswersError> {
        if bytes.len() < 16 || &bytes[..4] != MAGIC {
            return Err(AnswersError::Magic);
        }
        let len = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
        if len > MAX_BYTES - 16 || bytes.len() < 16 + len {
            return Err(AnswersError::Length);
        }
        let end = 12 + len;
        let crc = u32::from_le_bytes([bytes[end], bytes[end + 1], bytes[end + 2], bytes[end + 3]]);
        if crc32(&bytes[..end]) != crc {
            return Err(AnswersError::Checksum);
        }
        let text = std::str::from_utf8(&bytes[12..end]).map_err(|_| AnswersError::Body)?;
        let mut answers = Answers::default();
        for line in text.lines() {
            let (k, v) = line.split_once('=').ok_or(AnswersError::Body)?;
            answers.pairs.push((unescape(k), unescape(v)));
        }
        Ok(answers)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_hostile_input() {
        let mut a = Answers::default();
        a.set("lang", "ar");
        a.set("user", "أنس\nx\\y");
        a.set("lang", "en");
        let bytes = a.encode();
        let b = Answers::decode(&bytes).unwrap();
        assert_eq!(b.get("lang"), Some("en"));
        assert_eq!(b.get("user"), Some("أنس\nx\\y"));
        assert_eq!(Answers::decode(&bytes[..10]), Err(AnswersError::Magic));
        let mut bad = bytes.clone();
        bad[13] ^= 1;
        assert_eq!(Answers::decode(&bad), Err(AnswersError::Checksum));
        let mut long = bytes.clone();
        long[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(Answers::decode(&long), Err(AnswersError::Length));
        let mut seed = 0x2545_f491_u32;
        for _ in 0..2000 {
            let mut junk = bytes.clone();
            seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
            let i = seed as usize % junk.len();
            junk[i] = (seed >> 16) as u8;
            let _ = Answers::decode(&junk);
        }
    }

    #[test]
    fn record_is_bounded() {
        let mut a = Answers::default();
        for i in 0..500 {
            a.set(&format!("k{i}"), &"v".repeat(300));
        }
        assert!(a.encode().len() <= MAX_BYTES);
        assert!(Answers::decode(&a.encode()).is_ok());
    }
}
