//! Widget trees of Files, from `ultar-ui` only (boards "Main", "DesktopTop": the Files window
//! and the desktop icons). Each builder records what its nodes mean ([`Built::targets`]).

pub mod desk;
mod dialogs;
mod items;
mod window;

pub use window::build;

use crate::action::Target;
use crate::fs::AreaId;
use crate::model::{file_kind, FileKind, Item};
use std::collections::HashMap;
use ultar_ui::icon::IconId;
use ultar_ui::tokens::Space;
use ultar_ui::ui::NodeId;

/// Width of the sidebar (boards: 208 = 4 × 48 + 16).
pub const SIDEBAR: f32 = 4.0 * Space::S48.points() + Space::S16.points();
/// Width of the search field (boards: 200 = 4 × 48 + 8).
pub const SEARCH: f32 = 4.0 * Space::S48.points() + Space::S8.points();
/// Window width below which the folder toolbar cannot hold all of its parts at their board sizes
/// beside the sidebar (back, forward, title, count, search field of [`SEARCH`], two buttons): the
/// count goes and the search field takes the room that is left, so the field and the buttons after
/// it stay inside the window (at 1024 × 768 a new window is about 614 wide: the field was cut off
/// by the window's edge and the buttons behind it could not be reached).
pub const TOOLBAR_WHOLE: f32 = 720.0;
/// Height of the folder toolbar (boards: 48).
pub const TOOLBAR: f32 = Space::S48.points();

/// A built tree: what each node means and the nodes the app needs again.
#[derive(Debug, Default)]
pub struct Built {
    /// Meaning of each operable node.
    pub targets: HashMap<NodeId, Target>,
    /// The list (grid) or table of items.
    pub items: Option<NodeId>,
    /// Item nodes with their names (grid), in order.
    pub item_nodes: Vec<(NodeId, String)>,
    /// Names in the order of the table's rows (list view).
    pub rows: Vec<String>,
    /// The scroll container of the items (its offset is kept across rebuilds).
    pub scroll: Option<NodeId>,
    /// The search field.
    pub search: Option<NodeId>,
    /// The rename field of the open dialog.
    pub rename: Option<NodeId>,
    /// The open dialog.
    pub dialog: Option<NodeId>,
    /// The progress bar of the running job.
    pub progress: Option<NodeId>,
}

impl Built {
    pub(crate) fn mark(&mut self, id: NodeId, target: Target) -> NodeId {
        self.targets.insert(id, target);
        id
    }

    /// The node of `target`.
    pub fn node(&self, target: &Target) -> Option<NodeId> {
        self.targets
            .iter()
            .find(|(_, t)| *t == target)
            .map(|(n, _)| *n)
    }
}

/// The sidebar icon of a place.
pub fn place_icon(area: &AreaId) -> IconId {
    match area {
        AreaId::Home => IconId::Home,
        AreaId::Documents => IconId::PlaceDocuments,
        AreaId::Downloads => IconId::PlaceDownloads,
        AreaId::Pictures => IconId::PlacePictures,
        AreaId::Music => IconId::PlaceMusic,
        AreaId::Videos => IconId::Video,
        AreaId::Desktop => IconId::Monitor,
        AreaId::Trash => IconId::Bin,
        AreaId::Drive(_) => IconId::Usb,
    }
}

/// The icon of an item (by kind and name).
pub fn item_icon(item: &Item) -> IconId {
    match file_kind(&item.entry) {
        FileKind::Folder => IconId::KindFolder,
        FileKind::Text | FileKind::Plain | FileKind::Other | FileKind::Link => IconId::KindText,
        FileKind::Sheet => IconId::KindSheet,
        FileKind::Image => IconId::KindImage,
        FileKind::Audio => IconId::Media,
        FileKind::Video => IconId::Video,
        FileKind::Archive => IconId::Package,
        FileKind::Web => IconId::Browser,
    }
}
