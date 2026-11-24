//! The app grid (board AppGrid; DESKTOP-SPEC §5.1; CLICKS `apps.*`): a search field, the
//! categories as tabs, the apps of the bar ("On the bar") and every app in alphabetical
//! order of the language, each a cell with a context menu (open, new window, pin, details
//! in the Store, remove), and at the bottom how the grid is ordered and its keys.

use super::geometry::{self, Placement};
use super::{new_ui, Look, Target, View};
use crate::model::apps::{self, AppInfo, Category};
use crate::model::Desktop;
use ultar_i18n::{arabic, Translator};
use ultar_ui::geom::{Rect, Size};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, Brick, BrickKind, Card, Icon, Label, Menu, MenuItem, SearchField, Tabs, Text,
};

/// Cells per row.
pub const PER_ROW: usize = 8;
/// Rows of "all apps" shown (the rest is reached by searching or a category).
pub const MAX_ROWS: usize = 4;
/// Card width (board: 800).
pub const WIDTH: f32 = 200.0 * Space::S4.points();
/// Distance of the card from the top of the screen.
pub const TOP: Space = Space::S32;

/// Items of an app's context menu.
pub mod item {
    /// Open (start or raise).
    pub const OPEN: u32 = 1;
    /// New window.
    pub const NEW_WINDOW: u32 = 2;
    /// Add to / remove from the bar.
    pub const PIN: u32 = 3;
    /// Details in the Store.
    pub const DETAILS: u32 = 4;
    /// Remove the app.
    pub const UNINSTALL: u32 = 5;
}

/// The categories that have apps (tabs after "All").
pub fn categories(d: &Desktop) -> Vec<Category> {
    Category::ALL
        .into_iter()
        .filter(|c| d.catalog.iter().any(|a| a.category == Some(*c)))
        .collect()
}

/// The apps listed under "all apps" for a query and category.
pub fn listed(
    d: &Desktop,
    category: Option<Category>,
    query: &str,
    tr: &Translator,
) -> Vec<AppInfo> {
    apps::sorted(&d.catalog, tr)
        .into_iter()
        .filter(|a| category.is_none_or(|c| a.category == Some(c)))
        .filter(|a| {
            query.trim().is_empty() || arabic::search_matches(&a.display_name(tr), query.trim())
        })
        .take(PER_ROW * MAX_ROWS)
        .collect()
}

/// Where the card is: centred, from [`TOP`] down to the sheets' distance above the bar.
fn card_rect(screen: Size) -> Rect {
    let margin = Space::S16.points();
    let w = WIDTH.min(screen.w - 2.0 * margin).max(0.0);
    let top = TOP.points();
    let h = (screen.h - geometry::SHEET_BOTTOM - top).max(0.0);
    Rect::new(((screen.w - w) / 2.0).max(0.0), top, w, h)
}

/// Builds the grid.
pub fn build(
    d: &Desktop,
    category: Option<Category>,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_transparent_background();
    let rect = card_rect(screen);
    let place = geometry::place(screen, rect, look.lang.direction().is_rtl());
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start).padding(Padding {
        start: Some(place.start),
        end: None,
        top: Some(place.top),
        bottom: None,
    }));
    let flex = Flex::column()
        .padding(Padding::all(Space::S24))
        .gap(Space::S16);
    let card = Card::new().floating().flex(flex);
    let root = ui.root();
    let card = ui.add_sized(root, card, Sizing::width(rect.w).with_height(rect.h))?;
    ui.set_size(place.surface)?;
    let mut view = View::new(ui);
    let field = SearchField::new(Label::key("desk.apps.search"))
        .placeholder(Label::key("desk.apps.search"))
        .text(&d.overview.query);
    let field = view.ui.add(card, field)?;
    view.mark(field, Target::SearchField);
    tabs(&mut view, card, d, category)?;
    let body = view
        .ui
        .add_sized(card, Flex::column().gap(Space::S16), Sizing::grow(1))?;
    if d.overview.query.trim().is_empty() && category.is_none() {
        let pinned: Vec<AppInfo> = d
            .pinned
            .iter()
            .filter_map(|id| d.catalog.iter().find(|a| &a.id == id))
            .cloned()
            .collect();
        section(&mut view, body, d, "desk.apps.pinned", &pinned)?;
    }
    let list = listed(d, category, &d.overview.query, tr);
    if list.is_empty() {
        view.ui.add(
            body,
            Text::new(Label::key("desk.apps.none")).color(ColorRole::TextMuted),
        )?;
    } else {
        section(&mut view, body, d, "desk.apps.title", &list)?;
    }
    footer(&mut view, card)?;
    Ok((view, place))
}

pub(crate) fn tabs(
    view: &mut View,
    card: NodeId,
    d: &Desktop,
    category: Option<Category>,
) -> Result<(), UiError> {
    let cats = categories(d);
    let mut labels = vec![Label::key("desk.apps.cat.all")];
    labels.extend(cats.iter().map(|c| Label::key(c.key())));
    let selected = category
        .and_then(|c| cats.iter().position(|x| *x == c))
        .map_or(0, |i| i + 1);
    let tabs = Tabs::new(Label::key("desk.apps.categories"), labels).selected(selected);
    // The tab strip keeps its own width at the start (board: tabs at the start of a rule).
    let strip = view.ui.add(card, Flex::row().cross(CrossAlign::End))?;
    let id = view.ui.add(strip, tabs)?;
    view.mark(id, Target::Categories);
    Ok(())
}

fn section(
    view: &mut View,
    body: NodeId,
    d: &Desktop,
    title: &str,
    list: &[AppInfo],
) -> Result<(), UiError> {
    let block = view
        .ui
        .add_sized(body, Flex::column().gap(Space::S8), Sizing::AUTO)?;
    let title =
        Text::new(Label::key(title)).spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(block, title)?;
    for chunk in list.chunks(PER_ROW) {
        let row = view
            .ui
            .add_sized(block, Flex::row().gap(Space::S8), Sizing::AUTO)?;
        for app in chunk {
            // The grid shows no window dots (board AppGrid): the bar does.
            let cell = Brick::new(
                BrickKind::AppCell {
                    icon: app.icon,
                    running: 0,
                    open: false,
                },
                app.label(),
            );
            let id = view.ui.add_sized(row, cell, Sizing::grow(1))?;
            view.mark(id, Target::AppItem(app.id.clone()));
            view.ui.set_context_menu(id, Some(menu(d, app)))?;
            if !d.presence.launcher && !d.windows.iter().any(|w| w.app_id == app.id) {
                view.ui.set_disabled(id, true)?;
                view.ui
                    .set_tooltip(id, Some(Label::key("desk.reason.no_launcher")))?;
            }
        }
        // Keep the columns of a short last row.
        for _ in chunk.len()..PER_ROW {
            view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
        }
    }
    Ok(())
}

/// The context menu of an app cell.
pub fn menu(d: &Desktop, app: &AppInfo) -> Menu {
    menu_for(d, app, false)
}

/// The context menu of an app cell; on a phone the bar is the favourites row.
pub fn menu_for(d: &Desktop, app: &AppInfo, phone: bool) -> Menu {
    let launcher = d.presence.launcher;
    let gate = |i: MenuItem| if launcher { i } else { i.disabled() };
    let pin = match (d.pinned.contains(&app.id), phone) {
        (true, false) => "desk.menu.unpin",
        (false, false) => "desk.menu.pin",
        (true, true) => "desk.menu.unfavourite",
        (false, true) => "desk.menu.favourite",
    };
    // Details and removal open the app's page in the Store (`store://app/<id>`, with
    // `?action=remove` its removal dialog: the Store holds the right to uninstall). Apps of
    // the base system cannot be removed.
    let removable = |i: MenuItem| if app.base { i.disabled() } else { gate(i) };
    Menu::new(vec![
        gate(MenuItem::new(item::OPEN, Label::key("desk.menu.open"))),
        gate(MenuItem::new(
            item::NEW_WINDOW,
            Label::key("desk.menu.new_window"),
        )),
        MenuItem::new(item::PIN, Label::key(pin)).separated(),
        gate(MenuItem::new(
            item::DETAILS,
            Label::key("desk.apps.details"),
        )),
        removable(
            MenuItem::new(item::UNINSTALL, Label::key("desk.apps.uninstall"))
                .danger()
                .separated(),
        ),
    ])
}

fn footer(view: &mut View, card: NodeId) -> Result<(), UiError> {
    let row = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .main(MainAlign::SpaceBetween);
    let row = view.ui.add_sized(card, row, Sizing::AUTO)?;
    let hint = view
        .ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    view.ui.add(hint, Icon::new(IconId::Info).small())?;
    let small = TextSpec::new(TypeSize::S12);
    view.ui.add(
        hint,
        Text::new(Label::key("desk.apps.hint"))
            .spec(small)
            .color(ColorRole::TextMuted),
    )?;
    let keys = view
        .ui
        .add(row, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    for (caps, what) in [
        (&["Super", "A"][..], "desk.apps.keys.open"),
        (&["Esc"][..], "desk.apps.keys.close"),
    ] {
        for cap in caps {
            view.ui.add(keys, Badge::new(Label::data(cap)))?;
        }
        view.ui.add(
            keys,
            Text::new(Label::key(what))
                .spec(small)
                .color(ColorRole::TextMuted),
        )?;
    }
    Ok(())
}
