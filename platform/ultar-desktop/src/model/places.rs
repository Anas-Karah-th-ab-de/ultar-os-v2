//! Where the person put apps (DESKTOP-SPEC §5.1): pinned on the bar, and as shortcuts on the
//! desktop background (beside the home folder, the drives and the desktop folder's items) (`desk.shortcut`: added from the background's menu or an app's menu in
//! the app grid, opened with a double click, renamed, removed). The shortcuts are kept per
//! person beside the shell (`<state>/<person>/desktop.json`): read at start, written when
//! they change. Removing a shortcut never touches the app.

use serde::{Deserialize, Serialize};
use std::path::Path;
use ultar_i18n::{collation, Translator};

/// Most shortcuts on the desktop.
pub const MAX_SHORTCUTS: usize = 64;
/// Longest own name of a shortcut, in characters.
pub const MAX_NAME: usize = 64;
/// The person's file beside the shell.
const FILE: &str = "desktop.json";

/// One shortcut: the app it starts and, when renamed, its own name.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct Shortcut {
    /// Application id (`org.ultar.files`).
    pub app: String,
    /// The name the person gave it (`None`: the app's name).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// One icon on the desktop background (CLICKS `desk.icon`, `desk.shortcut`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeskItem {
    /// The person's home folder (opens in Files).
    Home,
    /// A mounted drive (index into `Services::drives`).
    Drive(usize),
    /// An app shortcut (index into [`Places::shortcuts`]).
    Shortcut(usize),
    /// An item of the desktop folder (index into [`Places::files`]).
    File(usize),
}

/// The pinned apps of the bar and what the desktop shows.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Places {
    /// Pinned app ids (the bar's order).
    pub pinned: Vec<String>,
    /// The desktop's shortcuts in their order (from the start corner).
    pub shortcuts: Vec<Shortcut>,
    /// The items of the desktop folder as last read.
    pub files: Vec<super::desk_files::DeskFile>,
    /// The selected icon (a single click).
    pub selected: Option<DeskItem>,
    /// The app grid was opened to choose the app of a new shortcut ("New shortcut").
    pub picking: bool,
    /// An app dragged out of the app grid (the background shows where it would go).
    pub dragging: Option<String>,
    /// The text of the rename dialog while it is typed.
    pub draft: String,
}

impl Places {
    /// The bar's standard pins and no shortcuts.
    pub fn new(pinned: Vec<String>) -> Places {
        Places {
            pinned,
            ..Places::default()
        }
    }

    /// The app has a shortcut on the desktop.
    pub fn on_desktop(&self, app: &str) -> bool {
        self.shortcuts.iter().any(|s| s.app == app)
    }

    /// Puts a shortcut of `app` on the desktop (after the others) and selects it; `false`
    /// when it is there already (then it is selected) or the desktop is full.
    pub fn add(&mut self, app: &str) -> bool {
        if let Some(i) = self.shortcuts.iter().position(|s| s.app == app) {
            self.selected = Some(DeskItem::Shortcut(i));
            return false;
        }
        if self.shortcuts.len() >= MAX_SHORTCUTS || app.is_empty() {
            return false;
        }
        self.shortcuts.push(Shortcut {
            app: app.to_owned(),
            name: None,
        });
        self.selected = Some(DeskItem::Shortcut(self.shortcuts.len() - 1));
        true
    }

    /// Takes shortcut `index` off the desktop (the app stays installed).
    pub fn remove(&mut self, index: usize) -> bool {
        if index >= self.shortcuts.len() {
            return false;
        }
        self.shortcuts.remove(index);
        self.selected = None;
        true
    }

    /// Gives shortcut `index` its own name (trimmed, cut to [`MAX_NAME`]); an empty name or
    /// the app's own name gives it back the app's name.
    pub fn rename(&mut self, index: usize, text: &str, app_name: &str) -> bool {
        let Some(shortcut) = self.shortcuts.get_mut(index) else {
            return false;
        };
        let text: String = text.trim().chars().take(MAX_NAME).collect();
        shortcut.name = (!text.is_empty() && text != app_name).then_some(text);
        true
    }

    /// Orders the shortcuts by their shown names (the language's alphabetical order).
    pub fn sort(&mut self, name: impl Fn(&Shortcut) -> String) {
        self.shortcuts.sort_by_cached_key(|s| {
            collation::collation_key(super::apps::without_article(&name(s)))
        });
        self.selected = None;
    }
}

/// The name a shortcut shows: its own, or its app's in the current language (the app id
/// when the app is not installed).
pub fn shown_name(s: &Shortcut, catalog: &[super::apps::AppInfo], tr: &Translator) -> String {
    if let Some(name) = &s.name {
        return name.clone();
    }
    catalog
        .iter()
        .find(|a| a.id == s.app)
        .map_or_else(|| s.app.clone(), |a| a.display_name(tr))
}

/// Reads the person's shortcuts (an unreadable or damaged file leaves none; unknown or
/// repeated apps and over-long names are dropped).
pub fn load(dir: Option<&Path>, user: &str) -> Vec<Shortcut> {
    let Some(file) = super::history::person_file(dir, user, FILE) else {
        return Vec::new();
    };
    let read = std::fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str::<Vec<Shortcut>>(&t).ok())
        .unwrap_or_default();
    let mut places = Places::default();
    for s in read {
        if places.add(&s.app) {
            let last = places.shortcuts.len() - 1;
            places.shortcuts[last].name = s
                .name
                .filter(|n| !n.trim().is_empty() && n.chars().count() <= MAX_NAME);
        }
    }
    places.shortcuts
}

/// Writes the person's shortcuts down; `false` when they could not be written (nothing is
/// kept without a folder).
pub fn save(shortcuts: &[Shortcut], dir: Option<&Path>, user: &str) -> bool {
    let Some(file) = super::history::person_file(dir, user, FILE) else {
        return false;
    };
    let text = serde_json::to_string(shortcuts).unwrap_or_default();
    super::history::write_atomic(&file, &text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_are_added_once_renamed_sorted_and_removed() {
        let mut p = Places::new(Vec::new());
        assert!(p.add("org.ultar.terminal") && p.add("org.ultar.files"));
        assert!(!p.add("org.ultar.terminal"), "a second shortcut of an app");
        assert_eq!(
            p.selected,
            Some(DeskItem::Shortcut(0)),
            "the existing one is selected"
        );
        assert!(p.on_desktop("org.ultar.files") && !p.on_desktop("org.ultar.editor"));
        assert!(p.rename(1, "  Meine Dateien  ", "Files"));
        assert_eq!(p.shortcuts[1].name.as_deref(), Some("Meine Dateien"));
        assert!(p.rename(1, "Files", "Files"));
        assert_eq!(
            p.shortcuts[1].name, None,
            "the app's own name is no own name"
        );
        assert!(p.rename(1, &"x".repeat(100), "Files"));
        assert_eq!(
            p.shortcuts[1].name.as_ref().map(|n| n.chars().count()),
            Some(MAX_NAME)
        );
        assert!(!p.rename(9, "x", "x"));
        p.shortcuts[1].name = None;
        p.sort(|s| s.app.clone());
        assert_eq!(p.shortcuts[0].app, "org.ultar.files");
        assert!(p.remove(0) && !p.remove(5));
        assert_eq!(p.shortcuts.len(), 1);
        assert_eq!(p.selected, None);
        for i in 0..MAX_SHORTCUTS + 3 {
            p.add(&format!("app.{i}"));
        }
        assert_eq!(p.shortcuts.len(), MAX_SHORTCUTS);
    }

    #[test]
    fn the_file_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("desk-places-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = Places::new(Vec::new());
        p.add("org.ultar.files");
        p.add("org.ultar.terminal");
        p.rename(1, "Shell", "Terminal");
        assert!(save(&p.shortcuts, Some(&dir), "amal"));
        assert_eq!(load(Some(&dir), "amal"), p.shortcuts);
        assert!(
            load(Some(&dir), "badr").is_empty(),
            "another person keeps their own"
        );
        assert!(!save(&p.shortcuts, Some(&dir), "../x"));
        assert!(load(None, "amal").is_empty());
        std::fs::write(
            dir.join("amal").join(FILE),
            "[{\"app\":\"a\"},{\"app\":\"a\"}]",
        )
        .unwrap();
        assert_eq!(
            load(Some(&dir), "amal").len(),
            1,
            "a repeated app is dropped"
        );
        std::fs::write(dir.join("amal").join(FILE), "not json").unwrap();
        assert!(load(Some(&dir), "amal").is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
