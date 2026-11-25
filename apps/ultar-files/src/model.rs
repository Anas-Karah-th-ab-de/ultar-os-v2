//! The state of one Files window: pure data, changed only by [`crate::update`].

use crate::fs::copy::{Cancel, Policy, Progress};
use crate::fs::trash::Trashed;
use crate::fs::undo::UndoStack;
use crate::fs::{AreaId, Entry, FsError, Kind, Loc};
use ultar_ui::widgets::Preview;

/// Most folders kept in each direction of the history.
pub const MAX_HISTORY: usize = 64;

/// A mounted drive (`volumes.state`) whose folder the cell shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Drive {
    /// Id for `volumes.eject`.
    pub id: String,
    /// Label ("USB").
    pub name: String,
    /// Its URI (`file:///…/NAME`).
    pub uri: String,
    /// Its area.
    pub area: AreaId,
}

/// One shown item.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// What the folder listing said.
    pub entry: Entry,
    /// In the trash: where it came from.
    pub trashed: Option<Trashed>,
    /// Its picture, once made.
    pub thumb: Option<Preview>,
}

/// Grid of tiles or a table of rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Tiles (boards).
    #[default]
    Grid,
    /// Rows with name, size, date, kind.
    List,
}

/// What the items are sorted by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    /// Name (collation of the user's language; folders first).
    #[default]
    Name,
    /// Size.
    Size,
    /// Last change.
    Modified,
    /// Kind (by extension).
    Kind,
}

/// Back and forward of this window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct History {
    /// Earlier folders, newest last.
    pub back: Vec<Loc>,
    /// Folders after going back, newest last.
    pub forward: Vec<Loc>,
}

impl History {
    /// Leaving `from` for a new folder: forward is forgotten.
    pub fn visit(&mut self, from: Loc) {
        if self.back.last() != Some(&from) {
            self.back.push(from);
        }
        if self.back.len() > MAX_HISTORY {
            self.back.remove(0);
        }
        self.forward.clear();
    }
}

/// Which operation a job does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    /// Copy into a folder.
    Copy(Vec<Loc>, Loc, Policy),
    /// Move into a folder.
    Move(Vec<Loc>, Loc, Policy),
    /// Move to the trash.
    Trash(Vec<Loc>),
    /// Back from the trash (ids).
    Restore(Vec<String>),
    /// Delete for good from the trash (ids).
    Purge(Vec<String>),
    /// Empty the trash.
    Empty,
    /// Undo the newest step.
    Undo,
}

/// A running job as the window shows it.
#[derive(Clone, Debug)]
pub struct Running {
    /// What it does.
    pub kind: JobKind,
    /// How far it is.
    pub progress: Progress,
    /// Its cancel switch.
    pub cancel: Cancel,
}

/// A question or panel over the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Dialog {
    /// Rename (also right after "new folder"): the item and the text typed.
    Rename(Loc),
    /// Empty the trash?
    ConfirmEmpty,
    /// Delete these trash items for good?
    ConfirmPurge(Vec<String>),
    /// Properties of an item.
    Properties(Loc),
    /// Names taken: the job and the first source/target.
    Conflict(Box<JobKind>, Loc),
    /// The drive is in use (eject refused).
    DriveBusy(String),
    /// Something failed: the message key and how many items.
    Failed(FsError, usize),
}

/// A short message at the bottom (toast), optionally with Undo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// i18n key.
    pub key: &'static str,
    /// Number for the message (items).
    pub count: i64,
    /// Name for the message.
    pub name: String,
    /// Offers Undo.
    pub undo: bool,
}

/// The whole state of the window.
#[derive(Debug)]
pub struct State {
    /// Granted fixed areas (sidebar, in order), without the trash.
    pub places: Vec<AreaId>,
    /// The trash is granted.
    pub has_trash: bool,
    /// Mounted drives.
    pub drives: Vec<Drive>,
    /// The folder shown.
    pub folder: Loc,
    /// Items of the folder (all, unsorted).
    pub items: Vec<Item>,
    /// Names of the selected items.
    pub selected: Vec<String>,
    /// Why the folder cannot be shown.
    pub error: Option<FsError>,
    /// Grid or list, sort key and direction.
    pub look: Look,
    /// The search text.
    pub query: String,
    /// Names that match the search by name or content (`None`: no search).
    pub hits: Option<Vec<String>>,
    /// Back/forward.
    pub history: History,
    /// The dialog shown.
    pub dialog: Option<Dialog>,
    /// The running job.
    pub job: Option<Running>,
    /// The toast.
    pub notice: Option<Notice>,
    /// Clipboard and undo steps.
    pub edit: Edit,
}

/// How the items are shown.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    /// Grid or list.
    pub view: ViewMode,
    /// Sort key and ascending.
    pub sort: (SortKey, bool),
}

/// What the window keeps for editing: the clipboard and the undo steps.
#[derive(Debug, Default)]
pub struct Edit {
    /// Cut or copied items (cut = true).
    pub clipboard: Option<(Vec<Loc>, bool)>,
    /// Undo steps.
    pub undo: UndoStack,
}

impl State {
    /// A window on `folder`.
    pub fn new(places: Vec<AreaId>, has_trash: bool, folder: Loc) -> State {
        State {
            places,
            has_trash,
            drives: Vec::new(),
            folder,
            items: Vec::new(),
            selected: Vec::new(),
            error: None,
            look: Look {
                view: ViewMode::Grid,
                sort: (SortKey::Name, true),
            },
            query: String::new(),
            hits: None,
            history: History::default(),
            dialog: None,
            job: None,
            notice: None,
            edit: Edit::default(),
        }
    }

    /// `true` when the trash is shown.
    pub fn in_trash(&self) -> bool {
        self.folder.area == AreaId::Trash
    }

    /// Locations of the selected items.
    pub fn selection(&self) -> Vec<Loc> {
        self.selected
            .iter()
            .filter_map(|n| self.folder.child(n).ok())
            .collect()
    }

    /// Trash ids of the selected items (in the trash).
    pub fn selected_ids(&self) -> Vec<String> {
        if self.in_trash() {
            self.selected.clone()
        } else {
            Vec::new()
        }
    }

    /// The item called `name`.
    pub fn item(&self, name: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.entry.name == name)
    }
}

/// The icon kind of a file name (folders and links are told by [`Kind`]).
pub fn file_kind(entry: &Entry) -> FileKind {
    if entry.kind == Kind::Folder {
        return FileKind::Folder;
    }
    if entry.kind == Kind::Link {
        return FileKind::Link;
    }
    let lower = entry.name.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    match ext {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "svg" => FileKind::Image,
        "xlsx" | "xls" | "ods" | "csv" => FileKind::Sheet,
        "docx" | "doc" | "odt" | "txt" | "md" | "rtf" | "pdf" => FileKind::Text,
        "mp3" | "ogg" | "flac" | "wav" | "m4a" | "opus" => FileKind::Audio,
        "mp4" | "mkv" | "webm" | "avi" | "mov" => FileKind::Video,
        "zip" | "tar" | "gz" | "xz" | "zst" | "7z" | "uap" | "deb" => FileKind::Archive,
        "html" | "htm" => FileKind::Web,
        _ => FileKind::Other,
    }
}

/// Kinds of items for icons, sorting and the default app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FileKind {
    /// A folder.
    Folder,
    /// A text document.
    Text,
    /// A spreadsheet.
    Sheet,
    /// A picture.
    Image,
    /// Sound.
    Audio,
    /// Film.
    Video,
    /// An archive or package.
    Archive,
    /// A web page.
    Web,
    /// A symbolic link.
    Link,
    /// Anything else.
    Other,
}

impl FileKind {
    /// i18n key of the kind's name (list view, properties).
    pub fn key(self) -> &'static str {
        match self {
            FileKind::Folder => "files.kind.folder",
            FileKind::Text => "files.kind.text",
            FileKind::Sheet => "files.kind.sheet",
            FileKind::Image => "files.kind.image",
            FileKind::Audio => "files.kind.audio",
            FileKind::Video => "files.kind.video",
            FileKind::Archive => "files.kind.archive",
            FileKind::Web => "files.kind.web",
            FileKind::Link => "files.kind.link",
            FileKind::Other => "files.kind.other",
        }
    }
}
