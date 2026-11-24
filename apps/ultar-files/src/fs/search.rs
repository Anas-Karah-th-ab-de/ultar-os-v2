//! Search in the current folder by name and by content at once (CLICKS `files.search`):
//! Arabic without regard to vowel marks and letter variants (`arabic::search_matches`).

use super::{Entry, Fs, Kind, Loc};
use std::io::Read;
use ultar_i18n::arabic;

/// Largest file whose content is searched.
pub const MAX_CONTENT: u64 = 1024 * 1024;
/// Longest query that is used.
pub const MAX_QUERY: usize = 256;

/// `true` when `entry` matches `query` by name, or (files up to [`MAX_CONTENT`] that are
/// UTF-8 text) by content. An empty query matches everything.
pub fn matches(fs: &Fs, folder: &Loc, entry: &Entry, query: &str) -> bool {
    let query = query.trim();
    if query.is_empty() {
        return true;
    }
    let query: String = query.chars().take(MAX_QUERY).collect();
    if arabic::search_matches(&entry.name, &query) {
        return true;
    }
    if entry.kind != Kind::File || entry.size > MAX_CONTENT {
        return false;
    }
    let Ok(loc) = folder.child(&entry.name) else {
        return false;
    };
    let Ok(file) = fs.open_read(&loc) else {
        return false;
    };
    let mut bytes = Vec::new();
    if file.take(MAX_CONTENT).read_to_end(&mut bytes).is_err() || bytes.contains(&0) {
        return false;
    }
    std::str::from_utf8(&bytes).is_ok_and(|text| arabic::search_matches(text, &query))
}
