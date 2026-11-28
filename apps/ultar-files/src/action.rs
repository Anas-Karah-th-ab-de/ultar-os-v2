//! What a node of the window means (CLICKS.md ids in the comments) and what the app must
//! do outside its state (effects).

use crate::fs::Loc;
use crate::model::JobKind;

/// The meaning of a node, given when the view is built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// `files.back`.
    Back,
    /// `files.forward`.
    Forward,
    /// `files.place`: index into `State::places`.
    Place(usize),
    /// `files.drive`: index into `State::drives`.
    Drive(usize),
    /// `files.eject`: index into `State::drives`.
    Eject(usize),
    /// `files.trashplace`.
    TrashPlace,
    /// `files.search`.
    Search,
    /// `files.newfolder`.
    NewFolder,
    /// Grid / list switch.
    ViewMode,
    /// The list of items (grid or table).
    Items,
    /// `files.item`: the item's name.
    Item(String),
    /// Empty the trash (trash toolbar).
    EmptyTrash,
    /// The text field of the rename dialog.
    RenameField,
    /// Confirm the dialog.
    DialogOk,
    /// The second choice of a dialog (keep both, eject anyway …).
    DialogAlt,
    /// Close the dialog.
    DialogCancel,
    /// Cancel the running job.
    JobCancel,
    /// The sort choice of the list view.
    Sort,
    /// Compact layout: the button that opens the places menu.
    PlacesMenu,
    /// Install sheet: the internet switch.
    ForeignInternet,
    /// Install sheet: the local network.
    ForeignLan,
    /// Install sheet: one of the person's folders (contract word).
    ForeignFolder(String),
}

/// Commands of the context menus and shortcuts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Open the selected items.
    Open,
    /// Copy to the clipboard.
    Copy,
    /// Cut to the clipboard.
    Cut,
    /// Paste into the folder.
    Paste,
    /// Rename the selected item.
    Rename,
    /// Move the selected items to the trash.
    Trash,
    /// Show properties.
    Properties,
    /// Restore from the trash.
    Restore,
    /// Delete for good (trash).
    Purge,
    /// Empty the trash.
    EmptyTrash,
    /// New folder.
    NewFolder,
    /// Select all.
    SelectAll,
    /// Undo.
    Undo,
    /// Eject the drive shown.
    Eject,
    /// Install an app from another system (`.deb`, `.exe`, `.msi`, `.apk`).
    Install,
}

impl Command {
    /// Every command in menu id order.
    pub const ALL: [Command; 15] = [
        Command::Open,
        Command::Copy,
        Command::Cut,
        Command::Paste,
        Command::Rename,
        Command::Trash,
        Command::Properties,
        Command::Restore,
        Command::Purge,
        Command::EmptyTrash,
        Command::NewFolder,
        Command::SelectAll,
        Command::Undo,
        Command::Eject,
        Command::Install,
    ];

    /// Menu item id.
    pub fn id(self) -> u32 {
        Command::ALL.iter().position(|c| *c == self).unwrap_or(0) as u32
    }

    /// The command of a menu item id.
    pub fn from_id(id: u32) -> Option<Command> {
        Command::ALL.get(id as usize).copied()
    }

    /// i18n key of its menu label.
    pub fn key(self) -> &'static str {
        match self {
            Command::Open => "files.cmd.open",
            Command::Copy => "files.cmd.copy",
            Command::Cut => "files.cmd.cut",
            Command::Paste => "files.cmd.paste",
            Command::Rename => "files.cmd.rename",
            Command::Trash => "files.cmd.trash",
            Command::Properties => "files.cmd.properties",
            Command::Restore => "files.cmd.restore",
            Command::Purge => "files.cmd.purge",
            Command::EmptyTrash => "files.cmd.empty_trash",
            Command::NewFolder => "files.cmd.new_folder",
            Command::SelectAll => "files.cmd.select_all",
            Command::Undo => "files.cmd.undo",
            Command::Eject => "files.cmd.eject",
            Command::Install => "files.cmd.install",
        }
    }
}

/// What the app does outside its state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// List the shown folder again.
    Reload,
    /// Start a job in the background.
    Job(JobKind),
    /// Open `uri` with `app` (`launcher.start` v2); `fallback` is the Store page to offer
    /// when the app is not installed.
    Launch {
        /// App id.
        app: String,
        /// What to open.
        uri: String,
    },
    /// Open `uri` with its default app: the launcher resolves it through `apps.default.*`
    /// (`launcher.resolve`), then Files starts that app.
    Open {
        /// What to open.
        uri: String,
        /// The item's name (for "no app can open it").
        name: String,
    },
    /// `volumes.eject`.
    Eject(String),
    /// Create the folder and then rename it (new folder).
    NewFolder(Loc),
    /// Rename one item.
    Rename(Loc, String),
    /// Focus the search field / the items after a rebuild.
    FocusItems,
    /// Ask the service what the file at `path` (as the device sees it) is.
    ForeignInspect(String),
    /// Install the file of the open sheet with its choices.
    ForeignInstall,
    /// Start an app (just installed).
    StartApp(String),
}
