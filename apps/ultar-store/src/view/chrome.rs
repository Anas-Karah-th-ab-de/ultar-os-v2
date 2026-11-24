//! The frame: side nav (wide, board Store) or tabs + more menu (compact,
//! board CompactStore); the title row with the search field. The window
//! header (`win.*`) and the phone bars (`mobile.*`) belong to the
//! compositor/shell, not to this app.

use super::{bind_add, states, HitMap, NAV_WIDTH};
use crate::action::StoreAction;
use crate::state::{Area, Store};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    BarEdge, Card, IconButton, Label, List, ListItem, SearchField, Tabs, Text,
};

/// The six areas of the desktop nav, with the icons of ICONS.md.
const WIDE_AREAS: [(Area, IconId); 6] = [
    (Area::Discover, IconId::Compass),
    (Area::Installed, IconId::Installed),
    (Area::Updates, IconId::Update),
    (Area::InstallFile, IconId::PackageFile),
    (Area::OfflineSources, IconId::Usb),
    (Area::Account, IconId::User),
];

/// The three compact tabs (board CompactStore).
pub(crate) const COMPACT_TABS: [Area; 3] = [Area::Discover, Area::Installed, Area::Updates];

/// The areas of the `store.more` menu (CLICKS CompactStore #1).
pub(crate) const MORE_AREAS: [(Area, IconId); 3] = [
    (Area::InstallFile, IconId::PackageFile),
    (Area::OfflineSources, IconId::Usb),
    (Area::Account, IconId::User),
];

pub(crate) fn area_key(area: Area) -> &'static str {
    match area {
        Area::Discover => "store.area.discover",
        Area::Categories => "store.area.categories",
        Area::Search => "store.search.placeholder",
        Area::AppPage => "app.store.name",
        Area::Installed => "store.area.installed",
        Area::Updates => "store.area.updates",
        Area::InstallFile => "store.area.install-file",
        Area::OfflineSources => "store.area.offline-sources",
        Area::Account => "store.area.account",
    }
}

pub(crate) fn area_icon(area: Area) -> IconId {
    MORE_AREAS
        .iter()
        .find(|(a, _)| *a == area)
        .map(|(_, i)| *i)
        .or_else(|| WIDE_AREAS.iter().find(|(a, _)| *a == area).map(|(_, i)| *i))
        .unwrap_or(IconId::Compass)
}

/// Desktop: nav column + page column (board Store geometry).
pub(crate) fn wide_root(
    ui: &mut Ui,
    root: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    // The nav and the page span the full window height (cross axis stretch;
    // the row default centres).
    ui.set_root_flex(Flex::row().cross(CrossAlign::Stretch));
    nav(ui, root, store, hits)?;
    let page = ui.add_sized(
        root,
        Flex::column()
            .gap(Space::S16)
            .padding(Padding::symmetric(Space::S32, Space::S24)),
        Sizing::grow(1),
    )?;
    let head = ui.add(page, Flex::row().gap(Space::S16).cross(CrossAlign::Center))?;
    ui.add(
        head,
        Text::new(Label::key(page_title_key(store))).spec(TextSpec::title(TypeSize::S24)),
    )?;
    ui.add_sized(head, Flex::row(), Sizing::grow(1))?;
    search_field(ui, head, store, hits, Some(320.0))?;
    states::surface_body(ui, page, store, hits)
}

fn page_title_key(_store: &Store) -> &'static str {
    "store.discover.title"
}

fn nav(ui: &mut Ui, root: NodeId, store: &Store, hits: &mut HitMap) -> Result<NodeId, UiError> {
    let current = current_nav_area(store);
    // Board Store: 240 wide sidebar, `bar` fill, `line` border on the inline
    // end, padding 12, gap 4 — that is `Card::bar(BarEdge::End)`.
    let sidebar = ui.add_sized(
        root,
        Card::bar(BarEdge::End).flex(
            Flex::column()
                .gap(Space::S4)
                .padding(Padding::all(Space::S12)),
        ),
        Sizing::width(NAV_WIDTH),
    )?;
    let list = ui.add(
        sidebar,
        List::navigation().name(Label::key("store.window.title")),
    )?;
    for (area, icon) in WIDE_AREAS {
        let mut item = ListItem::new(Label::key(area_key(area))).icon(icon);
        if area == current {
            item = item.selected();
        }
        let node = ui.add(list, item)?;
        hits.bind(node, StoreAction::Area(area));
    }
    Ok(list)
}

/// The app page highlights the nav entry it came from.
fn current_nav_area(store: &Store) -> Area {
    match store.area {
        Area::AppPage => store.back_to.unwrap_or(Area::Discover),
        Area::Search | Area::Categories => Area::Discover,
        other => other,
    }
}

/// The search field (boards: 320 wide on desktop, full width on the phone).
pub(crate) fn search_field(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
    width: Option<f32>,
) -> Result<(), UiError> {
    let field = SearchField::new(Label::key("store.search.aria"))
        .placeholder(Label::key("store.search.placeholder"))
        .text(&store.query);
    let sizing = match width {
        Some(w) => Sizing::width(w),
        None => Sizing::grow(1),
    };
    let node = ui.add_sized(parent, field, sizing)?;
    hits.set_search(node);
    Ok(())
}

/// Phone frame: title + `store.more`, search, tabs, body (board CompactStore).
pub(crate) fn compact_root(
    ui: &mut Ui,
    root: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    ui.set_root_flex(
        Flex::column()
            .gap(Space::S16)
            .padding(Padding::symmetric(Space::S16, Space::S12)),
    );
    let head = ui.add(root, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let title_box = ui.add_sized(head, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    ui.add(
        title_box,
        Text::new(Label::key("app.store.name")).spec(TextSpec::title(TypeSize::S24)),
    )?;
    let more = bind_add(
        ui,
        hits,
        head,
        IconButton::new(IconId::More, Label::key("store.aria.more-areas")),
        StoreAction::More,
    )?;
    hits.set_menu(more, MORE_AREAS.iter().map(|(a, _)| *a).collect());
    let bar = ui.add(root, Flex::row().gap(Space::S8))?;
    search_field(ui, bar, store, hits, None)?;
    let tab_labels: Vec<Label> = COMPACT_TABS
        .iter()
        .map(|a| Label::key(area_key(*a)))
        .collect();
    let selected = COMPACT_TABS
        .iter()
        .position(|a| *a == current_nav_area(store))
        .unwrap_or(0);
    let tabs = ui.add(
        root,
        Tabs::new(Label::key("store.window.title"), tab_labels).selected(selected),
    )?;
    hits.set_tabs(tabs, COMPACT_TABS.to_vec());
    states::surface_body(ui, root, store, hits)
}

/// A title-only page (empty catalogue fallback).
pub(crate) fn page_title_only(ui: &mut Ui, parent: NodeId) -> Result<(), UiError> {
    ui.add(
        parent,
        Text::new(Label::key("store.discover.title")).spec(TextSpec::title(TypeSize::S24)),
    )?;
    Ok(())
}

/// The page for the Ready surface: the area decides its content.
pub(crate) fn page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    match store.area {
        Area::AppPage if store.open_app.is_some() => {
            super::app_page::build(ui, parent, store, hits)
        }
        Area::Discover => super::discover::discover_page(ui, parent, store, hits),
        Area::Categories => super::lists::categories_page(ui, parent, store, hits),
        Area::Search => super::lists::search_page(ui, parent, store, hits),
        Area::Installed => super::lists::installed_page(ui, parent, store, hits),
        Area::Updates => super::lists::updates_page(ui, parent, store, hits),
        Area::InstallFile => super::lists::install_file_page(ui, parent, store, hits),
        Area::OfflineSources => super::lists::offline_sources_page(ui, parent, store, hits),
        Area::Account => super::lists::account_page(ui, parent, store, hits),
        _ => page_title_only(ui, parent),
    }
}
