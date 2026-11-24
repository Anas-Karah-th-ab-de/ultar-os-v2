//! The frame of the window: sidebar (search, areas) and content pane; on compact surfaces
//! either the list of areas or one area page (DESKTOP-SPEC §8, boards *CompactSettings*).

use super::{k, metrics, View};
use crate::action::Act;
use crate::areas::AREAS;
use crate::state::State;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::tokens::{SizeClass, Space};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{List, ListItem, Scroll, SearchField};

/// Longest search text in bytes.
pub const MAX_QUERY: usize = 128;

/// `true` when the surface is phone-sized.
pub fn compact(ui: &Ui) -> bool {
    ui.size_class() == SizeClass::Compact
}

/// Builds sidebar and content pane.
pub fn build(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    let compact = compact(ui);
    ui.set_root_flex(Flex::row().cross(CrossAlign::Stretch));
    let root = ui.root();
    if !compact || state.compact_list {
        let sizing = if compact {
            Sizing::grow(1)
        } else {
            Sizing::width(metrics::sidebar())
        };
        let scroll = Scroll::new()
            .flex(
                Flex::column()
                    .padding(Padding::all(Space::S12))
                    .gap(Space::S4),
            )
            .name(k("app.settings.areas"));
        let side = ui.add_sized(root, scroll, sizing)?;
        let field = SearchField::new(k("app.settings.search"))
            .placeholder(k("app.settings.search"))
            .text(&state.query)
            .max_len(MAX_QUERY);
        if compact {
            let title = ultar_ui::widgets::Text::new(k("app.settings.name"))
                .spec(super::spec::page_title());
            ui.add(side, title)?;
        }
        let search = ui.add(side, field)?;
        view.shell.add(search, Act::Search);
        view.search = Some(search);
        let list = if compact {
            List::new().name(k("app.settings.areas"))
        } else {
            List::navigation().name(k("app.settings.areas"))
        };
        let parent = if compact {
            ui.add(side, ultar_ui::widgets::Card::new())?
        } else {
            side
        };
        let nav = ui.add(parent, list)?;
        view.nav = Some(nav);
        fill_nav(ui, state, view, nav, compact)?;
    }
    if !compact || !state.compact_list {
        let main = ui.add_sized(root, Scroll::new(), Sizing::grow(1))?;
        view.main = Some(main);
    }
    Ok(())
}

/// Area indices matching the search text (all when it is empty).
pub fn matching_areas(ui: &Ui, state: &State) -> Vec<usize> {
    let order = if compact(ui) {
        crate::areas::compact()
    } else {
        crate::areas::wide()
    };
    let query = state.query.trim().to_lowercase();
    if query.is_empty() {
        return order;
    }
    let tr = ui.translator();
    order
        .into_iter()
        .map(|i| (i, &AREAS[i]))
        .filter(|(_, area)| {
            let own = tr.t(&area.label_key()).to_lowercase().contains(&query);
            let rows = match area.page {
                crate::areas::PageKind::Panel(id) => state.panel(id).is_some_and(|p| {
                    p.items
                        .iter()
                        .any(|i| tr.t(&i.label.key).to_lowercase().contains(&query))
                }),
                _ => false,
            };
            own || rows
        })
        .map(|(i, _)| i)
        .collect()
}

fn fill_nav(
    ui: &mut Ui,
    state: &State,
    view: &mut View,
    nav: NodeId,
    compact: bool,
) -> Result<(), UiError> {
    view.nav_areas = matching_areas(ui, state);
    for &index in &view.nav_areas {
        let area = &AREAS[index];
        let mut item = ListItem::new(k(&area.label_key())).icon(area.icon);
        if index == state.area && !compact {
            item = item.selected();
        }
        if compact {
            let summary = format!("app.settings.summary.{}", area.id);
            if ui.translator().has(&summary) {
                item = item.sublabel(k(&summary));
            }
        }
        let id = ui.add(nav, item)?;
        view.shell.add(id, Act::Area(index));
    }
    Ok(())
}

/// Rebuilds the items of the navigation list (search text changed).
pub fn refresh_nav(ui: &mut Ui, state: &State, view: &mut View) -> Result<(), UiError> {
    let Some(nav) = view.nav else {
        return Ok(());
    };
    for child in ui.children(nav).to_vec() {
        ui.remove(child)?;
    }
    view.shell.retain(|_, act| !matches!(act, Act::Area(_)));
    let compact = compact(ui);
    fill_nav(ui, state, view, nav, compact)
}
