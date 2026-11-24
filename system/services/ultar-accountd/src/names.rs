//! Account names, display names, and the names and ids of the system
//! (`/etc/ultar/ids.toml`), which no person's account may take.

use std::collections::BTreeSet;
use ultar_bus::messages::account_local::{MAX_DISPLAY_NAME, MAX_NAME};

/// A valid account name: 1–32 bytes, a lower-case letter first, then
/// lower-case letters, digits, `-`, `_`, `.` (as the authentication service).
pub fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && name.len() <= MAX_NAME
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '_' | '.'))
}

/// A valid display name: 1–128 bytes, not only spaces, no control or
/// invisible direction characters (they could make one name look like another).
pub fn valid_display(name: &str) -> bool {
    !name.trim().is_empty()
        && name.len() <= MAX_DISPLAY_NAME
        && !name.chars().any(|c| c.is_control() || is_format(c))
}

fn is_format(c: char) -> bool {
    matches!(c, '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' | '\u{feff}')
}

/// Names and ids the system uses.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SystemIds {
    /// Names of system users and groups.
    pub names: BTreeSet<String>,
    /// Their ids.
    pub ids: BTreeSet<u32>,
}

/// Why `ids.toml` was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdsError {
    /// Line number (1-based).
    pub line: usize,
}

impl std::fmt::Display for IdsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ids file: line {} not understood", self.line)
    }
}

impl std::error::Error for IdsError {}

/// Reads `ids.toml`: `[users]` and `[groups]` lines `name = id`; `[members]`
/// and `[ranges]` are skipped; anything else is an error (fail closed).
pub fn parse_ids(text: &str) -> Result<SystemIds, IdsError> {
    let mut out = SystemIds::default();
    let mut section = "";
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let bad = IdsError { line: n + 1 };
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = match name {
                "users" => "users",
                "groups" => "groups",
                "members" | "ranges" => "skip",
                _ => return Err(bad),
            };
            continue;
        }
        if section == "skip" {
            continue;
        }
        if section.is_empty() {
            return Err(bad);
        }
        let (name, id) = line.split_once('=').ok_or(bad)?;
        let id: u32 = id.trim().parse().map_err(|_| bad)?;
        out.names.insert(name.trim().to_owned());
        out.ids.insert(id);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_checked() {
        assert!(valid_name("anas") && valid_name("a.b-c_9"));
        for bad in ["", "Anas", "9a", "a b", "a/b", "ä", &"a".repeat(33)] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert!(valid_display("أنس قرة محمد") && valid_display("Leila"));
        for bad in ["", "  ", "a\nb", "ad\u{202e}min", "x\u{200b}"] {
            assert!(!valid_display(bad), "{bad:?}");
        }
        assert!(!valid_display(&"x".repeat(129)));
    }

    #[test]
    fn the_shipped_ids_file_reads() {
        let text = include_str!("../../../../rootfs/etc/ultar/ids.toml");
        let ids = parse_ids(text).unwrap();
        assert!(ids.names.contains("root") && ids.names.contains("seat"));
        assert!(ids.ids.contains(&0) && ids.ids.contains(&65534));
        assert!(ids.names.contains("accountd") && ids.names.contains("displayd"));
        for bad in ["x = 1", "[users]\nx = y", "[other]", "[users]\nnoequals"] {
            assert!(parse_ids(bad).is_err(), "{bad}");
        }
        assert!(!IdsError { line: 3 }.to_string().is_empty());
    }
}
