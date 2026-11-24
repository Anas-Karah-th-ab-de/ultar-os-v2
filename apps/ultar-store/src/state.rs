//! The Store's state (APP-RULES §2 `state.rs`): pure data, serialisable, no I/O.
//!
//! One `Store` holds everything the views show; [`crate::update`] changes it.

use crate::catalog::Catalog;
use crate::error::StoreError;
use std::collections::BTreeMap;

/// The areas of the Store (DESKTOP-SPEC §7.3, nav list and tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Area {
    /// Recommendations and components (the board "Store").
    Discover,
    /// Apps by category.
    Categories,
    /// Search results for the current query.
    Search,
    /// The app page (`store.open`).
    AppPage,
    /// Installed apps.
    Installed,
    /// Updates: system and apps.
    Updates,
    /// Install a package file from a stick or folder.
    InstallFile,
    /// Drives and folders with a signed catalogue.
    OfflineSources,
    /// Account (optional).
    Account,
}

/// The lifecycle state of one operation (install / update / remove).
#[derive(Debug, Clone, PartialEq)]
pub enum OpState {
    /// Running; `None` = no percentage known yet.
    Running(Option<f32>),
    /// Finished successfully.
    Done,
    /// Failed.
    Failed,
}

/// What is currently in flight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    /// Installing.
    Install,
    /// Updating.
    Update,
    /// Removing (kept data or not is remembered by the caller).
    Remove,
}

impl OpKind {
    /// The catalog key naming the kind (progress text and cancel dialog).
    pub fn key(self) -> &'static str {
        match self {
            Self::Install => "store.app.install",
            Self::Update => "store.app.update",
            Self::Remove => "store.app.remove",
        }
    }
}

/// One in-flight operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Operation {
    /// What kind.
    pub kind: OpKind,
    /// Per-app state.
    pub states: BTreeMap<String, OpState>,
    /// When the store ran out of space: the app that needs it (drives
    /// `store.free.*`).
    pub no_space_for: Option<String>,
}

impl Operation {
    /// A new operation for one app.
    pub fn started(kind: OpKind, app: &str) -> Self {
        let mut states = BTreeMap::new();
        states.insert(app.to_string(), OpState::Running(None));
        Self {
            kind,
            states,
            no_space_for: None,
        }
    }

    /// The state of one app in this operation.
    pub fn state(&self, app: &str) -> Option<&OpState> {
        self.states.get(app)
    }
}

/// Where the catalogue and the device are right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Surface {
    /// Loading the catalogue.
    Loading,
    /// Catalogue ready.
    Ready,
    /// Offline: only installed apps and offline sources are shown.
    Offline,
    /// Error with its reason (message key).
    Error {
        /// The `store.error.*` catalog key explaining the cause.
        key: String,
    },
}

impl Surface {
    /// Whether a catalogue is usable in this surface.
    pub fn has_catalog(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// An installed app as the launcher reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledApp {
    /// Package id.
    pub id: String,
    /// Installed version.
    pub version: String,
    /// Installed size in bytes, when known.
    pub size: Option<u64>,
    /// Last start in ms, when known.
    pub last_used_ms: Option<u64>,
}

/// One selectable entry of the "what can be freed" list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FreeEntry {
    /// Stable id (`trash`, `temp`, `old-updates`).
    pub id: String,
    /// Bytes it would free, when known (the board shows `—` for unknown).
    pub size: Option<u64>,
    /// Whether it is selected (default: on, like the board).
    pub selected: bool,
}

/// The "not enough space" data of the surface on the board.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FreeSpace {
    /// The selectable items (drives `store.free.item` / `store.free.go`).
    pub entries: Vec<FreeEntry>,
    /// Bytes required by the blocked install (None = `—`).
    pub required: Option<u64>,
    /// Bytes free (None = `—`).
    pub available: Option<u64>,
}

impl FreeSpace {
    /// Sum of the selected entries (the board recomputes it per click).
    pub fn selected_total(&self) -> u64 {
        self.entries
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.size.unwrap_or_default())
            .sum()
    }
}

/// The catalogue and where it came from.
#[derive(Debug, Clone, Default)]
pub struct CatalogState {
    /// The data (`None` until the first successful load).
    pub data: Option<Catalog>,
    /// True when the catalogue signature was verified.
    pub verified: bool,
    /// Directory the catalogue was read from (packages sit next to it).
    pub dir: Option<String>,
    /// Drives and folders found as offline sources.
    pub sources: Vec<crate::source::FoundSource>,
}

/// The state of the Store.
#[derive(Debug, Clone)]
pub struct Store {
    /// The current area.
    pub area: Area,
    /// The app whose app page is open (area == AppPage).
    pub open_app: Option<String>,
    /// The area the app page was opened from (back goes there, CLICKS `store.back`).
    pub back_to: Option<Area>,
    /// The chosen category on the Categories page.
    pub category: Option<String>,
    /// Current search query.
    pub query: String,
    /// Which page state is shown (loading / ready / offline / error).
    pub surface: Surface,
    /// The catalogue.
    pub catalog: CatalogState,
    /// Installed apps from the launcher.
    pub installed: Vec<InstalledApp>,
    /// The in-flight operation (install/update/remove).
    pub operation: Option<Operation>,
    /// The "not enough space" list.
    pub free: FreeSpace,
    /// Apps waiting in the "update all" chain (`store.update.all`).
    pub update_queue: Vec<String>,
    /// The last failure (drives the error surface and `store.log`).
    pub last_error: Option<StoreError>,
    /// Path of the last saved error report (`store.log` answer).
    pub log_saved: Option<String>,
}

impl Store {
    /// A fresh store in the loading state.
    pub fn new() -> Self {
        Self {
            area: Area::Discover,
            open_app: None,
            back_to: None,
            category: None,
            query: String::new(),
            surface: Surface::Loading,
            catalog: CatalogState::default(),
            installed: Vec::new(),
            operation: None,
            free: FreeSpace::default(),
            update_queue: Vec::new(),
            last_error: None,
            log_saved: None,
        }
    }

    /// Is the app with this id installed?
    pub fn is_installed(&self, id: &str) -> bool {
        self.installed.iter().any(|a| a.id == id)
    }

    /// The installed record, if any.
    pub fn installed_app(&self, id: &str) -> Option<&InstalledApp> {
        self.installed.iter().find(|a| a.id == id)
    }

    /// True when the catalogue offers a newer version than the installed one.
    pub fn update_available(&self, id: &str) -> bool {
        match (&self.catalog.data, self.installed_app(id)) {
            (Some(c), Some(inst)) => c.app(id).is_some_and(|a| a.is_update_for(&inst.version)),
            _ => false,
        }
    }

    /// The catalogue entry of an id (accessor that keeps the views concise).
    pub fn entry(&self, id: &str) -> Option<&crate::catalog::CatalogApp> {
        self.catalog.data.as_ref().and_then(|c| c.app(id))
    }

    /// The app list of a whole catalogue (empty when none was loaded).
    pub fn apps(&self) -> &[crate::catalog::CatalogApp] {
        match &self.catalog.data {
            Some(c) => &c.apps,
            None => &[],
        }
    }

    /// The category list in catalogue order.
    pub fn categories(&self) -> Vec<&str> {
        match &self.catalog.data {
            Some(c) => c.categories(),
            None => Vec::new(),
        }
    }

    /// Whether the offline surface should also show a catalogue: it keeps the
    /// last good one but marks it as unverified-run (DESKTOP-SPEC §7.3).
    pub fn has_catalog(&self) -> bool {
        self.surface.has_catalog() && self.catalog.data.is_some()
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{CatalogApp, Loc, Source};

    fn catalog_with(id: &str, version: &str) -> Catalog {
        let loc = |s: &str| Loc {
            ar: s.into(),
            en: s.into(),
            de: None,
        };
        Catalog {
            format_version: crate::catalog::FORMAT_VERSION,
            generated: None,
            apps: vec![CatalogApp {
                id: id.into(),
                name: loc("Media"),
                summary: loc("s"),
                description: loc("d"),
                publisher: "p".into(),
                category: "media".into(),
                icon: "media".into(),
                source: Source::Ultar,
                version: Some(version.into()),
                size_download: None,
                size_installed: None,
                file_types: Vec::new(),
                permissions: Vec::new(),
                requires: Vec::new(),
                package: None,
            }],
        }
    }

    #[test]
    fn update_compare_uses_installed_version() {
        let mut s = Store::new();
        s.catalog.data = Some(catalog_with("media", "2.0.0"));
        assert!(!s.update_available("media"));
        s.installed.push(InstalledApp {
            id: "media".into(),
            version: "1.0.0".into(),
            size: None,
            last_used_ms: None,
        });
        assert!(s.update_available("media"));
        s.installed[0].version = "2.0.0".into();
        assert!(!s.update_available("media"));
        assert!(s.is_installed("media"));
        assert!(!s.is_installed("other"));
    }

    #[test]
    fn free_total_follows_selection() {
        let mut s = Store::new();
        s.free.entries = vec![
            FreeEntry {
                id: "trash".into(),
                size: Some(10),
                selected: true,
            },
            FreeEntry {
                id: "temp".into(),
                size: Some(5),
                selected: false,
            },
            FreeEntry {
                id: "old".into(),
                size: None,
                selected: true,
            },
        ];
        assert_eq!(s.free.selected_total(), 10);
        s.free.entries[1].selected = true;
        assert_eq!(s.free.selected_total(), 15);
    }

    #[test]
    fn surface_catalog_presence() {
        assert!(!Surface::Loading.has_catalog());
        assert!(Surface::Ready.has_catalog());
        assert!(!Surface::Offline.has_catalog());
        assert!(!Surface::Error {
            key: String::new()
        }
        .has_catalog());
    }
}
