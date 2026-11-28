//! The Discover area (board Store): "Components by Ultar" at the top, the
//! grid of app cards, the category section and the run-time-size note.

use super::{chrome, is_compact, rows, HitMap};
use crate::action::StoreAction;
use crate::state::Store;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, InfoBar, Label, List, Scroll, Text, Tone, Widget};

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
    let scroll = ui.add_sized(parent, Scroll::new(), Sizing::grow(1))?;
    let body = ui.add_sized(scroll, Flex::column().gap(Space::S16), Sizing::grow(1))?;
    chosen_hint(ui, body, store)?;
    section_head(
        ui,
        body,
        "store.section.components",
        Some("store.section.components.desc"),
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
        return Ok(());
    }
    components_grid(ui, body, store, hits)?;
    // Categories (part of Discover per DESKTOP-SPEC §7.3 "Empfehlungen, Kategorien").
    section_head(
        ui,
        body,
        "store.section.categories",
        Some("store.section.categories.desc"),
    )?;
    categories_chips(ui, body, store, hits)?;
    // Run-time-size note (board last line).
    ui.add(
        body,
        Text::new(Label::key("store.note.discover")).color(ColorRole::TextMuted),
    )?;
    Ok(())
}

/// The hint above the components when some chosen at the installation are not installed
/// yet: their cards carry the accent badge, the app page installs each with its rights shown.
fn chosen_hint(ui: &mut Ui, body: NodeId, store: &Store) -> Result<(), UiError> {
    let missing = store.chosen_missing().len();
    if missing == 0 {
        return Ok(());
    }
    let count = missing.to_string();
    let bar = InfoBar::new(
        Tone::Info,
        Label::key_with("store.chosen.title", &[("count", count.as_str())]),
    )
    .message(Label::key("store.chosen.message"));
    ui.add(body, bar).map(|_| ())
}

/// A section heading: title + one-line description (board order and sizes).
pub(crate) fn section_head(
    ui: &mut Ui,
    parent: NodeId,
    title: &str,
    desc: Option<&str>,
) -> Result<(), UiError> {
    let head = ui.add(parent, Flex::column().gap(Space::S4))?;
    ui.add(
        head,
        Text::new(Label::key(title)).spec(TextSpec::title(TypeSize::S16)),
    )?;
    if let Some(desc) = desc {
        ui.add(
            head,
            Text::new(Label::key(desc)).color(ColorRole::TextMuted),
        )?;
    }
    Ok(())
}

/// The card grid, four per row (board Store).
fn components_grid(
    ui: &mut Ui,
    body: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    for chunk in store.apps().chunks(GRID_COLUMNS) {
        let line = ui.add(body, Flex::row().gap(Space::S12).cross(CrossAlign::Stretch))?;
        for app in chunk {
            rows::card_of(ui, hits, line, store, app)?;
        }
        // Keep the grid even when the last row is short.
        for _ in chunk.len()..GRID_COLUMNS {
            ui.add_sized(line, Widget::Flex(Flex::column()), Sizing::grow(1))?;
        }
    }
    Ok(())
}

/// The category chips: each opens the Categories page filtered to it.
fn categories_chips(
    ui: &mut Ui,
    body: NodeId,
    store: &Store,
    hits: &mut HitMap,
) -> Result<(), UiError> {
    let chips = ui.add(body, Flex::row().gap(Space::S8).wrap())?;
    for category in store.categories() {
        let shown = category_display(category, ui);
        let chip = ui.add(
            chips,
            Button::new(Label::data(&shown)).variant(ButtonVariant::Quiet),
        )?;
        hits.bind(chip, StoreAction::Category(category.to_string()));
    }
    Ok(())
}

/// Category name translated if the catalog carries it, otherwise the id.
pub(crate) fn category_display(category: &str, ui: &Ui) -> String {
    let key = format!("store.category.{category}");
    match ui.translator().try_t(&key) {
        Some(text) if !text.is_empty() => text.into_owned(),
        _ => category.to_string(),
    }
}
