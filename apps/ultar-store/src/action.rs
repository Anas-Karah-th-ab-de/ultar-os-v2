//! The actions of the Store (APP-RULES §2 `action.rs`).
//!
//! The names match the `data-act` values of the boards (docs/ui/CLICKS.md);
//! `id()` returns exactly that string, so the click tests can compare them.
//! Actions the boards of other apps use (`win.*`, `mobile.*`) are not the app's
//! own: the window buttons belong to the compositor, the gesture bar to the shell.

use crate::error::StoreError;
use crate::source::LoadedCatalog;
use crate::state::{Area, FreeEntry, InstalledApp, OpKind};
use crate::source::FoundSource;

/// One click of the Store UI, with its board argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreAction {
    /// `store.area` – opens a Store area (nav item / tab / menu entry).
    Area(Area),
    /// `store.search` – the search field changed (live, tolerant).
    Search(String),
    /// `store.search.clear` – empties the search field, shows Discover again.
    SearchClear,
    /// `store.open` – opens the app page (cards never install directly).
    Open(String),
    /// `store.back` – returns from the app page to the list it came from.
    Back,
    /// `store.launch` – starts the installed app.
    Launch(String),
    /// `store.install` – install (or update) the app of the open page.
    Install(String),
    /// `store.update.all` – update every app with a newer catalogue version.
    UpdateAll,
    /// `store.remove` – remove the app of the open page (confirm keeps/deletes data).
    Remove(String),
    /// `store.remove.confirm` – the confirmation of a removal (keep data or delete).
    RemoveConfirmed {
        /// App id.
        app: String,
        /// Keep the user data?
        keep_data: bool,
    },
    /// The dialog cancelled the removal: nothing happens.
    RemoveCancelled,
    /// `store.retry` – tries to load the catalogue again.
    Retry,
    /// `store.log` – saves the error log as a file.
    Log,
    /// `store.offline` – opens the offline sources (drive or folder with a signed catalogue).
    Offline,
    /// `store.more` – compact: the menu of areas that do not fit as tabs.
    More,
    /// `store.free.item` – selects what is freed; the sum changes.
    FreeItem(String),
    /// `store.free.go` – frees the selected space and continues the installation.
    FreeGo,
    /// `store.free.cancel` – cancels the installation; nothing is freed.
    FreeCancel,
    /// A running operation was cancelled by the user.
    Cancel,
    /// The category tile `store.category` (part of the Categories page).
    Category(String),
    /// `store.file.install` – install from a file picked on the InstallFile page.
    InstallFile(String),
    /// Chose an offline source from the list (its catalogue is loaded).
    UseSource(String),
}

impl StoreAction {
    /// The board id of this action (`data-act` in docs/ui/CLICKS.md).
    pub fn id(&self) -> &'static str {
        match self {
            Self::Area(_) => "store.area",
            Self::Search(_) => "store.search",
            Self::SearchClear => "store.search.clear",
            Self::Open(_) => "store.open",
            Self::Back => "store.back",
            Self::Launch(_) => "store.launch",
            Self::Install(_) => "store.install",
            Self::UpdateAll => "store.update.all",
            Self::Remove(_) => "store.remove",
            Self::RemoveConfirmed { .. } => "store.remove.confirm",
            Self::RemoveCancelled => "store.remove.cancel",
            Self::Retry => "store.retry",
            Self::Log => "store.log",
            Self::Offline => "store.offline",
            Self::More => "store.more",
            Self::FreeItem(_) => "store.free.item",
            Self::FreeGo => "store.free.go",
            Self::FreeCancel => "store.free.cancel",
            Self::Cancel => "store.cancel",
            Self::Category(_) => "store.category",
            Self::InstallFile(_) => "store.file.install",
            Self::UseSource(_) => "store.source.use",
        }
    }

    /// Every distinct id, for the click tests (docs/ui/CLICKS.md coverage).
    pub fn all_ids() -> [&'static str; 21] {
        [
            "store.area",
            "store.search",
            "store.search.clear",
            "store.open",
            "store.back",
            "store.launch",
            "store.install",
            "store.remove",
            "store.remove.confirm",
            "store.remove.cancel",
            "store.retry",
            "store.log",
            "store.offline",
            "store.more",
            "store.free.item",
            "store.free.go",
            "store.free.cancel",
            "store.cancel",
            "store.category",
            "store.file.install",
            "store.source.use",
        ]
    }
}

/// A command the model asks the shell to run (I/O happens only in `service.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Load (or reload) the catalogue from the configured source.
    ReloadCatalog,
    /// Ask the launcher for the installed apps.
    ReloadInstalled,
    /// Discover removable media carrying a catalogue.
    DiscoverSources,
    /// Use the catalogue in this directory.
    UseSource(String),
    /// Start an installed app (`store.launch`).
    Launch(String),
    /// Install (or update) this package file through the launcher.
    Install {
        /// App id (or the picked file on the Install-file page).
        app: String,
        /// Absolute path of the `.uap`.
        path: String,
    },
    /// Remove an app, keeping or deleting its data.
    Uninstall {
        /// App id.
        app: String,
        /// Keep the user data?
        keep_data: bool,
    },
    /// Cancel a running operation.
    CancelOp {
        /// Which kind.
        kind: OpKind,
    },
    /// The UI must ask "Remove this app?" with keep/delete data.
    AskRemove(String),
    /// Free the selected items; continue installing `resume`.
    Free {
        /// Selected item ids.
        ids: Vec<String>,
        /// App whose installation continues afterwards.
        resume: Option<String>,
    },
    /// Ask the launcher what can be freed.
    FreeItems {
        /// App the space is needed for.
        for_app: Option<String>,
    },
    /// Save this report as a file (`store.log`).
    SaveLog {
        /// The text to save.
        report: String,
    },
    /// The catalogue lists no package file for this app (offline gap).
    PackageMissing(String),
}

/// An answer from the shell (`service.rs` events).
#[derive(Debug, Clone, PartialEq)]
pub enum StoreEvent {
    /// The catalogue load finished.
    CatalogLoaded(Result<LoadedCatalog, StoreError>),
    /// The installed apps, fresh from the launcher.
    Installed(Vec<InstalledApp>),
    /// Progress of a running operation; `None` percent = unknown.
    Progress {
        /// App id.
        app: String,
        /// 0.0 … 1.0.
        percent: Option<f32>,
    },
    /// An operation finished.
    OperationDone {
        /// App id.
        app: String,
        /// Ok or the reason.
        result: Result<(), StoreError>,
    },
    /// The list of what can be freed (with required/available bytes).
    FreeItems(Vec<FreeEntry>, Option<u64>, Option<u64>),
    /// Freeing finished; `resumed` names the install that continues.
    Freed {
        /// Bytes freed.
        freed: u64,
        /// App whose installation was continued and finished, if any.
        resumed: Option<String>,
    },
    /// Offline sources found on the machine.
    Sources(Vec<FoundSource>),
    /// An app was removed.
    Removed(String),
    /// The error log was saved to this path (`store.log` answer).
    LogSaved {
        /// Where the report file was written.
        path: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_the_board_names() {
        let a = StoreAction::Area(Area::Discover);
        assert_eq!(a.id(), "store.area");
        assert_eq!(StoreAction::SearchClear.id(), "store.search.clear");
        assert_eq!(StoreAction::FreeGo.id(), "store.free.go");
        assert_eq!(
            StoreAction::RemoveConfirmed {
                app: "x".into(),
                keep_data: true
            }
            .id(),
            "store.remove.confirm"
        );
    }

    #[test]
    fn every_id_is_unique() {
        let ids = StoreAction::all_ids();
        for (i, a) in ids.iter().enumerate() {
            for b in ids.iter().skip(i + 1) {
                assert_ne!(a, b, "duplicate id {a}");
            }
        }
    }
}
