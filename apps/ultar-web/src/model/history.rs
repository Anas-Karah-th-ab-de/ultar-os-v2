//! Visited pages for the address field's suggestions. Private tabs never add to it. The list
//! is bounded; the oldest entries go first.

use super::store::{self, StoreError};
use std::path::Path;

/// Most remembered pages.
pub const MAX_ENTRIES: usize = 5000;
/// Most suggestions shown.
pub const MAX_SUGGESTIONS: usize = 6;

/// One visited page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Address.
    pub url: String,
    /// Last known title.
    pub title: String,
    /// Visits.
    pub visits: u32,
}

/// The history, most recent last.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct History {
    entries: Vec<Entry>,
}

impl History {
    /// Records a visit (moves the page to the end).
    pub fn visit(&mut self, url: &str, title: &str) {
        if url.is_empty() || url == "about:blank" {
            return;
        }
        let mut visits = 0;
        if let Some(i) = self.entries.iter().position(|e| e.url == url) {
            visits = self.entries.remove(i).visits;
        }
        if self.entries.len() >= MAX_ENTRIES {
            self.entries.remove(0);
        }
        self.entries.push(Entry {
            url: url.to_string(),
            title: title.to_string(),
            visits: visits.saturating_add(1),
        });
    }

    /// Updates the title of a page already in the history.
    pub fn set_title(&mut self, url: &str, title: &str) {
        if let Some(e) = self.entries.iter_mut().rev().find(|e| e.url == url) {
            e.title = title.to_string();
        }
    }

    /// Pages whose address or title matches the typed text (Arabic-tolerant), most visited
    /// first, at most [`MAX_SUGGESTIONS`].
    pub fn suggest(&self, typed: &str) -> Vec<&Entry> {
        let typed = typed.trim();
        if typed.is_empty() {
            return Vec::new();
        }
        let mut hits: Vec<&Entry> = self
            .entries
            .iter()
            .rev()
            .filter(|e| {
                e.url.contains(typed) || ultar_i18n::arabic::search_matches(&e.title, typed)
            })
            .collect();
        hits.sort_by_key(|e| std::cmp::Reverse(e.visits));
        hits.truncate(MAX_SUGGESTIONS);
        hits
    }

    /// Number of pages.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Forgets everything.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Reads the file (`url  title  visits` lines).
    pub fn load(path: &Path) -> Result<History, StoreError> {
        let mut h = History::default();
        for (n, row) in store::read(path)?.into_iter().enumerate() {
            let [url, title, visits] = row.as_slice() else {
                return Err(StoreError::Damaged(n + 1));
            };
            let visits = visits
                .parse::<u32>()
                .map_err(|_| StoreError::Damaged(n + 1))?;
            if crate::url::host_of(url).is_none() {
                return Err(StoreError::Damaged(n + 1));
            }
            h.visit(url, title);
            if let Some(last) = h.entries.last_mut() {
                last.visits = visits;
            }
        }
        Ok(h)
    }

    /// Writes the file atomically.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        let rows: Vec<Vec<String>> = self
            .entries
            .iter()
            .map(|e| vec![e.url.clone(), e.title.clone(), e.visits.to_string()])
            .collect();
        store::write(path, &rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visits_suggestions_limits_and_file() {
        let mut h = History::default();
        h.visit("https://a.example/", "الصفحة الأولى");
        h.visit("https://b.example/", "Second");
        h.visit("https://a.example/", "الصفحة الأولى");
        h.visit("about:blank", "");
        assert_eq!(h.len(), 2);
        assert_eq!(h.suggest("a.example")[0].visits, 2);
        assert_eq!(h.suggest("الاولى").len(), 1);
        assert!(h.suggest(" ").is_empty());
        h.set_title("https://b.example/", "New");
        assert_eq!(h.suggest("New").len(), 1);
        let dir = std::env::temp_dir().join(format!("ultar-web-hist-{}", std::process::id()));
        let path = dir.join("h.tsv");
        h.save(&path).unwrap();
        assert_eq!(History::load(&path).unwrap(), h);
        std::fs::write(&path, "javascript:x\tt\t1\n").unwrap();
        assert!(History::load(&path).is_err());
        std::fs::write(&path, "https://a/\tt\tx\n").unwrap();
        assert!(History::load(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        for i in 0..MAX_ENTRIES + 3 {
            h.visit(&format!("https://h{i}.example/"), "");
        }
        assert_eq!(h.len(), MAX_ENTRIES);
        for i in 0..10 {
            h.visit(&format!("https://x.example/{i}"), "x");
        }
        assert_eq!(h.suggest("x.example").len(), MAX_SUGGESTIONS);
        h.clear();
        assert!(h.is_empty());
    }
}
