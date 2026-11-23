//! The update history: a bounded log of what happened, for the "history" view.
//!
//! One line per entry: `<unix time> <kind> <version or -> <message key or ->`. The file is
//! rewritten atomically and never holds more than [`MAX_ENTRIES`] entries.

use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use crate::version::Version;
use std::path::Path;

/// Most entries kept; the oldest are dropped.
pub const MAX_ENTRIES: usize = 200;
const MAX_FILE: u64 = 64 * 1024;

/// What happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryKind {
    /// A bundle was downloaded or copied and verified.
    Staged,
    /// A release was written to the other slot; a restart is needed.
    Applied,
    /// A release started successfully and is now the system.
    Installed,
    /// A release did not start; the previous version is running again.
    RolledBack,
    /// An operation failed (the message key says why).
    Failed,
    /// The user chose to start the previous version.
    PreviousSelected,
    /// A pending update was cancelled.
    Cancelled,
    /// The set of trusted update keys was replaced.
    KeysRotated,
}

impl HistoryKind {
    const ALL: [HistoryKind; 8] = [
        Self::Staged,
        Self::Applied,
        Self::Installed,
        Self::RolledBack,
        Self::Failed,
        Self::PreviousSelected,
        Self::Cancelled,
        Self::KeysRotated,
    ];

    /// Stable word used in the file and as the last segment of the message key.
    pub fn name(self) -> &'static str {
        match self {
            Self::Staged => "staged",
            Self::Applied => "applied",
            Self::Installed => "installed",
            Self::RolledBack => "rolled_back",
            Self::Failed => "failed",
            Self::PreviousSelected => "previous_selected",
            Self::Cancelled => "cancelled",
            Self::KeysRotated => "keys_rotated",
        }
    }

    /// i18n key of the entry's title.
    pub fn key(self) -> &'static str {
        match self {
            Self::Staged => "update.history.staged",
            Self::Applied => "update.history.applied",
            Self::Installed => "update.history.installed",
            Self::RolledBack => "update.history.rolled_back",
            Self::Failed => "update.history.failed",
            Self::PreviousSelected => "update.history.previous_selected",
            Self::Cancelled => "update.history.cancelled",
            Self::KeysRotated => "update.history.keys_rotated",
        }
    }
}

/// One history entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    /// When (seconds since 1970, device time).
    pub time: u64,
    /// What.
    pub kind: HistoryKind,
    /// Which release, when one is concerned.
    pub version: Option<Version>,
    /// i18n key with the reason (errors), when there is one.
    pub detail_key: Option<String>,
}

fn key_ok(key: &str) -> bool {
    !key.is_empty()
        && key.len() <= 96
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

impl HistoryEntry {
    fn encode(&self) -> String {
        let version = self
            .version
            .map_or_else(|| "-".to_string(), |v| v.to_string());
        let detail = self
            .detail_key
            .as_deref()
            .filter(|k| key_ok(k))
            .unwrap_or("-");
        format!("{} {} {version} {detail}\n", self.time, self.kind.name())
    }

    fn decode(line: &str) -> Option<Self> {
        let mut words = line.split(' ');
        let time = words.next()?.parse().ok()?;
        let name = words.next()?;
        let kind = HistoryKind::ALL.into_iter().find(|k| k.name() == name)?;
        let version = match words.next()? {
            "-" => None,
            text => Some(Version::parse(text)?),
        };
        let detail_key = match words.next()? {
            "-" => None,
            key if key_ok(key) => Some(key.to_string()),
            _ => return None,
        };
        words.next().is_none().then_some(Self {
            time,
            kind,
            version,
            detail_key,
        })
    }
}

/// Reads the history, oldest first. Unreadable lines are skipped; a missing or oversized file
/// gives an empty history.
pub fn load(path: &Path) -> Vec<HistoryEntry> {
    let bytes = read_bounded(path, MAX_FILE)
        .ok()
        .flatten()
        .unwrap_or_default();
    String::from_utf8_lossy(&bytes)
        .lines()
        .filter_map(HistoryEntry::decode)
        .collect()
}

/// Appends an entry, dropping the oldest ones beyond [`MAX_ENTRIES`].
pub fn append(path: &Path, entry: &HistoryEntry) -> Result<(), UpdateError> {
    let mut entries = load(path);
    entries.push(entry.clone());
    let skip = entries.len().saturating_sub(MAX_ENTRIES);
    let text: String = entries[skip..].iter().map(HistoryEntry::encode).collect();
    Ok(write_atomic(path, text.as_bytes())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(time: u64, kind: HistoryKind) -> HistoryEntry {
        HistoryEntry {
            time,
            kind,
            version: Some(Version::new(0, 7, 1)),
            detail_key: None,
        }
    }

    #[test]
    fn append_and_load_every_kind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        assert!(load(&path).is_empty());
        for (i, kind) in HistoryKind::ALL.into_iter().enumerate() {
            let mut e = entry(i as u64, kind);
            if kind == HistoryKind::Failed {
                e.detail_key = Some("update.error.expired".into());
                e.version = None;
            }
            append(&path, &e).unwrap();
            assert!(kind.key().ends_with(kind.name()));
        }
        let all = load(&path);
        assert_eq!(all.len(), 8);
        assert_eq!(all[4].detail_key.as_deref(), Some("update.error.expired"));
        assert_eq!(all[4].version, None);
        assert_eq!(all[0].version, Some(Version::new(0, 7, 1)));
    }

    #[test]
    fn bounded_and_oldest_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        for time in 0..(MAX_ENTRIES as u64 + 25) {
            append(&path, &entry(time, HistoryKind::Staged)).unwrap();
        }
        let all = load(&path);
        assert_eq!(all.len(), MAX_ENTRIES);
        assert_eq!(all[0].time, 25);
        assert!(std::fs::metadata(&path).unwrap().len() < MAX_FILE);
    }

    #[test]
    fn hostile_content_is_skipped_and_unsafe_keys_are_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history");
        std::fs::write(
            &path,
            b"garbage\n5 staged 0.7.1 -\n7 nope - -\n9 failed - a b\n\xff\n",
        )
        .unwrap();
        let all = load(&path);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].time, 5);
        let mut e = entry(1, HistoryKind::Failed);
        e.detail_key = Some("two words\ninjected 1 staged - -".into());
        append(&path, &e).unwrap();
        let all = load(&path);
        assert_eq!(all.len(), 2);
        assert_eq!(all[1].detail_key, None);
    }
}
