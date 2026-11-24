//! The app page (board CompactStoreApp, DESKTOP-SPEC §7.3): icon, name,
//! publisher, the one main action (Install → progress with percent → Open ·
//! Remove), size/version/source, description and the permissions in plain
//! words. The page is built the same in both layouts; the wide layout puts
//! it into the content column.

use super::{
    app_name, icon_by_name, loc_label, main_action, muted, running_progress, size_text, spacer,
    HitMap, MainAction,
};
use crate::action::StoreAction;
use crate::catalog::CatalogApp;
use crate::state::Store;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, Icon, IconButton, Label, ListItem, Text,
};

pub(crate) fn build(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let Some(id) = store.open_app.clone() else {
        return Ok(());
    };
    let Some(app) = store.entry(&id).cloned() else {
        return installed_only_page(ui, parent, store, &id, hits);
    };
    // Back row (boards: `store.back` icon button at the start).
    let back = ui.add(
        parent,
        IconButton::new(IconId::Back, Label::key("store.aria.back")),
    )?;
    hits.bind(back, StoreAction::Back);
    // Icon tile + name + publisher.
    let head = ui.add(parent, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    let tile = ui.add_sized(
        head,
        Icon::new(icon_by_name(&app.icon)).name(Label::key_with(
            "store.aria.app-icon",
            &[("name", &super::loc_label(&app.name, ui).resolve(ui.translator()).into_owned())],
        )),
        Sizing::width(48.0),
    )?;
    let _ = tile;
    let names = ui.add_sized(head, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    ui.add(
        names,
        Text::new(app_name(store, &app.id, ui)).spec(TextSpec::title(TypeSize::S20)),
    )?;
    ui.add(
        names,
        Text::new(crate::view::rows::publisher_line(store, &app)).color(ColorRole::TextMuted),
    )?;
    main_button_row(ui, parent, store, &app, hits)?;
    if let Some(err) = failed_reason(store, &app.id) {
        ui.add(
            parent,
            Text::new(Label::key(err)).color(ColorRole::Danger),
        )?;
    }
    stats_row(ui, parent, store, &app)?;
    muted(ui, parent, loc_label(&app.description, ui), TypeSize::S14)?;
    if !app.requires.is_empty() {
        requires_row(ui, parent, store, &app, hits)?;
    }
    permissions_card(ui, parent, store, &app)?;
    muted(ui, parent, Label::key("store.note.app-page"), TypeSize::S12)?;
    Ok(())
}

fn failed_reason(store: &Store, id: &str) -> Option<&'static str> {
    use crate::state::OpState;
    match store.operation.as_ref().and_then(|o| o.state(id)) {
        Some(OpState::Failed) => store.last_error.as_ref().map(|e| e.message_key()),
        _ => None,
    }
}

/// The one main action with its progress ring (CLICKS `store.install`:
/// install → ring with percent on the button, Cancel next to it →
/// Open · Remove).
fn main_button_row(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    app: &CatalogApp,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let line = ui.add(parent, Flex::row().gap(Space::S8))?;
    let progress = running_progress(store, &app.id);
    let install_button = |ui: &mut Ui, key: &'static str| -> Result<(NodeId, ButtonVariant), UiError> {
        let button = ui.add_sized(
            line,
            Button::new(Label::key(key)).variant(ButtonVariant::Primary),
            Sizing::grow(1),
        )?;
        Ok((button, ButtonVariant::Primary))
    };
    match (main_action(store, &app.id), progress) {
        (MainAction::Install, Some(percent)) | (MainAction::Update, Some(percent)) => {
            let (button, _) = install_button(ui, operation_key(main_action(store, &app.id)))?;
            ui.set_loading(button, true)?;
            if let Some(fraction) = percent {
                ui.set_progress(button, Some(fraction))?;
            }
            let cancel = ui.add(
                line,
                Button::new(Label::key("store.app.cancel")).variant(ButtonVariant::Plain),
            )?;
            hits.bind(cancel, StoreAction::Cancel);
        }
        (MainAction::Install, None) => {
            let (button, _) = install_button(ui, "store.app.install")?;
            hits.bind(button, StoreAction::Install(app.id.clone()));
        }
        (MainAction::Update, None) => {
            let (button, _) = install_button(ui, "store.app.update")?;
            hits.bind(button, StoreAction::Install(app.id.clone()));
        }
        (MainAction::Open, _) => {
            let open = ui.add(
                line,
                Button::new(Label::key("store.open")).variant(ButtonVariant::Plain),
            )?;
            hits.bind(open, StoreAction::Launch(app.id.clone()));
            let remove = ui.add(
                line,
                Button::new(Label::key("store.app.remove")).variant(ButtonVariant::Danger),
            )?;
            hits.bind(remove, StoreAction::Remove(app.id.clone()));
        }
        (MainAction::InstalledOnly, _) => {}
    }
    Ok(())
}

fn operation_key(action: MainAction) -> &'static str {
    match action {
        MainAction::Update => "store.app.update",
        _ => "store.app.install",
    }
}

/// Size · version · source row (board: three centred columns; unknown values
/// are the dash, as the board footnote defines).
fn stats_row(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    app: &CatalogApp,
) -> Result<(), UiError> {
    let line = ui.add(parent, Flex::row().gap(Space::S8))?;
    let size = store
        .installed_app(&app.id)
        .and_then(|i| i.size)
        .or(app.size_installed);
    stat_cell(ui, line, "store.app.size", &size_text(size, ui))?;
    let version = store
        .installed_app(&app.id)
        .map(|i| i.version.clone())
        .or_else(|| app.version.clone())
        .unwrap_or_else(|| super::DASH.to_string());
    stat_cell(ui, line, "store.app.version", &version)?;
    stat_cell(ui, line, "store.app.source", app.source.label())?;
    Ok(())
}

fn stat_cell(ui: &mut Ui, parent: NodeId, key: &str, value: &str) -> Result<(), UiError> {
    let cell = ui.add_sized(
        parent,
        Flex::column().gap(Space::S4).cross(CrossAlign::Center),
        Sizing::grow(1),
    )?;
    ui.add(
        cell,
        Text::new(Label::key(key))
            .spec(TextSpec::new(TypeSize::S12))
            .color(ColorRole::TextMuted),
    )?;
    ui.add(cell, Text::new(Label::data(value)))?;
    Ok(())
}

/// "Permissions in plain words" card (board CompactStoreApp): each right
/// named from the app's language catalog, with its catalogue reason.
fn permissions_card(
    ui: &mut Ui,
    parent: NodeId,
    _store: &Store,
    app: &CatalogApp,
) -> Result<(), UiError> {
    let card = ui.add(parent, Card::new())?;
    let head = ui.add(
        card,
        Flex::row().gap(Space::S16).cross(CrossAlign::Center),
    )?;
    ui.add(
        head,
        Text::new(Label::key("store.app.permissions")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    let _ = head;
    for permission in &app.permissions {
        let row = ui.add(card, ListItem::new(permission_label(&permission.id)))?;
        ui.add(
            row,
            Text::new(loc_label(&permission.reason, ui)).color(ColorRole::TextMuted),
        )?;
    }
    ui.add(
        card,
        Text::new(Label::key("store.app.permissions-hint")).color(ColorRole::TextMuted),
    )?;
    Ok(())
}

/// A permission's plain-word name: the app catalog key when present, the raw
/// id otherwise (never a failure: unknown ids are shown as data).
fn permission_label(id: &str) -> Label {
    Label::key(format!("store.perm.{id}"))
}

/// "Needs X" row (board note "Fallback browser. Needs Linux programs."):
/// each requirement opens its app page (`store.open`).
fn requires_row(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    app: &CatalogApp,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let line = ui.add(parent, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    ui.add(
        line,
        Text::new(Label::key("store.app.requires")).color(ColorRole::TextMuted),
    )?;
    for dep in &app.requires {
        let button = ui.add(
            line,
            Button::new(app_name(store, dep, ui)).variant(ButtonVariant::Quiet),
        )?;
        hits.bind(button, StoreAction::Open(dep.clone()));
    }
    Ok(())
}

/// An installed app the catalogue no longer carries: version, Open, Remove.
fn installed_only_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    id: &str,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let back = ui.add(
        parent,
        IconButton::new(IconId::Back, Label::key("store.aria.back")),
    )?;
    hits.bind(back, StoreAction::Back);
    ui.add(
        parent,
        Text::new(app_name(store, id, ui)).spec(TextSpec::title(TypeSize::S20)),
    )?;
    let Some(entry) = store.installed_app(id) else {
        return Ok(());
    };
    ui.add(
        parent,
        Text::new(Label::key_with(
            "store.app.installed-version",
            &[("version", &entry.version)],
        ))
        .color(ColorRole::TextMuted),
    )?;
    let line = ui.add(parent, Flex::row().gap(Space::S8))?;
    let open = ui.add(
        line,
        Button::new(Label::key("store.open")).variant(ButtonVariant::Plain),
    )?;
    hits.bind(open, StoreAction::Launch(id.to_string()));
    let remove = ui.add(
        line,
        Button::new(Label::key("store.app.remove")).variant(ButtonVariant::Danger),
    )?;
    hits.bind(remove, StoreAction::Remove(id.to_string()));
    let _ = spacer(ui, line);
    Ok(())
}
