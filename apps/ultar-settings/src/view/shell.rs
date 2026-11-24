//! The frame of the window: sidebar (search, areas) and content pane; on compact surfaces
//! either the list of areas or one area page (DESKTOP-SPEC §8, boards *CompactSettings*).

use super::{k, metrics, View};
use crate::action::Act;
use crate::areas::AREAS;
use crate::state::State;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, SizeClass, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{BarEdge, Card, List, ListItem, Scroll, SearchField, Text};

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
        let flex = if compact {
            Flex::column()
                .padding(super::compact_padding())
                .gap(Space::S16)
        } else {
            Flex::column()
                .padding(Padding::all(Space::S12))
                .gap(Space::S4)
        };
        let scroll = Scroll::new().flex(flex).name(k("app.settings.areas"));
        // Boards: the sidebar is a bar region (`bar` fill, `line` border at its end edge).
        let side = if compact {
            ui.add_sized(root, scroll, sizing)?
        } else {
            let bar = ui.add_sized(root, Card::bar(BarEdge::End), sizing)?;
            ui.add_sized(bar, scroll, Sizing::grow(1))?
        };
        search_row(ui, state, view, side, compact)?;
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

/// The page title (compact) and the search field at the top of the sidebar.
fn search_row(
    ui: &mut Ui,
    state: &State,
    view: &mut View,
    side: NodeId,
    compact: bool,
) -> Result<(), UiError> {
    let field = SearchField::new(k("app.settings.search"))
        .placeholder(k("app.settings.search"))
        .text(&state.query)
        .max_len(MAX_QUERY);
    if compact {
        let touch = ultar_ui::tokens::size::TARGET_TOUCH_MIN;
        let row = ui.add_sized(
            side,
            Flex::row().cross(CrossAlign::Center),
            Sizing::AUTO.with_height(touch),
        )?;
        let title =
            ultar_ui::widgets::Text::new(k("app.settings.name")).spec(super::spec::page_title());
        ui.add(row, title)?;
    }
    let search = ui.add(side, field)?;
    view.shell.add(search, Act::Search);
    view.search = Some(search);
    Ok(())
}

/// `false` for an area whose page is a settingsd panel the service does not offer on this
/// device (its hardware is absent: no modem, no pointer, no display). While the service
/// cannot be read every area stays, so its page can say why.
pub fn offered(state: &State, area: usize) -> bool {
    match AREAS.get(area).map(|a| a.page) {
        Some(crate::areas::PageKind::Panel(id)) => {
            state.panels_error.is_some() || state.panels.is_empty() || state.panel(id).is_some()
        }
        _ => true,
    }
}

/// Area indices matching the search text (all offered ones when it is empty).
pub fn matching_areas(ui: &Ui, state: &State) -> Vec<usize> {
    let order = if compact(ui) {
        crate::areas::compact()
    } else {
        crate::areas::wide()
    };
    let order: Vec<usize> = order.into_iter().filter(|&i| offered(state, i)).collect();
    let query = state.query.trim().to_lowercase();
    if query.is_empty() {
        return order;
    }
    let tr = ui.translator();
    order
        .into_iter()
        .map(|i| (i, &AREAS[i]))
        .filter(|(i, area)| {
            let i = *i;
            let own = tr.t(&area.label_key()).to_lowercase().contains(&query);
            let group = crate::areas::group_of(i).is_some_and(|g| {
                let heading = crate::areas::GROUPS[g].label_key();
                tr.t(&heading).to_lowercase().contains(&query)
            });
            let rows = match area.page {
                crate::areas::PageKind::Panel(id) => state.panel(id).is_some_and(|p| {
                    p.items
                        .iter()
                        .any(|item| tr.t(&item.label.key).to_lowercase().contains(&query))
                }),
                _ => false,
            };
            own || group || rows
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
    let mut shown_group = None;
    for &index in &view.nav_areas {
        let area = &AREAS[index];
        let group = crate::areas::group_of(index);
        if !compact && group != shown_group {
            if let Some(g) = group {
                heading(ui, nav, g, shown_group.is_none())?;
            }
            shown_group = group;
        }
        let mut item = ListItem::new(k(&area.label_key())).icon(area.icon);
        if index == state.area && !compact {
            item = item.selected();
        }
        if compact {
            if let Some(summary) = summary(ui, state, area.id) {
                item = item.sublabel(summary);
            }
        }
        let id = if compact {
            // Boards (`arow`): at least a row high (56), padded 8, a hairline under all but
            // the last; the arrow at the end opens the area's page.
            let lines = if item.sublabel.is_some() { 1.0 } else { 0.0 };
            let texts = super::lines_height(ui, TypeSize::S14, 1.0)
                + super::lines_height(ui, TypeSize::S12, lines);
            let last = index == *view.nav_areas.last().unwrap_or(&index);
            let rule = if last { 0.0 } else { 1.0 };
            let h = (texts + 2.0 * Space::S8.points() + rule).max(ultar_ui::tokens::size::ROW_MIN);
            let mut item = item.chevron();
            if !last {
                item = item.separator();
            }
            ui.add_sized(nav, item, Sizing::AUTO.with_height(h))?
        } else {
            ui.add(nav, item)?
        };
        view.shell.add(id, Act::Area(index));
    }
    Ok(())
}

/// The heading of a group in the wide sidebar: small, semibold, muted, aligned with the item
/// labels; more space above it than between items, except above the first.
fn heading(ui: &mut Ui, nav: NodeId, group: usize, first: bool) -> Result<(), UiError> {
    let padding = Padding {
        top: Some(if first { Space::S4 } else { Space::S12 }),
        bottom: Some(Space::S4),
        ..Padding::symmetric(Space::S12, Space::S4)
    };
    let row = ui.add(nav, Flex::column().padding(padding))?;
    let text = Text::new(k(&crate::areas::GROUPS[group].label_key()))
        .spec(TextSpec::new(TypeSize::S12).weight(Weight::Semibold))
        .color(ColorRole::TextMuted);
    ui.add(row, text)?;
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

/// The one-line state of an area in the compact list (boards *CompactSettings*): the theme
/// from settingsd; for areas whose service reports the state (network name, SIM, Bluetooth,
/// battery) "not available" while that service is missing.
fn summary(ui: &Ui, state: &State, id: &str) -> Option<ultar_ui::widgets::Label> {
    match id {
        "wifi" | "cellular" | "bluetooth" | "power" => Some(k("app.settings.summary.no_service")),
        "appearance" => {
            let item = state
                .panels
                .iter()
                .flat_map(|p| p.items.iter())
                .find(|i| i.key == "appearance.theme")?;
            let value = match &item.value {
                ultar_bus::messages::settings::SettingValue::Text(t) => t.clone(),
                _ => return None,
            };
            let key = format!("settings.appearance.theme.choice.{value}");
            ui.translator().has(&key).then(|| k(&key))
        }
        _ => {
            let key = format!("app.settings.summary.{id}");
            ui.translator().has(&key).then(|| k(&key))
        }
    }
}
