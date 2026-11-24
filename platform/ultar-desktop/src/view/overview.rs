//! The overview (board Overview, Super): the search field with the caret, the workspace
//! strip (a miniature per workspace, "new workspace") and the windows of the shown
//! workspace as thumbnails (the compositor's copies, `WindowThumbnail`); while typing, the results replace strip and
//! windows, grouped (apps, settings, calculation, commands).

use super::{new_ui, Look, Target, View};
use crate::model::search::{self, Hit};
use crate::model::Desktop;
use crate::shell::Effect;
use ultar_i18n::Translator;
use ultar_surface::proto::{ShellWindow, WindowState, ALL_WORKSPACES};
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, IconButton, Label, List, ListItem, SearchField, Text, MAX_MINIS, STRIP,
};

/// Width of the search field (`g(140)`).
pub const SEARCH_WIDTH: f32 = 140.0 * Space::S4.points();
/// Most windows shown.
pub const MAX_PREVIEWS: usize = 12;

/// Builds the overview for the work area of `screen`.
pub fn build(d: &Desktop, look: Look, area: Size, tr: &Translator) -> Result<View, UiError> {
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
        .text(&d.overview.query);
    let field = view.ui.add_sized(top, field, Sizing::width(SEARCH_WIDTH))?;
    view.mark(field, Target::SearchField);
    if d.overview.query.trim().is_empty() {
        strip(&mut view, root, d)?;
        windows(&mut view, root, d)?;
    } else {
        results(&mut view, root, d, tr)?;
    }
    Ok(view)
}

/// Largest copy of a window the overview asks for, in pixels.
pub const THUMB_MAX: (u16, u16) = (512, 384);
/// Most workspaces shown (the compositor's limit).
const MAX_WORKSPACES: u8 = ultar_surface::proto::MAX_WORKSPACES as u8;

/// The windows the overview shows: those of the shown workspace (and those on all), not
/// minimised, at most [`MAX_PREVIEWS`].
pub fn shown(d: &Desktop) -> Vec<&ShellWindow> {
    let current = d.overview.workspaces.map_or(0, |w| w.0);
    d.windows
        .iter()
        .filter(|w| w.state != WindowState::Minimized)
        .filter(|w| w.workspace == current || w.workspace == ALL_WORKSPACES)
        .take(MAX_PREVIEWS)
        .collect()
}

/// The request for copies of `ids` (the compositor answers each with a thumbnail).
pub fn thumbnails(_d: &Desktop, ids: &[u32]) -> Effect {
    Effect::Thumbnails(ids.to_vec(), THUMB_MAX)
}

/// The shapes of a workspace's windows in its miniature (points; board: 64 × 48 …).
fn minis(d: &Desktop, workspace: u8) -> Vec<(u8, u8)> {
    d.windows
        .iter()
        .filter(|w| w.workspace == workspace && w.state != WindowState::Minimized)
        .take(MAX_MINIS)
        .map(|w| match d.overview.thumb(w.id) {
            Some(t) if t.height > 0 => {
                let h = 48u32;
                let w = (h * t.width / t.height).clamp(24, 80);
                (w as u8, h as u8)
            }
            _ => (64, 48),
        })
        .collect()
}

fn strip(view: &mut View, root: NodeId, d: &Desktop) -> Result<(), UiError> {
    let row = view
        .ui
        .add(root, Flex::row().gap(Space::S16).main(MainAlign::Center))?;
    let (current, count) = d.overview.workspaces.unwrap_or((0, 1));
    for i in 0..count {
        let n = (u32::from(i) + 1).to_string();
        let card = Brick::new(
            BrickKind::WorkspaceCard {
                minis: minis(d, i),
                chosen: i == current,
                new: false,
            },
            Label::key_with("desk.ov.workspace", &[("n", &n)]),
        );
        let id = view.ui.add(row, card)?;
        view.mark(id, Target::Workspace(u32::from(i)));
    }
    // "New workspace": only while the compositor can add one (and knows workspaces).
    let new = Brick::new(
        BrickKind::WorkspaceCard {
            minis: Vec::new(),
            chosen: false,
            new: true,
        },
        Label::key("desk.ov.new_workspace"),
    );
    let id = view.ui.add(row, new)?;
    view.mark(id, Target::Workspace(u32::from(count)));
    if d.overview.workspaces.is_none() || count >= MAX_WORKSPACES {
        view.ui.set_disabled(id, true)?;
        let reason = if d.overview.workspaces.is_none() {
            "desk.reason.no_workspaces"
        } else {
            "desk.reason.workspaces_full"
        };
        view.ui.set_tooltip(id, Some(Label::key(reason)))?;
    }
    Ok(())
}

/// Card height by how many windows share the row (board: 288 for two or three).
fn card_height(n: usize) -> u16 {
    match n {
        0..=3 => 288,
        4..=6 => 192,
        _ => 144,
    }
}

fn windows(view: &mut View, root: NodeId, d: &Desktop) -> Result<(), UiError> {
    let row = Flex::row()
        .gap(Space::S32)
        .main(MainAlign::Center)
        .cross(CrossAlign::Center);
    let row = view.ui.add_sized(root, row, Sizing::grow(1))?;
    let shown = shown(d);
    if shown.is_empty() {
        let empty = view.ui.add(
            row,
            Flex::column().gap(Space::S16).cross(CrossAlign::Center),
        )?;
        let mark = BrickKind::Mark {
            points: ultar_brand::mark::HERO_SIZE as u16,
        };
        view.ui
            .add(empty, Brick::new(mark, Label::key("brand.label.mark")))?;
        view.ui.add(
            empty,
            Text::new(Label::key("desk.ov.no_windows")).color(ColorRole::TextMuted),
        )?;
        return Ok(());
    }
    let card_h = card_height(shown.len());
    for w in shown {
        let icon = d
            .catalog
            .iter()
            .find(|a| a.id == w.app_id)
            .map_or(IconId::Apps, |a| a.icon);
        let picture = d.overview.thumb(w.id);
        let content = f32::from(card_h) - STRIP;
        let aspect = picture
            .as_ref()
            .map_or(1.7, |t| t.width as f32 / t.height.max(1) as f32);
        let width = (content * aspect).clamp(f32::from(card_h) * 0.75, f32::from(card_h) * 2.0);
        window_card(view, row, w, (icon, picture, card_h), width)?;
    }
    Ok(())
}

/// One window: its picture card with the close button inside the top end corner, inset 4
/// (board Overview), over the picture (toolkit overlay, `Flex::stack`).
fn window_card(
    view: &mut View,
    row: NodeId,
    w: &ultar_surface::proto::ShellWindow,
    (icon, picture, card_h): (
        IconId,
        Option<std::rc::Rc<ultar_ui::widgets::Thumbnail>>,
        u16,
    ),
    width: f32,
) -> Result<(), UiError> {
    // overview.close: the button lies inside the card's top end corner, inset 4
    // (board Overview), over the window's picture (toolkit overlay, `Flex::stack`).
    let cell = Flex::stack().main(MainAlign::End).cross(CrossAlign::Start);
    let cell = view.ui.add(row, cell)?;
    let preview = Brick::new(
        BrickKind::WindowPreview {
            icon,
            picture,
            card_h,
        },
        Label::data(&w.title),
    );
    let id = view.ui.add_sized(cell, preview, Sizing::width(width))?;
    let corner = view
        .ui
        .add(cell, Flex::row().padding(Padding::all(Space::S4)))?;
    let close_name = Label::key_with("desk.ov.close", &[("title", &w.title)]);
    let close = view
        .ui
        .add(corner, IconButton::new(IconId::Close, close_name))?;
    view.mark(close, Target::CloseWindow(w.id));
    view.mark(id, Target::Window(w.id));
    Ok(())
}

pub(crate) fn hit_item(hit: &Hit, d: &Desktop, tr: &Translator) -> ListItem {
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
    let hits = search::search_in(&d.overview.query, &d.catalog, tr, d.overview.sources);
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
