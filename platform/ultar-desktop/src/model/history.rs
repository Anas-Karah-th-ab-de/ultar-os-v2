//! The search history (`search.history`): the last searches and opened results, kept local
//! to the person in front, bounded and clearable. With the setting off nothing is stored and
//! what was kept is deleted. The entries live in the state ([`History`]); the file beside the
//! shell (`<state>/<person>/history.json`) is read at start and written when they change. The
//! overview search offers matching entries as a group; choosing one runs it again.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ultar_i18n::arabic;

/// Most entries kept (the newest first; older ones fall out).
pub const MAX_ENTRIES: usize = 20;
/// Longest remembered text, in characters.
pub const MAX_TEXT: usize = 200;

/// One thing remembered: a typed search, an opened app, or an opened Settings panel.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub enum Record {
    /// A search the person submitted.
    Search(String),
    /// An app the person opened from the search (its id).
    App(String),
    /// A Settings panel the person jumped to.
    Setting(String),
}

impl Record {
    /// The text matched against a query (and shown for a search).
    pub fn text(&self) -> &str {
        match self {
            Record::Search(t) | Record::App(t) | Record::Setting(t) => t,
        }
    }
}

/// The remembered entries, newest first. Whether they are kept and shown is the setting
/// `search.history` (`Sources::history`); this is only the list.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    /// The entries, at most [`MAX_ENTRIES`].
    pub entries: Vec<Record>,
}

impl History {
    /// Writes one entry down (the newest first; a repeat moves to the front, the oldest
    /// beyond the limit falls out). Over-long text is cut. The caller gates on the setting.
    pub fn record(&mut self, mut entry: Record) {
        let text: String = entry.text().chars().take(MAX_TEXT).collect();
        if text.trim().is_empty() {
            return;
        }
        entry = match entry {
            Record::Search(_) => Record::Search(text),
            Record::App(_) => Record::App(text),
            Record::Setting(_) => Record::Setting(text),
        };
        self.entries.retain(|e| e != &entry);
        self.entries.insert(0, entry);
        self.entries.truncate(MAX_ENTRIES);
    }

    /// Forgets everything (the setting switched off, or a clear).
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// The entries whose text matches `query` (Arabic without vowel marks and letter
    /// variants when `variants`), newest first. The caller bounds how many it shows.
    pub fn matching(&self, query: &str, variants: bool) -> impl Iterator<Item = &Record> {
        let query = query.trim().to_lowercase();
        self.entries.iter().filter(move |e| {
            if query.is_empty() {
                return false;
            }
            let text = e.text();
            (variants && arabic::search_matches(text, &query))
                || text.to_lowercase().contains(&query)
        })
    }
}

/// A person's folder name: lowercase letters, digits, `-` and `_`, never empty, never a
/// path (the same rule the volume service uses). Anything else keeps no file.
fn plain(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 32
        && !user.starts_with('-')
        && user
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Where the person's history is kept (`None` when there is no folder or the name is unsafe).
fn file(dir: Option<&Path>, user: &str) -> Option<PathBuf> {
    let dir = dir.filter(|_| plain(user))?;
    Some(dir.join(user).join("history.json"))
}

/// Reads the person's kept entries (an unreadable or damaged file leaves nothing). The
/// setting decides afterwards whether they are shown; this only restores the list.
pub fn load(dir: Option<&Path>, user: &str) -> History {
    let Some(file) = file(dir, user) else {
        return History::default();
    };
    let entries = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Record>>(&t).ok())
        .unwrap_or_default();
    let mut h = History { entries };
    h.entries.truncate(MAX_ENTRIES);
    h
}

/// Writes the entries down (atomically: a part file, then renamed). `false` when it could
/// not be written; nothing is kept without a folder.
pub fn save(history: &History, dir: Option<&Path>, user: &str) -> bool {
    let Some(file) = file(dir, user) else {
        return false;
    };
    let Some(parent) = file.parent() else {
        return false;
    };
    if std::fs::create_dir_all(parent).is_err() {
        return false;
    }
    let part = file.with_extension("part");
    let text = serde_json::to_string(&history.entries).unwrap_or_default();
    std::fs::write(&part, text).is_ok() && std::fs::rename(&part, &file).is_ok()
}

/// Deletes the person's kept history (the setting switched off). A missing file is fine.
pub fn delete(dir: Option<&Path>, user: &str) {
    if let Some(file) = file(dir, user) {
        let _ = std::fs::remove_file(file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("desk-history-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    #[test]
    fn entries_are_bounded_newest_first_and_deduplicated() {
        let mut h = History::default();
        for i in 0..30 {
            h.record(Record::Search(format!("q{i}")));
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert_eq!(h.entries[0], Record::Search("q29".into()));
        // A repeat moves to the front, not a second copy.
        h.record(Record::Search("q25".into()));
        assert_eq!(h.entries[0], Record::Search("q25".into()));
        assert_eq!(
            h.entries
                .iter()
                .filter(|e| **e == Record::Search("q25".into()))
                .count(),
            1
        );
        // Blank and over-long text.
        h.record(Record::Search("   ".into()));
        assert_eq!(h.entries[0], Record::Search("q25".into()));
        h.record(Record::Search("x".repeat(400)));
        assert_eq!(h.entries[0].text().chars().count(), MAX_TEXT);
        h.clear();
        assert!(h.entries.is_empty());
    }

    #[test]
    fn matching_finds_queries_apps_and_panels_in_both_languages() {
        let mut h = History::default();
        h.record(Record::Search("نجمة".into()));
        h.record(Record::App("org.ultar.files".into()));
        h.record(Record::Setting("display".into()));
        assert!(h
            .matching("نجمة", false)
            .any(|e| e == &Record::Search("نجمة".into())));
        assert!(h
            .matching("files", false)
            .any(|e| e == &Record::App("org.ultar.files".into())));
        assert!(h
            .matching("display", false)
            .any(|e| e == &Record::Setting("display".into())));
        // Arabic variants ignore vowel marks.
        assert!(h
            .matching("نَجْمَة", true)
            .any(|e| e == &Record::Search("نجمة".into())));
        assert!(!h
            .matching("نَجْمَة", false)
            .any(|e| e == &Record::Search("نجمة".into())));
        assert_eq!(h.matching("", false).count(), 0);
        assert_eq!(h.matching("nothing", false).count(), 0);
    }

    #[test]
    fn the_file_survives_a_restart_and_off_deletes_it() {
        let d = dir("store");
        let mut h = History::default();
        h.record(Record::Search("hello".into()));
        h.record(Record::App("org.ultar.files".into()));
        assert!(save(&h, Some(&d), "amal"));
        // It survives a restart.
        let again = load(Some(&d), "amal");
        assert_eq!(again.entries, h.entries);
        // Another person keeps their own.
        assert!(load(Some(&d), "badr").entries.is_empty());
        // Off deletes it.
        delete(Some(&d), "amal");
        assert!(load(Some(&d), "amal").entries.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn unsafe_names_and_missing_folders_keep_nothing() {
        let d = dir("unsafe");
        let mut h = History::default();
        h.record(Record::Search("x".into()));
        for bad in ["../x", "/etc", "Amal", "", "-x", "a/b"] {
            assert!(!save(&h, Some(&d), bad), "{bad}");
            assert!(load(Some(&d), bad).entries.is_empty(), "{bad}");
        }
        // No folder: nothing is kept, nothing fails.
        assert!(!save(&h, None, "amal"));
        assert!(load(None, "amal").entries.is_empty());
        delete(None, "amal");
        // A damaged file leaves nothing.
        std::fs::create_dir_all(d.join("amal")).unwrap();
        std::fs::write(d.join("amal/history.json"), "not json").unwrap();
        assert!(load(Some(&d), "amal").entries.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
