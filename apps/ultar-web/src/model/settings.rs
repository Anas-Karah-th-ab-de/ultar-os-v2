//! Browser settings: search engine, tracking protection and the sites it is off for.

use super::store::{self, StoreError};
use crate::url::SearchEngine;
use std::collections::BTreeSet;
use std::path::Path;

/// Most sites with tracking protection switched off.
pub const MAX_EXCEPTIONS: usize = 1000;

/// The settings of the browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Engine for searches from the address field.
    pub search: SearchEngine,
    /// Block ads and trackers (default on).
    pub protection: bool,
    /// Hosts where protection is off ("Protection for this site").
    pub exceptions: BTreeSet<String>,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            search: SearchEngine::default(),
            protection: true,
            exceptions: BTreeSet::new(),
        }
    }
}

impl Settings {
    /// `true` when protection applies to `host`.
    pub fn protects(&self, host: &str) -> bool {
        self.protection && !self.exceptions.contains(host)
    }

    /// Switches protection for one site; returns `false` when the exception list is full.
    pub fn set_site_protection(&mut self, host: &str, on: bool) -> bool {
        if on {
            self.exceptions.remove(host);
            return true;
        }
        if self.exceptions.len() >= MAX_EXCEPTIONS && !self.exceptions.contains(host) {
            return false;
        }
        self.exceptions.insert(host.to_string());
        true
    }

    /// Reads the settings file; missing → defaults; unknown keys are ignored.
    pub fn load(path: &Path) -> Result<Settings, StoreError> {
        let mut s = Settings::default();
        for (n, row) in store::read(path)?.into_iter().enumerate() {
            match row.as_slice() {
                [k, v] if k == "search" => {
                    s.search = SearchEngine::from_id(v).ok_or(StoreError::Damaged(n + 1))?
                }
                [k, v] if k == "protection" => s.protection = v != "off",
                [k, v] if k == "exception" && s.exceptions.len() < MAX_EXCEPTIONS => {
                    s.exceptions.insert(v.clone());
                }
                _ => {}
            }
        }
        Ok(s)
    }

    /// Writes the settings file atomically.
    pub fn save(&self, path: &Path) -> Result<(), StoreError> {
        let mut rows = vec![
            vec!["search".to_string(), self.search.id().to_string()],
            vec![
                "protection".to_string(),
                if self.protection { "on" } else { "off" }.to_string(),
            ],
        ];
        rows.extend(
            self.exceptions
                .iter()
                .map(|h| vec!["exception".to_string(), h.clone()]),
        );
        store::write(path, &rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exceptions_and_round_trip() {
        let mut s = Settings::default();
        assert!(s.protects("a.example"));
        assert!(s.set_site_protection("a.example", false));
        assert!(!s.protects("a.example"));
        s.search = SearchEngine::Brave;
        let dir = std::env::temp_dir().join(format!("ultar-web-settings-{}", std::process::id()));
        let path = dir.join("settings.tsv");
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
        std::fs::write(&path, "search\tnope\n").unwrap();
        assert!(Settings::load(&path).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
        for i in 0..MAX_EXCEPTIONS {
            s.set_site_protection(&format!("h{i}"), false);
        }
        assert!(!s.set_site_protection("one-more", false));
        assert!(s.set_site_protection("one-more", true));
        s.protection = false;
        assert!(!s.protects("b.example"));
    }
}
