//! The Discover area (board Store): "Components by Ultar" at the top, the
//! grid of app cards, the category section and the run-time-size note.

use super::{chrome, is_compact, rows, HitMap};
use crate::action::StoreAction;
use crate::state::Store;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Label, List, Text, Widget};

/// Cards per row on the desktop board (grid: `repeat(4, 1fr)`).
const GRID_COLUMNS: usize = 4;

pub(crate) fn discover_page(
    ui: &mut Ui,
    parent: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    if store.catalog.data.is_none() {
        return chrome::page_title_only(ui, parent);
    }
    let scroll = ui.add_sized(parent, ultar_ui::widgets::Scroll::new(), Sizing::grow(1))?;
    let body = ui.add_sized(scroll, Flex::column().gap(Space::S16), Sizing::grow(1))?;
    // Section header: title + one-line description (board order and text).
    let head = ui.add(body, Flex::column().gap(Space::S4))?;
    ui.add(
        head,
        Text::new(Label::key("store.section.components")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    ui.add(
        head,
        Text::new(Label::key("store.section.components.desc")).color(ColorRole::TextMuted),
    )?;
    if is_compact(ui) {
        // Phone layout (board CompactStore): one list of clickable rows.
        let list = ui.add_sized(
            body,
            List::new().name(Label::key("store.section.components")),
            Sizing::grow(1),
        )?;
        for app in store.apps() {
            rows::compact_row(ui, hits, list, store, app)?;
        }
        // A single "more" affordance lives in the chrome, not here.
        return Ok(());
    }
    // The card grid, four per row.
    for chunk in store.apps().chunks(GRID_COLUMNS) {
        let line = ui.add(body, Flex::row().gap(Space::S12).cross(CrossAlign::Stretch))?;
        for app in chunk {
            rows::card_of(ui, hits, line, store, app)?;
        }
        if chunk.len() < GRID_COLUMNS {
            for _ in chunk.len()..GRID_COLUMNS {
                ui.add_sized(line, Widget::Flex(Flex::column()), Sizing::grow(1))?;
            }
        }
    }
    // Categories (part of Discover per DESKTOP-SPEC §7.3 "Empfehlungen, Kategorien").
    let chips_head = ui.add(body, Flex::column().gap(Space::S4))?;
    ui.add(
        chips_head,
        Text::new(Label::key("store.section.categories")).spec(TextSpec::title(TypeSize::S16)),
    )?;
    ui.add(
        chips_head,
        Text::new(Label::key("store.section.categories.desc")).color(ColorRole::TextMuted),
    )?;
    let chips = ui.add(body, Flex::row().gap(Space::S8).wrap())?;
    for category in store.categories() {
        let shown = category_display(store, category, ui);
        let chip = ui.add(
            chips,
            Button::new(Label::data(&shown)).variant(ButtonVariant::Quiet),
        )?;
        hits.bind(chip, StoreAction::Category(category.to_string()));
    }
    // Run-time-size note (board last line).
    ui.add(
        body,
        Text::new(Label::key("store.note.discover")).color(ColorRole::TextMuted),
    )?;
    Ok(())
}

/// Category name translated if the catalog carries it, otherwise the id.
fn category_display(_store: &Store, category: &str, ui: &Ui) -> String {
    let key = format!("store.category.{category}");
    match ui.translator().try_t(&key) {
        Some(text) if !text.is_empty() => text.into_owned(),
        _ => category.to_string(),
    }
}
