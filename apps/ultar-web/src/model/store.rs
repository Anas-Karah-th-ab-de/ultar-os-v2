//! The file format of the browser's state (settings, history, site permissions): lines of
//! tab-separated fields with `\t`, `\n`, `\\` escaped. Files are written atomically (temporary
//! file, `fsync`, rename, `fsync` of the folder) and are read with limits; a damaged file is
//! reported, never half used.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;

/// Largest state file read (bytes).
pub const MAX_FILE: u64 = 4 * 1024 * 1024;
/// Most lines of a state file.
pub const MAX_LINES: usize = 20_000;
/// Most fields of one line.
pub const MAX_FIELDS: usize = 8;

/// Why a state file could not be read or written.
#[derive(Debug)]
pub enum StoreError {
    /// File system error.
    Io(std::io::Error),
    /// The file is larger than [`MAX_FILE`] or has more than [`MAX_LINES`] lines.
    TooLarge,
    /// Line `n` (1-based) is damaged (bad escape, invalid UTF-8, too many fields, bad value).
    Damaged(usize),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Io(e) => write!(f, "state file: {e}"),
            StoreError::TooLarge => f.write_str("state file too large"),
            StoreError::Damaged(n) => write!(f, "state file damaged at line {n}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> StoreError {
        StoreError::Io(e)
    }
}

/// Escapes one field.
pub fn escape(field: &str) -> String {
    let mut out = String::with_capacity(field.len());
    for c in field.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn unescape(field: &str) -> Option<String> {
    let mut out = String::with_capacity(field.len());
    let mut chars = field.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next()? {
            '\\' => '\\',
            't' => '\t',
            'n' => '\n',
            'r' => '\r',
            _ => return None,
        });
    }
    Some(out)
}

/// Parses the text of a state file into lines of fields (empty lines and `#` lines skipped).
pub fn parse(text: &str) -> Result<Vec<Vec<String>>, StoreError> {
    let mut rows = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if rows.len() >= MAX_LINES {
            return Err(StoreError::TooLarge);
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() > MAX_FIELDS {
            return Err(StoreError::Damaged(n + 1));
        }
        let row: Option<Vec<String>> = fields.into_iter().map(unescape).collect();
        rows.push(row.ok_or(StoreError::Damaged(n + 1))?);
    }
    Ok(rows)
}

/// Formats lines of fields.
pub fn format(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let line: Vec<String> = row.iter().map(|f| escape(f)).collect();
        out.push_str(&line.join("\t"));
        out.push('\n');
    }
    out
}

/// Reads a state file; a missing file is an empty one.
pub fn read(path: &Path) -> Result<Vec<Vec<String>>, StoreError> {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    if meta.len() > MAX_FILE {
        return Err(StoreError::TooLarge);
    }
    let bytes = fs::read(path)?;
    let text = String::from_utf8(bytes).map_err(|_| StoreError::Damaged(0))?;
    parse(&text)
}

/// Writes a state file atomically (mode 0600): it is either the old or the new file after a
/// power loss, never a mix.
pub fn write(path: &Path, rows: &[Vec<String>]) -> Result<(), StoreError> {
    let text = format(rows);
    if text.len() as u64 > MAX_FILE {
        return Err(StoreError::TooLarge);
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = dir.join(format!(".{name}.tmp{}", std::process::id()));
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
    }
    if let Err(e) = fs::rename(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(e.into());
    }
    fs::File::open(dir)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_escapes_and_atomic_write() {
        let rows = vec![
            vec!["a\tb".to_string(), "c\nd\\e\r".to_string()],
            vec!["عربي".to_string()],
        ];
        assert_eq!(parse(&format(&rows)).unwrap(), rows);
        let dir = std::env::temp_dir().join(format!("ultar-web-store-{}", std::process::id()));
        let path = dir.join("x.tsv");
        write(&path, &rows).unwrap();
        assert_eq!(read(&path).unwrap(), rows);
        assert!(read(&dir.join("missing")).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn damage_and_limits_are_reported() {
        assert!(matches!(parse("ok\nbad\\x\n"), Err(StoreError::Damaged(2))));
        assert!(matches!(parse("a\\"), Err(StoreError::Damaged(1))));
        assert!(matches!(
            parse(&"a\t".repeat(9)),
            Err(StoreError::Damaged(1))
        ));
        assert!(matches!(
            parse(&"x\n".repeat(MAX_LINES + 1)),
            Err(StoreError::TooLarge)
        ));
        assert!(parse("# comment\n\n").unwrap().is_empty());
        assert!(!StoreError::Damaged(3).to_string().is_empty());
    }
}
