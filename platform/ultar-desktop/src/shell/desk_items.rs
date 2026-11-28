//! The icons of the desktop besides the app shortcuts (CLICKS `desk.icon`): the home folder,
//! the mounted drives and the desktop folder's items. A double click opens them (a folder or
//! drive in Files, a file with its default app); their right-click menus open, show in Files,
//! rename, move to the trash, show the properties (Files does the changes:
//! `files://desktop/<name>?action=…`) and eject a drive.

use super::bar::{launch, FILES};
use super::{Effect, Opening, Outcome, Shell};
use crate::model::desk_files::{self, AREA};
use crate::model::places::DeskItem;
use crate::model::services::Call;
use ultar_ui::widgets::{Label, Menu, MenuItem};

/// The home folder in Files.
pub const HOME_URI: &str = "files://home/";

/// Items of the menus of the home folder, a drive and a desktop folder item.
pub mod item {
    /// Open.
    pub const OPEN: u32 = 1;
    /// Show the desktop folder in Files.
    pub const SHOW: u32 = 2;
    /// Rename (in Files).
    pub const RENAME: u32 = 3;
    /// Move to the trash (in Files, with undo).
    pub const TRASH: u32 = 4;
    /// Properties (in Files).
    pub const PROPERTIES: u32 = 5;
    /// Eject the drive.
    pub const EJECT: u32 = 6;
}

/// The menu of the home folder, a drive or a desktop folder item.
pub(super) fn menu(s: &Shell, it: DeskItem) -> Menu {
    let gate = |i: MenuItem| {
        if s.d.presence.launcher {
            i
        } else {
            i.disabled()
        }
    };
    let entry = |id: u32, key: &str| gate(MenuItem::new(id, Label::key(key.to_owned())));
    match it {
        DeskItem::Home => Menu::new(vec![
            entry(item::OPEN, "desk.menu.open"),
            entry(item::PROPERTIES, "desk.desk.properties").separated(),
        ]),
        DeskItem::Drive(_) => Menu::new(vec![
            entry(item::OPEN, "desk.menu.open"),
            MenuItem::new(item::EJECT, Label::key("desk.menu.eject")).separated(),
        ]),
        _ => Menu::new(vec![
            entry(item::OPEN, "desk.menu.open"),
            entry(item::SHOW, "desk.desk.show_in_files"),
            entry(item::RENAME, "desk.desk.rename").separated(),
            entry(item::TRASH, "desk.desk.trash").danger(),
            entry(item::PROPERTIES, "desk.desk.properties").separated(),
        ]),
    }
}

/// Opens the home folder, a drive or a desktop folder item.
pub(super) fn open(s: &mut Shell, it: DeskItem) -> Outcome {
    match it {
        DeskItem::Home => launch(s, FILES, Opening::uri(HOME_URI)),
        DeskItem::Drive(i) => match s.d.svc.drives.as_ref().and_then(|d| d.get(i)) {
            Some(drive) => {
                let uri = drive.uri.clone();
                launch(s, FILES, Opening::uri(uri))
            }
            None => Outcome::none(),
        },
        DeskItem::File(i) => match s.d.places.files.get(i).cloned() {
            Some(f) if f.folder => launch(s, FILES, Opening::uri(desk_files::uri(&f.name))),
            // A file opens with its default app (the launcher resolves its URI).
            Some(f) => Outcome::effect(Effect::OpenFile {
                uri: desk_files::uri(&f.name),
                name: f.name,
            }),
            None => Outcome::none(),
        },
        DeskItem::Shortcut(_) => Outcome::none(),
    }
}

/// A chosen item of the menu of the home folder, a drive or a desktop folder item.
pub(super) fn chosen(s: &mut Shell, it: DeskItem, id: u32) -> Outcome {
    match (it, id) {
        (_, item::OPEN) => open(s, it),
        (DeskItem::Home, item::PROPERTIES) => launch(
            s,
            FILES,
            Opening::uri(format!("{HOME_URI}?action=properties")),
        ),
        (DeskItem::Drive(i), item::EJECT) => match s.d.svc.drives.as_ref().and_then(|d| d.get(i)) {
            Some(drive) => Outcome::effect(Effect::Service(Call::Eject(drive.id.clone()))),
            None => Outcome::none(),
        },
        (DeskItem::File(_), item::SHOW) => {
            launch(s, FILES, Opening::uri(format!("files://{AREA}/")))
        }
        (DeskItem::File(i), item::RENAME | item::TRASH | item::PROPERTIES) => {
            let Some(name) = s.d.places.files.get(i).map(|f| f.name.clone()) else {
                return Outcome::none();
            };
            let action = match id {
                item::RENAME => "rename",
                item::TRASH => "trash",
                _ => "properties",
            };
            launch(
                s,
                FILES,
                Opening::uri(desk_files::action_uri(&name, action)),
            )
        }
        _ => Outcome::none(),
    }
}

/// A new folder in the desktop folder: Files creates it with its name ready to type.
pub(super) fn new_folder(s: &mut Shell) -> Outcome {
    launch(
        s,
        FILES,
        Opening::uri(desk_files::action_uri("", "new_folder")),
    )
}
