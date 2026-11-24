//! All apps on a phone (board CompactApps): the search field, the categories as tabs, the
//! apps in four columns in the order of the language, and how to reach an app's menu.

use super::{app_cell, cell_rows, content_area};
use crate::model::apps::Category;
use crate::model::arrange::Folder;
use crate::model::Desktop;
use crate::view::{apps as grid, new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Overflow;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Card, Icon, IconButton, Label, SearchField, Text};

/// Builds the grid for the space between the bars; returns the view and its size.
pub fn build(
    d: &Desktop,
    category: Option<Category>,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Size), UiError> {
    let area = content_area(screen);
    let mut ui = new_ui(look, area, tr)?;
    ui.set_background(ColorRole::Ground);
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S12))
        .gap(Space::S16);
    ui.set_root_flex(main);
    let mut view = View::new(ui);
    let root = view.ui.root();
    let field = SearchField::new(Label::key("desk.apps.search"))
        .placeholder(Label::key("desk.apps.search"))
        .text(&d.overview.query);
    let field = view.ui.add(root, field)?;
    view.mark(field, Target::SearchField);
    grid::tabs(&mut view, root, d, category)?;
    match d.home.open_folder.and_then(|id| d.home.folder(id)) {
        Some(folder) => folder_page(&mut view, root, d, folder)?,
        None => cells(&mut view, root, d, category, tr)?,
    }
    view.ui.add_sized(root, Flex::column(), Sizing::grow(1))?;
    let hint = view
        .ui
        .add(root, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    view.ui.add(
        hint,
        Icon::new(IconId::Info).small().color(ColorRole::TextMuted),
    )?;
    let text = Text::new(Label::key("desk.mobile.apps_hint"))
        .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap))
        .color(ColorRole::TextMuted);
    // A column takes what the icon leaves, so that the text wraps inside it.
    let lines = view.ui.add_sized(hint, Flex::column(), Sizing::grow(1))?;
    view.ui.add(lines, text)?;
    Ok((view, area))
}

/// The apps of the category; in "All" without a search the apps of a folder are one
/// folder cell at the place of its first app (CLICKS `apps.app`: drag onto another app).
fn cells(
    view: &mut View,
    root: NodeId,
    d: &Desktop,
    category: Option<Category>,
    tr: &Translator,
) -> Result<(), UiError> {
    let list = grid::listed(d, category, &d.overview.query, tr);
    if list.is_empty() {
        let none = Text::new(Label::key("desk.apps.none")).color(ColorRole::TextMuted);
        view.ui.add(root, none)?;
        return Ok(());
    }
    let grouped = category.is_none() && d.overview.query.is_empty();
    let mut shown: Vec<Cell<'_>> = Vec::new();
    for app in &list {
        match d.home.folder_of(&app.id).filter(|_| grouped) {
            Some(f)
                if shown
                    .iter()
                    .any(|c| matches!(c, Cell::Folder(o) if o.id == f.id)) => {}
            Some(f) => shown.push(Cell::Folder(f)),
            None => shown.push(Cell::App(app)),
        }
    }
    let rows = view.ui.add(root, Flex::column().gap(Space::S8))?;
    for chunk in shown.chunks(super::COLUMNS) {
        let row = view.ui.add(rows, Flex::row().gap(Space::S8))?;
        for cell in chunk {
            match cell {
                Cell::App(app) => {
                    let id = view.ui.add_sized(
                        row,
                        phone_cell(app.icon, app.label()),
                        Sizing::grow(1),
                    )?;
                    app_cell(view, id, d, app)?;
                }
                Cell::Folder(f) => folder_cell(view, row, f)?,
            }
        }
        for _ in chunk.len()..super::COLUMNS {
            view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
        }
    }
    Ok(())
}

/// One cell of all apps.
enum Cell<'a> {
    App(&'a crate::model::apps::AppInfo),
    Folder(&'a Folder),
}

fn phone_cell(icon: IconId, label: Label) -> Brick {
    Brick::new(BrickKind::PhoneCell { icon, open: false }, label)
}

fn folder_name(f: &Folder) -> Label {
    Label::key_with("desk.folder.name", &[("number", &f.id.to_string())])
}

/// A folder: opens on a click, takes a dragged app.
fn folder_cell(view: &mut View, row: NodeId, f: &Folder) -> Result<(), UiError> {
    let id = view.ui.add_sized(
        row,
        phone_cell(IconId::Folder, folder_name(f)),
        Sizing::grow(1),
    )?;
    view.mark(id, Target::Folder(f.id));
    view.ui.set_drop_target(id, true)
}

/// The open folder: its name with "back", its apps, and the place that takes an app out.
fn folder_page(view: &mut View, root: NodeId, d: &Desktop, f: &Folder) -> Result<(), UiError> {
    let head = view
        .ui
        .add(root, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back = view.ui.add(
        head,
        IconButton::new(IconId::Back, Label::key("desk.folder.close")),
    )?;
    view.mark(back, Target::FolderClose);
    view.ui.add(head, Text::new(folder_name(f)))?;
    let apps = crate::model::apps::listed(&f.apps, &d.catalog);
    let rows = view.ui.add(root, Flex::column().gap(Space::S8))?;
    cell_rows(view, rows, &apps, |v, id, app| app_cell(v, id, d, app))?;
    let out = Card::padded().flex(Flex::row().main(MainAlign::Center));
    let out = view.ui.add(root, out)?;
    let text = Text::new(Label::key("desk.folder.out")).color(ColorRole::TextMuted);
    view.ui.add(out, text)?;
    view.mark(out, Target::FolderOut);
    view.ui.set_drop_target(out, true)
}
