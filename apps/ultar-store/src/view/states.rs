//! The page states of the board "SurfaceStates" (dark, Arabic): loading, no
//! search results, offline, error with reason, not enough space. Each panel
//! is built exactly as drawn: icon, title, reason, buttons.

use super::{
    chrome, column, muted, no_space_app, no_space_open, rows, size_text, HitMap,
};
use crate::action::StoreAction;
use crate::state::{Area, Store, Surface};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, Checkbox, Dialog, Icon, InfoBar, Label, List, Text, Tone,
};

/// The body dispatch: which surface the user sees right now.
pub(crate) fn surface_body(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    if no_space_open(store) {
        return no_space_panel(ui, parent, store, hits);
    }
    match &store.surface {
        Surface::Loading => loading_panel(ui, parent),
        Surface::Error { key } => error_panel(ui, parent, store, hits, key),
        Surface::Offline => offline_panel(ui, parent, store, hits),
        Surface::Ready => {
            if store.area == Area::Search && !store.query.is_empty() {
                if crate::update::visible_results(store).is_empty() {
                    return no_results_panel(ui, parent, store, hits);
                }
            }
            chrome::page(ui, parent, store, hits)
        }
    }
}

/// Loading: centred ring with the store's loading line (panel 1).
pub(crate) fn loading_panel(ui: &mut Ui, parent: NodeId) -> Result<(), UiError> {
    centered(ui, parent, |ui, inner| {
        let ring = ui.add(inner, Icon::new(IconId::Ring).color(ColorRole::TextMuted))?;
        let _ = ring;
        ui.add_sized(inner, ultar_ui::widgets::Progress::new(Label::key("store.state.loading")), Sizing::width(24.0))?;
        ui.add(
            inner,
            Text::new(Label::key("store.state.loading")).color(ColorRole::TextMuted),
        )?;
        Ok(())
    })
}

/// A centred column for the state panels.
pub(crate) fn centered(
    ui: &mut Ui,
    parent: NodeId,
    f: impl FnOnce(&mut Ui, NodeId) -> Result<(), UiError>,
) -> Result<(), UiError> {
    let inner = ui.add_sized(
        parent,
        Flex::column()
            .gap(Space::S12)
            .main(MainAlign::Center)
            .cross(CrossAlign::Center),
        Sizing::grow(1),
    )?;
    f(ui, inner)
}

/// No results for «query» (panel 2): icon, question, hint, `store.search.clear`.
pub(crate) fn no_results_panel(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    centered(ui, parent, |ui, inner| {
        ui.add(inner, Icon::new(IconId::Search).color(ColorRole::TextMuted))?;
        ui.add(
            inner,
            Text::new(Label::key_with(
                "store.search.no-results",
                &[("query", &store.query)],
            ))
            .spec(TextSpec::title(TypeSize::S16)),
        )?;
        ui.add(
            inner,
            Text::new(Label::key_with(
                "store.search.no-results-hint",
                &[("ext", ".xlsx")],
            ))
            .color(ColorRole::TextMuted),
        )?;
        let line = ui.add(inner, Flex::row().gap(Space::S8))?;
        let clear = ui.add(line, Button::new(Label::key("store.search.clear")))?;
        hits.bind(clear, StoreAction::SearchClear);
        Ok(())
    })
}

/// The installed list of the offline and error panels (panels 3 and 4).
fn installed_list(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    ui.add(
        parent,
        Text::new(Label::key("store.installed.header")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    let list = ui.add_sized(
        parent,
        List::new().name(Label::key("store.installed.header")),
        Sizing::grow(1),
    )?;
    for entry in &store.installed {
        rows::launch_row(ui, hits, list, store, &entry.id)?;
    }
    Ok(())
}

/// Offline (panel 3): warning banner + retry inside it, installed list,
/// offline sources row with `store.offline` ("Browse").
pub(crate) fn offline_panel(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let body = column(ui, parent, Space::S12, Sizing::grow(1))?;
    let bar = ui.add(
        body,
        InfoBar::new(Tone::Warning, Label::key("store.offline.banner.title"))
            .message(Label::key("store.offline.banner.desc")),
    )?;
    let retry = ui.add(bar, Button::new(Label::key("store.retry")))?;
    hits.bind(retry, StoreAction::Retry);
    installed_list(ui, body, store, hits)?;
    // Board row: usb icon · "Offline sources / USB stick: signed catalogue" · Browse.
    let src = ui.add(body, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    ui.add(src, Icon::new(IconId::Usb).color(ColorRole::TextMuted))?;
    let src_text = ui.add_sized(src, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    ui.add(src_text, Text::new(Label::key("store.offline.sources")))?;
    muted(ui, src_text, Label::key("store.offline.usb"), TypeSize::S12)?;
    let browse = ui.add(src, Button::new(Label::key("store.offline.browse")))?;
    hits.bind(browse, StoreAction::Offline);
    Ok(())
}

/// Error with reason (panel 4): banner with cause, `store.retry` + `store.log`
/// below, installed list under that.
pub(crate) fn error_panel(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
    key: &str,
) -> Result<(), UiError> {
    let body = column(ui, parent, Space::S12, Sizing::grow(1))?;
    // The reason is shown resolved, never as a bare key.
    let reason = ui.translator().t(key).into_owned();
    ui.add(
        body,
        InfoBar::new(Tone::Danger, Label::key("store.error.title"))
            .message(Label::key_with("store.error.reason", &[("reason", &reason)])),
    )?;
    let row = ui.add(body, Flex::row().gap(Space::S8))?;
    let retry = ui.add(row, Button::new(Label::key("store.retry")))?;
    hits.bind(retry, StoreAction::Retry);
    let log = ui.add(row, Button::new(Label::key("store.log")))?;
    hits.bind(log, StoreAction::Log);
    if let Some(path) = &store.log_saved {
        muted(
            ui,
            body,
            Label::key_with("store.log.saved", &[("path", path)]),
            TypeSize::S12,
        )?;
    }
    installed_list(ui, body, store, hits)?;
    Ok(())
}

/// Not enough space (panel 5): warning banner with required/available, the
/// checkbox list of what can be freed, the note, Cancel / Free-and-go
/// (`store.free.item`, `store.free.cancel`, `store.free.go`).
pub(crate) fn no_space_panel(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let body = column(ui, parent, Space::S12, Sizing::grow(1))?;
    ui.add(
        body,
        InfoBar::new(
            Tone::Warning,
            Label::key_with(
                "store.nospace.title",
                &[("name", &no_space_name(store, ui))],
            ),
        )
        .message(Label::key_with("store.nospace.sizes", &[
            ("required", &size_text(store.free.required, ui)),
            ("available", &size_text(store.free.available, ui)),
        ])),
    )?;
    ui.add(
        body,
        Text::new(Label::key("store.free.heading")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    let items_box = ui.add(body, Card::padded())?;
    let items_col = ui.add_sized(items_box, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    for entry in &store.free.entries {
        let line = ui.add(
            items_col,
            Flex::row().gap(Space::S12).cross(CrossAlign::Center),
        )?;
        let check = ui.add_sized(
            line,
            Checkbox::new(Label::key(free_item_key(&entry.id)), entry.selected),
            Sizing::grow(1),
        )?;
        hits.bind(check, StoreAction::FreeItem(entry.id.clone()));
        ui.add(
            line,
            Text::new(Label::data(&size_text(entry.size, ui))).color(ColorRole::TextMuted),
        )?;
    }
    muted(ui, body, Label::key("store.free.note"), TypeSize::S12)?;
    let actions = ui.add(body, Flex::row().gap(Space::S8).main(MainAlign::End))?;
    let cancel = ui.add(
        actions,
        Button::new(Label::key("store.free.cancel")).variant(ButtonVariant::Plain),
    )?;
    hits.bind(cancel, StoreAction::FreeCancel);
    let go = ui.add(
        actions,
        Button::new(Label::key("store.free.go")).variant(ButtonVariant::Primary),
    )?;
    hits.bind(go, StoreAction::FreeGo);
    Ok(())
}

fn no_space_name(store: &Store, ui: &Ui) -> String {
    let id = no_space_app(store).unwrap_or_default();
    match store.entry(id) {
        Some(app) => crate::view::loc_label(&app.name, ui)
            .resolve(ui.translator())
            .into_owned(),
        None => id.to_string(),
    }
}

pub(crate) fn free_item_key(id: &str) -> String {
    format!("store.free.item.{id}")
}

/// The removal confirmation (APP-RULES §6: dangerous actions ask first):
/// "Remove {name}?" with keep-data / delete-data / cancel. Nothing is
/// removed before the user answers.
pub(crate) fn remove_dialog(
    ui: &mut Ui,
    store: &Store,
    hits: &mut HitMap,
    app: &str,
) -> Result<(), UiError> {
    let name = super::resolved_name(store, app, ui);
    let dialog = ui.add(
        ui.root(),
        Dialog::new(Label::key_with("store.remove.dialog", &[("name", &name)])).alert(),
    )?;
    ui.add(
        dialog,
        Text::new(Label::key("store.remove.dialog.desc")).color(ColorRole::TextMuted),
    )?;
    let row = ui.add(dialog, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let keep = ui.add(
        row,
        Button::new(Label::key("store.remove.keep-data")).variant(ButtonVariant::Danger),
    )?;
    hits.bind(
        keep,
        StoreAction::RemoveConfirmed {
            app: app.to_string(),
            keep_data: true,
        },
    );
    let delete = ui.add(
        row,
        Button::new(Label::key("store.remove.delete-data")).variant(ButtonVariant::Danger),
    )?;
    hits.bind(
        delete,
        StoreAction::RemoveConfirmed {
            app: app.to_string(),
            keep_data: false,
        },
    );
    let cancel = ui.add(row, Button::new(Label::key("store.remove.cancel")))?;
    hits.bind(cancel, StoreAction::RemoveCancelled);
    ui.open_dialog(dialog)?;
    // Escape and Enter both take the safe way out: nothing is removed.
    ui.set_cancel_action(dialog, Some(cancel))?;
    ui.set_default_action(dialog, Some(cancel))
}
