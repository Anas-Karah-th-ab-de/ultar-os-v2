//! The on-disk format of a settings file: a checksum line followed by TOML.
//!
//! ```text
//! #ultar-settings crc32=1c291ca3
//! schema = 1
//!
//! [values]
//! "appearance.theme" = "light"
//!
//! [combo.c-0f3a…]
//! displays = ["edp-1", "del-u2720q-7"]
//! used = 4
//!
//! [combo.c-0f3a….values]
//! "display.mode" = "extend"
//! ```
//!
//! The checksum covers every byte after the first line, so a file that was
//! cut short or altered is recognised even when the rest is valid TOML.

use crate::value::Value;
use std::collections::BTreeMap;
use std::fmt;

const HEADER_PREFIX: &str = "#ultar-settings crc32=";

/// Values remembered for one combination of connected displays.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComboData {
    /// Identifiers of the displays of the combination, sorted.
    pub displays: Vec<String>,
    /// Use counter: the combination used last has the highest number.
    pub used: u64,
    /// Stored values by concrete key.
    pub values: BTreeMap<String, Value>,
}

/// Everything one settings file holds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FileContent {
    /// Schema version the values were written with.
    pub schema: u32,
    /// Stored values by concrete key.
    pub values: BTreeMap<String, Value>,
    /// Values per display combination (display file only).
    pub combos: BTreeMap<String, ComboData>,
}

/// Why file content was not accepted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// The first line is not the checksum line.
    MissingHeader,
    /// The checksum does not match the content.
    Checksum,
    /// The content is not valid TOML.
    Syntax,
    /// Valid TOML that does not have the layout of a settings file.
    Shape,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            DecodeError::MissingHeader => "checksum line missing",
            DecodeError::Checksum => "checksum mismatch",
            DecodeError::Syntax => "not valid TOML",
            DecodeError::Shape => "unexpected layout",
        })
    }
}

impl std::error::Error for DecodeError {}

/// CRC-32 (IEEE 802.3, reflected) of `bytes`.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn values_table(values: &BTreeMap<String, Value>) -> toml::Table {
    values
        .iter()
        .map(|(key, value)| (key.clone(), value.to_toml()))
        .collect()
}

fn body_table(content: &FileContent) -> toml::Table {
    let mut root = toml::Table::new();
    root.insert("schema".into(), toml::Value::Integer(content.schema.into()));
    root.insert(
        "values".into(),
        toml::Value::Table(values_table(&content.values)),
    );
    if !content.combos.is_empty() {
        let mut combos = toml::Table::new();
        for (id, combo) in &content.combos {
            let mut table = toml::Table::new();
            let displays = combo.displays.iter().cloned().map(toml::Value::String);
            table.insert("displays".into(), toml::Value::Array(displays.collect()));
            let used = i64::try_from(combo.used).unwrap_or(i64::MAX);
            table.insert("used".into(), toml::Value::Integer(used));
            table.insert(
                "values".into(),
                toml::Value::Table(values_table(&combo.values)),
            );
            combos.insert(id.clone(), toml::Value::Table(table));
        }
        root.insert("combo".into(), toml::Value::Table(combos));
    }
    root
}

/// Serialises file content, checksum line first.
pub fn encode(content: &FileContent) -> String {
    let body = body_table(content).to_string();
    format!("{HEADER_PREFIX}{:08x}\n{body}", crc32(body.as_bytes()))
}

fn read_values(table: Option<&toml::Value>) -> Result<BTreeMap<String, Value>, DecodeError> {
    let Some(table) = table else {
        return Ok(BTreeMap::new());
    };
    let table = table.as_table().ok_or(DecodeError::Shape)?;
    table
        .iter()
        .map(|(key, value)| {
            Value::from_toml(value)
                .map(|value| (key.clone(), value))
                .ok_or(DecodeError::Shape)
        })
        .collect()
}

fn read_combo(table: &toml::Value) -> Result<ComboData, DecodeError> {
    let table = table.as_table().ok_or(DecodeError::Shape)?;
    let displays = match table.get("displays") {
        Some(list) => match Value::from_toml(list) {
            Some(Value::List(displays)) => displays,
            _ => return Err(DecodeError::Shape),
        },
        None => Vec::new(),
    };
    let used = match table.get("used") {
        Some(value) => value
            .as_integer()
            .and_then(|used| u64::try_from(used).ok())
            .ok_or(DecodeError::Shape)?,
        None => 0,
    };
    Ok(ComboData {
        displays,
        used,
        values: read_values(table.get("values"))?,
    })
}

/// Checks the checksum line and reads the content.
pub fn decode(text: &str) -> Result<FileContent, DecodeError> {
    let (header, body) = text.split_once('\n').ok_or(DecodeError::MissingHeader)?;
    let digits = header
        .strip_prefix(HEADER_PREFIX)
        .ok_or(DecodeError::MissingHeader)?;
    let strict_hex = digits.len() == 8
        && digits
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !strict_hex {
        return Err(DecodeError::MissingHeader);
    }
    let expected = u32::from_str_radix(digits, 16).map_err(|_| DecodeError::MissingHeader)?;
    if crc32(body.as_bytes()) != expected {
        return Err(DecodeError::Checksum);
    }
    let root: toml::Table = body.parse().map_err(|_| DecodeError::Syntax)?;
    let schema = root
        .get("schema")
        .and_then(toml::Value::as_integer)
        .and_then(|schema| u32::try_from(schema).ok())
        .filter(|schema| *schema >= 1)
        .ok_or(DecodeError::Shape)?;
    let mut combos = BTreeMap::new();
    if let Some(table) = root.get("combo") {
        for (id, combo) in table.as_table().ok_or(DecodeError::Shape)? {
            combos.insert(id.clone(), read_combo(combo)?);
        }
    }
    Ok(FileContent {
        schema,
        values: read_values(root.get("values"))?,
        combos,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> FileContent {
        let mut content = FileContent {
            schema: 2,
            ..FileContent::default()
        };
        content
            .values
            .insert("appearance.theme".into(), Value::text("light"));
        content.values.insert("power.x".into(), Value::Int(-3));
        content.values.insert("a.float".into(), Value::Float(1.5));
        content
            .values
            .insert("a.list".into(), Value::list(&["x", "y"]));
        content.values.insert("a.flag".into(), Value::Bool(true));
        let mut combo = ComboData {
            displays: vec!["edp-1".into(), "hdmi".into()],
            used: 7,
            ..ComboData::default()
        };
        combo
            .values
            .insert("display.edp-1.scale".into(), Value::Int(125));
        content.combos.insert("c-1".into(), combo);
        content
    }

    #[test]
    fn crc_reference_values() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(
            crc32(b"The quick brown fox jumps over the lazy dog"),
            0x414F_A339
        );
    }

    #[test]
    fn round_trip() {
        let content = sample();
        let text = encode(&content);
        assert!(text.starts_with(HEADER_PREFIX));
        assert_eq!(decode(&text), Ok(content));
        let empty = FileContent {
            schema: 1,
            ..FileContent::default()
        };
        assert_eq!(decode(&encode(&empty)), Ok(empty));
    }

    #[test]
    fn encoding_is_deterministic() {
        assert_eq!(encode(&sample()), encode(&sample()));
    }

    #[test]
    fn header_problems() {
        let text = encode(&sample());
        assert_eq!(decode(""), Err(DecodeError::MissingHeader));
        assert_eq!(decode("schema = 1\n"), Err(DecodeError::MissingHeader));
        assert_eq!(
            decode(&text.replacen("#ultar", "#Ultar", 1)),
            Err(DecodeError::MissingHeader)
        );
        let upper = text.replacen("crc32=", "crc32=G", 1);
        assert_eq!(decode(&upper), Err(DecodeError::MissingHeader));
        let header_only = text.lines().next().unwrap();
        assert_eq!(decode(header_only), Err(DecodeError::MissingHeader));
    }

    #[test]
    fn checksum_catches_cut_and_changed_content() {
        let text = encode(&sample());
        let cut = &text[..text.rfind('\n').unwrap()];
        assert_eq!(decode(cut), Err(DecodeError::Checksum));
        assert_eq!(
            decode(&text.replace("light", "night")),
            Err(DecodeError::Checksum)
        );
    }

    fn with_valid_checksum(body: &str) -> String {
        format!("{HEADER_PREFIX}{:08x}\n{body}", crc32(body.as_bytes()))
    }

    #[test]
    fn syntax_and_shape_problems_behind_a_valid_checksum() {
        assert_eq!(
            decode(&with_valid_checksum("schema = ")),
            Err(DecodeError::Syntax)
        );
        for body in [
            "",
            "schema = 0\n",
            "schema = \"1\"\n",
            "schema = 1\nvalues = 3\n",
            "schema = 1\n[values]\nk = { a = 1 }\n",
            "schema = 1\n[values]\nk = [1, 2]\n",
            "schema = 1\ncombo = 1\n",
            "schema = 1\n[combo]\nc = 1\n",
            "schema = 1\n[combo.c]\ndisplays = 1\n",
            "schema = 1\n[combo.c]\nused = -1\n",
        ] {
            assert_eq!(
                decode(&with_valid_checksum(body)),
                Err(DecodeError::Shape),
                "{body}"
            );
        }
        let minimal = decode(&with_valid_checksum("schema = 1\n[combo.c]\n")).unwrap();
        assert_eq!(minimal.combos["c"], ComboData::default());
        assert!(!DecodeError::Shape.to_string().is_empty());
    }
}
