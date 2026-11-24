//! The items of the folder: a grid of tiles (boards) or a table, the empty state, the
//! error bar (no permission …) and the context menus.

use super::{item_icon, Built};
use crate::action::{Command, Target};
use crate::model::{file_kind, State, ViewMode};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    EmptyState, InfoBar, Label, List, ListItem, Menu, MenuItem, Scroll, Table, Tone,
};

/// The items part of the window.
pub fn build(ui: &mut Ui, b: &mut Built, main: NodeId, s: &State) -> Result<(), UiError> {
    if let Some(e) = s.error {
        let bar = InfoBar::new(Tone::Danger, Label::key(e.key()))
            .message(Label::key("files.error.folder_hint"));
        let flex = Flex::column().padding(Padding::all(Space::S12));
        let wrap = ui.add(main, flex)?;
        ui.add(wrap, bar)?;
        return Ok(());
    }
    let shown = crate::update::visible(s);
    if shown.is_empty() {
        let (icon, key) = match (s.query.is_empty(), s.in_trash()) {
            (false, _) => (IconId::Search, "files.empty.search"),
            (true, true) => (IconId::Bin, "files.empty.trash"),
            (true, false) => (IconId::KindFolder, "files.empty.folder"),
        };
        ui.add_sized(
            main,
            EmptyState::new(icon, Label::key(key)),
            Sizing::grow(1),
        )?;
        return Ok(());
    }
    match s.look.view {
        ViewMode::Grid => grid(ui, b, main, s, &shown),
        ViewMode::List => table(ui, b, main, s, &shown),
    }
}

fn grid(
    ui: &mut Ui,
    b: &mut Built,
    main: NodeId,
    s: &State,
    shown: &[usize],
) -> Result<(), UiError> {
    let scroll = ui.add_sized(main, Scroll::new(), Sizing::grow(1))?;
    let flex = Flex::row()
        .wrap()
        .gap(Space::S8)
        .padding(Padding::all(Space::S12));
    let mut list = List::new().multi().name(Label::key("files.items"));
    list.flex = flex;
    let list = ui.add(scroll, list)?;
    b.items = Some(b.mark(list, Target::Items));
    ui.set_context_menu(list, Some(folder_menu(s)))?;
    for &i in shown {
        let item = &s.items[i];
        let name = &item.entry.name;
        let mut tile = ListItem::new(Label::data(name))
            .icon(item_icon(item))
            .tile();
        if let Some(t) = &item.thumb {
            tile = tile.thumbnail(t.clone());
        }
        if s.selected.contains(name) {
            tile = tile.selected();
        }
        let id = ui.add(list, tile)?;
        b.mark(id, Target::Item(name.clone()));
        b.item_nodes.push((id, name.clone()));
        ui.set_context_menu(id, Some(item_menu(s)))?;
        ui.set_draggable(id, !s.in_trash())?;
        if file_kind(&item.entry) == crate::model::FileKind::Folder {
            ui.set_drop_target(id, true)?;
        }
    }
    Ok(())
}

fn table(
    ui: &mut Ui,
    b: &mut Built,
    main: NodeId,
    s: &State,
    shown: &[usize],
) -> Result<(), UiError> {
    let columns = [
        "files.col.name",
        "files.col.size",
        "files.col.modified",
        "files.col.kind",
    ];
    let tr = ui.translator();
    let rows: Vec<Vec<Label>> = shown
        .iter()
        .map(|&i| {
            let e = &s.items[i].entry;
            let size = match e.kind {
                crate::fs::Kind::File => ultar_i18n::units::format_size(e.size, tr),
                _ => String::new(),
            };
            vec![
                Label::data(&e.name),
                Label::data(&size),
                Label::data(&date(e.modified, tr)),
                Label::key(file_kind(e).key()),
            ]
        })
        .collect();
    let mut t = Table::new(
        Label::key("files.items"),
        columns.iter().map(|c| Label::key(*c)).collect(),
    )
    .multi()
    .rows(rows);
    let key = [
        crate::model::SortKey::Name,
        crate::model::SortKey::Size,
        crate::model::SortKey::Modified,
        crate::model::SortKey::Kind,
    ];
    t.sort = key
        .iter()
        .position(|k| *k == s.look.sort.0)
        .map(|c| (c, s.look.sort.1));
    let id = ui.add_sized(main, t, Sizing::grow(1))?;
    b.items = Some(b.mark(id, Target::Items));
    b.rows = shown
        .iter()
        .map(|&i| s.items[i].entry.name.clone())
        .collect();
    ui.set_context_menu(id, Some(item_menu(s)))?;
    Ok(())
}

/// A date as the user's locale writes it.
pub fn date(seconds: i64, tr: &ultar_i18n::Translator) -> String {
    ultar_i18n::calendar::Date::from_unix_seconds(seconds, 0)
        .map(|d| ultar_i18n::datefmt::format_date_numeric(d, tr))
        .unwrap_or_default()
}

fn entry(cmd: Command) -> MenuItem {
    MenuItem::new(cmd.id(), Label::key(cmd.key()))
}

/// The menu of the selected items.
pub fn item_menu(s: &State) -> Menu {
    if s.in_trash() {
        return Menu::new(vec![
            entry(Command::Restore),
            entry(Command::Purge).danger(),
            entry(Command::Properties).separated(),
        ]);
    }
    let mut items = vec![
        entry(Command::Open),
        entry(Command::Cut).separated(),
        entry(Command::Copy),
        entry(Command::Rename),
        entry(Command::Trash).danger(),
        entry(Command::Properties).separated(),
    ];
    if s.edit.clipboard.is_none() {
        items.insert(3, entry(Command::Paste).disabled());
    } else {
        items.insert(3, entry(Command::Paste));
    }
    Menu::new(items)
}

/// The menu of the folder background.
pub fn folder_menu(s: &State) -> Menu {
    if s.in_trash() {
        let mut empty = entry(Command::EmptyTrash).danger();
        if s.items.is_empty() {
            empty = empty.disabled();
        }
        return Menu::new(vec![empty]);
    }
    let mut paste = entry(Command::Paste);
    if s.edit.clipboard.is_none() {
        paste = paste.disabled();
    }
    let mut undo = entry(Command::Undo);
    if !s.edit.undo.can_undo() {
        undo = undo.disabled();
    }
    Menu::new(vec![
        entry(Command::NewFolder),
        paste,
        entry(Command::SelectAll),
        undo,
        entry(Command::Properties).separated(),
    ])
}
