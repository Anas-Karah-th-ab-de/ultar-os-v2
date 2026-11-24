//! The overview (board Overview, Super): the search field with the caret, the workspace
//! strip and the windows of the workspace; while typing, the results replace strip and
//! windows, grouped (apps, settings, calculation, commands).

use super::geometry;
use super::{new_ui, Look, Target, View};
use crate::model::search::{self, Hit};
use crate::model::Desktop;
use ultar_i18n::Translator;
use ultar_surface::proto::WindowState;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, IconButton, Label, List, ListItem, Picture, SearchField, Text,
};

/// Width of the search field (`g(140)`).
pub const SEARCH_WIDTH: f32 = 140.0 * Space::S4.points();
/// Width of a window preview.
pub const PREVIEW_WIDTH: f32 = 60.0 * Space::S4.points();
/// Most windows shown.
pub const MAX_PREVIEWS: usize = 12;

/// Builds the overview for the work area of `screen`.
pub fn build(
    d: &Desktop,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, (i32, i32)), UiError> {
    let (area, offset) = geometry::work_area(screen);
    let mut ui = new_ui(look, area, tr)?;
    ui.set_background(ColorRole::Ground);
    ui.set_root_flex(
        Flex::column()
            .gap(Space::S24)
            .padding(Padding::symmetric(Space::S48, Space::S24)),
    );
    let mut view = View::new(ui);
    let root = view.ui.root();
    let top = view.ui.add(root, Flex::row().main(MainAlign::Center))?;
    let field = SearchField::new(Label::key("desk.ov.search"))
        .placeholder(Label::key("desk.ov.search"))
        .text(&d.query);
    let field = view.ui.add_sized(top, field, Sizing::width(SEARCH_WIDTH))?;
    view.mark(field, Target::SearchField);
    if d.query.trim().is_empty() {
        strip(&mut view, root)?;
        windows(&mut view, root, d)?;
    } else {
        results(&mut view, root, d, tr)?;
    }
    Ok((view, offset))
}

fn strip(view: &mut View, root: NodeId) -> Result<(), UiError> {
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S16).main(MainAlign::Center))?;
    let current = Brick::new(
        BrickKind::ModeCard {
            picture: Picture::Icon(IconId::Workspaces),
            chosen: true,
        },
        Label::key_with("desk.ov.workspace", &[("n", "1")]),
    );
    let id = view
        .ui
        .add_sized(row, current, Sizing::width(48.0 * Space::S4.points()))?;
    view.mark(id, Target::Workspace(1));
    let new = Brick::new(
        BrickKind::ModeCard {
            picture: Picture::Icon(IconId::Plus),
            chosen: false,
        },
        Label::key("desk.ov.new_workspace"),
    );
    let id = view
        .ui
        .add_sized(row, new, Sizing::width(48.0 * Space::S4.points()))?;
    view.mark(id, Target::Workspace(2));
    view.ui.set_disabled(id, true)?;
    view.ui
        .set_tooltip(id, Some(Label::key("desk.reason.no_workspaces")))?;
    Ok(())
}

fn windows(view: &mut View, root: NodeId, d: &Desktop) -> Result<(), UiError> {
    let row = Flex::row()
        .gap(Space::S32)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let row = view.ui.add_sized(root, row, Sizing::grow(1))?;
    let shown: Vec<_> = d
        .windows
        .iter()
        .filter(|w| w.state != WindowState::Minimized)
        .take(MAX_PREVIEWS)
        .collect();
    if shown.is_empty() {
        view.ui.add(
            row,
            Text::new(Label::key("desk.ov.no_windows")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    for w in shown {
        let icon = d
            .catalog
            .iter()
            .find(|a| a.id == w.app_id)
            .map_or(IconId::Apps, |a| a.icon);
        let cell = view
            .ui
            .add(row, Flex::column().gap(Space::S8).cross(CrossAlign::End))?;
        let close_name = Label::key_with("desk.ov.close", &[("title", &w.title)]);
        let close = view
            .ui
            .add(cell, IconButton::new(IconId::Close, close_name))?;
        view.mark(close, Target::CloseWindow(w.id));
        let preview = Brick::new(
            BrickKind::ModeCard {
                picture: Picture::Icon(icon),
                chosen: w.focused,
            },
            Label::data(&w.title),
        );
        let id = view
            .ui
            .add_sized(cell, preview, Sizing::width(PREVIEW_WIDTH))?;
        view.mark(id, Target::Window(w.id));
    }
    Ok(())
}

fn hit_item(hit: &Hit, d: &Desktop, tr: &Translator) -> ListItem {
    match hit {
        Hit::App(id) => {
            let app = d.catalog.iter().find(|a| &a.id == id);
            let name = app.map_or(id.clone(), |a| a.display_name(tr));
            ListItem::new(Label::data(&name))
                .icon(app.map_or(IconId::Apps, |a| a.icon))
                .sublabel(Label::key("desk.ov.group.apps"))
        }
        Hit::Setting(panel) => ListItem::new(Label::key(format!("desk.search.panel.{panel}")))
            .icon(IconId::Gear)
            .sublabel(Label::key("desk.ov.group.settings")),
        Hit::Calc(value) => ListItem::new(Label::key_with("desk.ov.calc", &[("value", value)]))
            .icon(IconId::Plus)
            .sublabel(Label::key("desk.ov.group.calc")),
        Hit::Command(c) => ListItem::new(Label::key(c.key()))
            .icon(IconId::Forward)
            .sublabel(Label::key("desk.ov.group.commands")),
    }
}

fn results(view: &mut View, root: NodeId, d: &Desktop, tr: &Translator) -> Result<(), UiError> {
    let hits = search::search(&d.query, &d.catalog, tr);
    let row = view.ui.add(root, Flex::row().main(MainAlign::Center))?;
    if hits.is_empty() {
        view.ui.add(
            row,
            Text::new(Label::key("desk.ov.no_results")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    let list = List::new().name(Label::key("desk.ov.results"));
    let list = view.ui.add_sized(row, list, Sizing::width(SEARCH_WIDTH))?;
    for hit in hits {
        let id = view.ui.add(list, hit_item(&hit, d, tr))?;
        view.mark(id, Target::Result(hit));
    }
    Ok(())
}
