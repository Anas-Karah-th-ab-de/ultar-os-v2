//! The desktop icons (`desk.icon`, boards "Main", "DesktopTop": a column of tiles 24 from
//! the top and the start edge, 8 apart): the home folder, each mounted drive, then the items
//! of the user's desktop folder. They live on the desktop surface of Files.

use super::{item_icon, Built};
use crate::action::{Command, Effect, Target};
use crate::fs::{AreaId, Loc};
use crate::model::{file_kind, Dialog, FileKind, State};
use crate::update;
use ultar_ui::event::ActionKind;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::widgets::{Label, List, ListItem, Menu, MenuItem};

/// App id of Files (folders and drives open in a Files window).
pub const FILES: &str = "org.ultar.files";

/// Builds the desktop: `s.folder` is the desktop area's root.
pub fn build(ui: &mut Ui, s: &State, fs: &crate::fs::Fs) -> Result<Built, UiError> {
    ui.clear();
    let mut b = Built::default();
    let flex = Flex::column()
        .gap(Space::S8)
        .padding(Padding::all(Space::S24))
        .cross(ultar_ui::layout::CrossAlign::Start);
    let mut list = List::new().multi().name(Label::key("files.desk.name"));
    list.flex = flex;
    let list = ui.add(ui.root(), list)?;
    b.items = Some(list);
    if s.places.contains(&AreaId::Home) {
        let id = ui.add(
            list,
            ListItem::new(Label::key("files.place.home"))
                .icon(IconId::Home)
                .tile(),
        )?;
        b.mark(id, Target::Place(0));
        ui.set_context_menu(id, Some(menu(false, false)))?;
    }
    for (i, d) in s.drives.iter().enumerate() {
        let label = Label::key_with("files.place.drive", &[("name", &d.name)]);
        let id = ui.add(list, ListItem::new(label).icon(IconId::Usb).tile())?;
        b.mark(id, Target::Drive(i));
        ui.set_context_menu(id, Some(menu(false, true)))?;
    }
    for &i in &update::visible(s) {
        let item = &s.items[i];
        let mut tile = ListItem::new(Label::data(&item.entry.name))
            .icon(item_icon(item))
            .tile();
        if let Some(t) = &item.thumb {
            tile = tile.thumbnail(t.clone());
        }
        let id = ui.add(list, tile)?;
        b.mark(id, Target::Item(item.entry.name.clone()));
        b.item_nodes.push((id, item.entry.name.clone()));
        ui.set_context_menu(id, Some(menu(true, false)))?;
    }
    super::dialogs::build(ui, &mut b, s, fs)?;
    Ok(b)
}

/// Right click: Open, Rename, Move to trash, Properties; on a drive also Eject.
fn menu(item: bool, drive: bool) -> Menu {
    let entry = |c: Command| MenuItem::new(c.id(), Label::key(c.key()));
    let mut v = vec![entry(Command::Open)];
    let (mut rename, mut trash) = (
        entry(Command::Rename).separated(),
        entry(Command::Trash).danger(),
    );
    if !item {
        rename = rename.disabled();
        trash = trash.disabled();
    }
    v.extend([rename, trash, entry(Command::Properties).separated()]);
    if drive {
        v.push(entry(Command::Eject));
    }
    Menu::new(v)
}

/// What a click on a desktop icon does.
pub fn action(s: &mut State, target: &Target, kind: &ActionKind, key_input: bool) -> Vec<Effect> {
    let open = matches!(kind, ActionKind::DoubleActivated)
        || (key_input && *kind == ActionKind::Activated);
    match (target, kind) {
        (_, ActionKind::MenuChosen(id)) => match Command::from_id(*id) {
            Some(Command::Open) => open_icon(s, target),
            Some(Command::Eject) => match target {
                Target::Drive(i) => s
                    .drives
                    .get(*i)
                    .map(|d| vec![Effect::Eject(d.id.clone())])
                    .unwrap_or_default(),
                _ => Vec::new(),
            },
            Some(Command::Properties) => {
                s.dialog = Some(Dialog::Properties(icon_loc(s, target)));
                Vec::new()
            }
            Some(c @ (Command::Rename | Command::Trash)) => match target {
                Target::Item(name) => {
                    s.selected = vec![name.clone()];
                    update::command(s, c)
                }
                _ => Vec::new(),
            },
            _ => Vec::new(),
        },
        (Target::Item(name), ActionKind::Activated) if !open => {
            s.selected = vec![name.clone()];
            Vec::new()
        }
        // a single click on home or a drive only selects it
        (Target::Place(_) | Target::Drive(_), ActionKind::Activated) if !open => Vec::new(),
        _ if open => open_icon(s, target),
        _ => update::action(s, target, kind, key_input),
    }
}

fn icon_loc(s: &State, target: &Target) -> Loc {
    match target {
        Target::Drive(i) => s
            .drives
            .get(*i)
            .map_or(Loc::root(AreaId::Home), |d| Loc::root(d.area.clone())),
        Target::Item(name) => s.folder.child(name).unwrap_or_else(|_| s.folder.clone()),
        _ => Loc::root(AreaId::Home),
    }
}

fn open_icon(s: &mut State, target: &Target) -> Vec<Effect> {
    let uri = match target {
        Target::Place(_) => crate::uri::of_loc(&Loc::root(AreaId::Home)),
        Target::Drive(i) => match s.drives.get(*i) {
            Some(d) => d.uri.clone(),
            None => return Vec::new(),
        },
        Target::Item(name) => {
            let is_folder = s
                .item(name)
                .is_some_and(|i| file_kind(&i.entry) == FileKind::Folder);
            if !is_folder {
                return update::open_item(s, name);
            }
            match s.folder.child(name) {
                Ok(loc) => crate::uri::of_loc(&loc),
                Err(_) => return Vec::new(),
            }
        }
        _ => return Vec::new(),
    };
    vec![Effect::Launch {
        app: FILES.to_owned(),
        uri,
    }]
}
