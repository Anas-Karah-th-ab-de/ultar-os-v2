//! Repeated pieces: the desktop card of one app, the compact row, the
//! publisher line and the status badge (boards Store / CompactStore).

use super::{app_name, icon_by_name, loc_label, muted, HitMap};
use crate::action::StoreAction;
use crate::catalog::CatalogApp;
use crate::state::Store;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{ColorRole, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Badge, Button, ButtonVariant, Label, ListItem, Text, Tone};

/// The status pill of an app (board: green check `Installed`, grey `Not
/// installed`; an available update the accent pill).
pub(crate) fn badge_for(store: &Store, id: &str) -> Badge {
    if store.is_installed(id) {
        if store.update_available(id) {
            Badge::new(Label::key("store.badge.update")).tone(Tone::Accent)
        } else {
            Badge::new(Label::key("store.badge.installed"))
                .tone(Tone::Success)
                .with_icon()
        }
    } else {
        Badge::new(Label::key("store.badge.not-installed")).tone(Tone::Neutral)
    }
}

/// The publisher line `{publisher} · verified`: the signature of the
/// catalogue attests its publisher rows (offline first: an unverifiable
/// catalogue never loads — [`crate::source`]).
pub(crate) fn publisher_line(store: &Store, app: &CatalogApp) -> Label {
    let key = if store.catalog.verified {
        "store.publisher.verified"
    } else {
        "store.publisher.unverified"
    };
    Label::key_with(key, &[("publisher", app.publisher.as_str())])
}

/// The desktop card (board Store). Cards never install directly: the header
/// opens the app page (`store.open`); installed apps get an Open button.
pub(crate) fn card_of(
    ui: &mut Ui,
    hits: &mut HitMap,
    parent: NodeId,
    store: &Store,
    app: &CatalogApp,
) -> Result<(), UiError> {
    // Board Store card: radius 12 (card), padding 12, gap 8, surface fill.
    let body = ui.add_sized(
        parent,
        ultar_ui::widgets::Card::new().flex(
            Flex::column()
                .gap(Space::S8)
                .padding(Padding::all(Space::S12)),
        ),
        Sizing::grow(1),
    )?;
    // Header: icon + name + publisher as one clickable row (the board wraps
    // all three in the `store.open` button).
    let header = ui.add(
        body,
        ListItem::new(app_name(store, &app.id, ui))
            .sublabel(publisher_line(store, app))
            .icon(icon_by_name(&app.icon)),
    )?;
    hits.bind(header, StoreAction::Open(app.id.clone()));
    let details = Label::key_with("store.aria.details", &[("name", &resolved(&app.name, ui))]);
    ui.set_accessible_name(header, Some(details))?;
    muted(
        ui,
        body,
        loc_label(&app.summary, ui),
        ultar_ui::tokens::TypeSize::S12,
    )?;
    // Footer: badge, size (`—` = run time), Open / Update.
    let footer = ui.add(body, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    ui.add(footer, badge_for(store, &app.id))?;
    ui.add(
        footer,
        Text::new(Label::key("store.size.unknown")).color(ColorRole::TextMuted),
    )?;
    ui.add_sized(footer, Flex::row(), Sizing::grow(1))?;
    if let Some(button) = footer_button(store, app) {
        let node = ui.add(footer, button)?;
        hits.bind(node, footer_action(store, &app.id));
        if store.is_installed(&app.id) && !store.update_available(&app.id) {
            let open = Label::key_with("store.aria.open", &[("name", &resolved(&app.name, ui))]);
            ui.set_accessible_name(node, Some(open))?;
        }
    }
    Ok(())
}

fn resolved(loc: &crate::catalog::Loc, ui: &Ui) -> String {
    loc.text(ui.env().lang).to_string()
}

fn footer_button(store: &Store, app: &CatalogApp) -> Option<Button> {
    if store.update_available(&app.id) {
        Some(Button::new(Label::key("store.app.update")))
    } else if store.is_installed(&app.id) {
        Some(Button::new(Label::key("store.open")))
    } else {
        None // cards never install directly (CLICKS store.open note)
    }
}

fn footer_action(store: &Store, id: &str) -> StoreAction {
    if store.update_available(id) {
        StoreAction::Install(id.to_string())
    } else {
        StoreAction::Launch(id.to_string())
    }
}

/// The compact row (board CompactStore): the whole line opens the app page;
/// trailing status pill; the chevron marks that it opens a page.
pub(crate) fn compact_row(
    ui: &mut Ui,
    hits: &mut HitMap,
    list: NodeId,
    store: &Store,
    app: &CatalogApp,
) -> Result<(), UiError> {
    let item = ui.add(
        list,
        ListItem::new(app_name(store, &app.id, ui))
            .sublabel(publisher_line(store, app))
            .icon(icon_by_name(&app.icon))
            .chevron(),
    )?;
    hits.bind(item, StoreAction::Open(app.id.clone()));
    ui.add(item, badge_for(store, &app.id))?;
    let details = Label::key_with("store.aria.details", &[("name", &resolved(&app.name, ui))]);
    ui.set_accessible_name(item, Some(details))?;
    Ok(())
}

/// The installed-app row with a trailing Open button (`store.launch`; boards
/// SurfaceStates offline/error panels and the Installed page).
pub(crate) fn launch_row(
    ui: &mut Ui,
    hits: &mut HitMap,
    list: NodeId,
    store: &Store,
    id: &str,
) -> Result<(), UiError> {
    let item = ui.add(
        list,
        ListItem::new(app_name(store, id, ui)).icon(catalog_icon(store, id)),
    )?;
    let open = ui.add(
        item,
        Button::new(Label::key("store.open")).variant(ButtonVariant::Quiet),
    )?;
    hits.bind(open, StoreAction::Launch(id.to_string()));
    let aria = Label::key_with(
        "store.aria.open",
        &[("name", &resolved_name(store, id, ui))],
    );
    ui.set_accessible_name(open, Some(aria))?;
    Ok(())
}

fn resolved_name(store: &Store, id: &str, ui: &Ui) -> String {
    match store.entry(id) {
        Some(app) => resolved(&app.name, ui),
        None => id.to_string(),
    }
}

/// A search/category result row: opens the app page; the trailing control
/// reflects the app state (Open / Update / status pill).
pub(crate) fn result_row(
    ui: &mut Ui,
    hits: &mut HitMap,
    list: NodeId,
    store: &Store,
    app: &CatalogApp,
) -> Result<(), UiError> {
    let item = ui.add(
        list,
        ListItem::new(app_name(store, &app.id, ui))
            .sublabel(loc_label(&app.summary, ui))
            .icon(icon_by_name(&app.icon)),
    )?;
    hits.bind(item, StoreAction::Open(app.id.clone()));
    match footer_button(store, app) {
        Some(button) => {
            let node = ui.add(item, button)?;
            hits.bind(node, footer_action(store, &app.id));
        }
        None => {
            ui.add(item, badge_for(store, &app.id))?;
        }
    }
    Ok(())
}

pub(crate) fn catalog_icon(store: &Store, id: &str) -> IconId {
    store
        .entry(id)
        .map(|a| icon_by_name(&a.icon))
        .unwrap_or(IconId::Package)
}
