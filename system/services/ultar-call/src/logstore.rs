//! The call log on disk: one text file, mode 0600, written atomically (temp file, fsync,
//! rename, fsync of the directory), at most [`MAX_FILE`] bytes. Corrupt lines are skipped and
//! counted; a file that is too large or has no header is not read (the log starts empty and
//! the problem is reported).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use ultar_bus::messages::call::{
    check_number, Direction, LogEntry, Way, MAX_BLOCKED, MAX_LOG, MAX_NAME_LEN, MAX_NUMBER_LEN,
};

/// First line of the file.
const HEADER: &str = "ULTAR-CALLLOG 1";
/// Largest file read or written (entries and blocked numbers at their limits fit).
pub const MAX_FILE: u64 = 128 * 1024;

/// Failure of the store.
#[derive(Debug)]
pub enum StoreError {
    /// Reading or writing failed (errno).
    Io(std::io::ErrorKind),
    /// The file is larger than [`MAX_FILE`] or has no valid header.
    Corrupt,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(k) => write!(f, "call log store: {k:?}"),
            StoreError::Corrupt => f.write_str("call log store: corrupt file"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e.kind())
    }
}

/// What the file holds.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Stored {
    /// Entries, oldest first.
    pub entries: Vec<LogEntry>,
    /// Blocked numbers (canonical).
    pub blocked: Vec<String>,
    /// Lines that could not be read.
    pub bad_lines: usize,
}

fn clean(field: &str, max: usize) -> String {
    let s: String = field
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_owned()
}

fn way_code(way: Way) -> String {
    match way {
        Way::Ultar => "U".into(),
        Way::Cellular(slot) => format!("C{slot}"),
    }
}

/// The file contents for `entries` and `blocked`.
pub fn encode(entries: &[LogEntry], blocked: &[String]) -> String {
    let mut out = format!("{HEADER}\n");
    for e in entries {
        let dir = if e.direction == Direction::Incoming {
            "I"
        } else {
            "O"
        };
        out.push_str(&format!(
            "E\t{}\t{}\t{dir}\t{}\t{}\t{}\t{}\n",
            e.id,
            way_code(e.way),
            u8::from(e.missed),
            e.when_s,
            clean(&e.number, MAX_NUMBER_LEN),
            clean(e.name.as_deref().unwrap_or(""), MAX_NAME_LEN),
        ));
    }
    for b in blocked {
        out.push_str(&format!("B\t{}\n", clean(b, MAX_NUMBER_LEN)));
    }
    out
}

fn entry(f: &[&str]) -> Option<LogEntry> {
    let [id, way, dir, missed, when, number, name] = f else {
        return None;
    };
    let way = match *way {
        "U" => Way::Ultar,
        "C1" => Way::Cellular(1),
        "C2" => Way::Cellular(2),
        _ => return None,
    };
    let direction = match *dir {
        "I" => Direction::Incoming,
        "O" => Direction::Outgoing,
        _ => return None,
    };
    check_number(number).ok()?;
    Some(LogEntry {
        id: id.parse().ok()?,
        way,
        direction,
        missed: match *missed {
            "0" => false,
            "1" => true,
            _ => return None,
        },
        number: (*number).to_owned(),
        name: (!name.is_empty() && name.len() <= MAX_NAME_LEN).then(|| (*name).to_owned()),
        when_s: when.parse().ok()?,
    })
}

/// Reads file contents; bad lines are skipped and counted, limits are kept.
pub fn decode(text: &str) -> Result<Stored, StoreError> {
    let mut lines = text.lines();
    if lines.next() != Some(HEADER) {
        return Err(StoreError::Corrupt);
    }
    let mut stored = Stored::default();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        match f.split_first() {
            Some((&"E", rest)) if stored.entries.len() < MAX_LOG => match entry(rest) {
                Some(e) => stored.entries.push(e),
                None => stored.bad_lines += 1,
            },
            Some((&"B", [n])) if stored.blocked.len() < MAX_BLOCKED && check_number(n).is_ok() => {
                stored.blocked.push((*n).to_owned());
            }
            _ => stored.bad_lines += 1,
        }
    }
    Ok(stored)
}

/// Reads the file at `path` (a missing file is an empty log).
pub fn load(path: &Path) -> Result<Stored, StoreError> {
    let mut file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Stored::default()),
        Err(e) => return Err(e.into()),
    };
    if file.metadata()?.len() > MAX_FILE {
        return Err(StoreError::Corrupt);
    }
    let mut text = String::new();
    Read::by_ref(&mut file)
        .take(MAX_FILE)
        .read_to_string(&mut text)?;
    decode(&text)
}

/// Writes the file atomically with mode 0600.
pub fn save(path: &Path, entries: &[LogEntry], blocked: &[String]) -> Result<(), StoreError> {
    let text = encode(entries, blocked);
    if text.len() as u64 > MAX_FILE {
        return Err(StoreError::Corrupt);
    }
    let tmp = path.with_extension("tmp");
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()?;
    fs::rename(&tmp, path)?;
    if let Some(dir) = path.parent() {
        File::open(dir)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn e(id: u32, name: Option<&str>) -> LogEntry {
        LogEntry {
            id,
            way: Way::Cellular(2),
            direction: Direction::Incoming,
            missed: true,
            number: "+963 11 555".into(),
            name: name.map(str::to_owned),
            when_s: 1_763_800_000,
        }
    }

    #[test]
    fn round_trip_mode_and_atomic_file() {
        let dir = std::env::temp_dir().join(format!("calllog-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("calls.log");
        let entries = vec![e(1, Some("ليلى\tحداد")), e(2, None)];
        save(&path, &entries, &["0955".into()]).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let back = load(&path).unwrap();
        assert_eq!(
            back.entries[0].name.as_deref(),
            Some("ليلى حداد"),
            "tabs never split a line"
        );
        assert_eq!(back.entries[1], entries[1]);
        assert_eq!(back.blocked, vec!["0955".to_string()]);
        assert!(!path.with_extension("tmp").exists());
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(load(&dir.join("none")).unwrap(), Stored::default());
    }

    #[test]
    fn corrupt_input_is_detected_with_a_fixed_seed_fuzz() {
        assert!(matches!(decode("nonsense"), Err(StoreError::Corrupt)));
        let good = encode(&[e(1, None)], &[]);
        let bad = format!("{good}E\t1\tX\nB\tabc\nZ\n");
        let s = decode(&bad).unwrap();
        assert_eq!((s.entries.len(), s.bad_lines), (1, 3));
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for _ in 0..2000 {
            let mut bytes = good.clone().into_bytes();
            for _ in 0..4 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                let i = (seed % bytes.len() as u64) as usize;
                bytes[i] = (seed >> 32) as u8;
            }
            if let Ok(text) = String::from_utf8(bytes) {
                if let Ok(s) = decode(&text) {
                    assert!(s.entries.len() <= MAX_LOG && s.blocked.len() <= MAX_BLOCKED);
                }
            }
        }
    }
}
