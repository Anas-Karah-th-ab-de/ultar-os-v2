//! The phone's search and recent apps (CLICKS `mobile.search`, `mobile.home` held): the
//! search field with the same sources as the overview's search; while it is empty, the
//! open apps to go back to.

use super::content_area;
use crate::model::search;
use crate::model::Desktop;
use crate::view::{new_ui, overview, Look, Target, View};
use ultar_i18n::Translator;
use ultar_surface::proto::WindowState;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{Flex, Padding};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Label, List, ListItem, SearchField, Text};

/// Most recent apps listed.
pub const MAX_RECENT: usize = 8;

/// Builds the search for the space between the bars; returns the view and its size.
pub fn build(
    d: &Desktop,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Size), UiError> {
    let area = content_area(screen);
    let mut ui = new_ui(look, area, tr)?;
    ui.set_background(ColorRole::Ground);
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S12))
        .gap(Space::S16);
    ui.set_root_flex(main);
    let mut view = View::new(ui);
    let root = view.ui.root();
    let field = SearchField::new(Label::key("desk.mobile.search"))
        .placeholder(Label::key("desk.mobile.search"))
        .text(&d.overview.query);
    let field = view.ui.add(root, field)?;
    view.mark(field, Target::SearchField);
    if d.overview.query.trim().is_empty() {
        recent(&mut view, root, d)?;
    } else {
        results(&mut view, root, d, tr)?;
    }
    Ok((view, area))
}

fn muted(view: &mut View, root: NodeId, key: &str) -> Result<(), UiError> {
    view.ui
        .add(root, Text::new(Label::key(key)).color(ColorRole::TextMuted))?;
    Ok(())
}

/// The open apps, the one used last first.
fn recent(view: &mut View, root: NodeId, d: &Desktop) -> Result<(), UiError> {
    let title = Text::new(Label::key("desk.mobile.recent"))
        .spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(root, title)?;
    if d.windows.is_empty() {
        return muted(view, root, "desk.ov.no_windows");
    }
    let list = view
        .ui
        .add(root, List::new().name(Label::key("desk.mobile.recent")))?;
    for w in d.windows.iter().rev().take(MAX_RECENT) {
        let app = d.catalog.iter().find(|a| a.id == w.app_id);
        let mut item =
            ListItem::new(Label::data(&w.title)).icon(app.map_or(IconId::Apps, |a| a.icon));
        if let Some(app) = app {
            item = item.sublabel(app.label());
        }
        if w.focused && w.state != WindowState::Minimized {
            item = item.selected();
        }
        let id = view.ui.add(list, item)?;
        view.mark(id, Target::Window(w.id));
    }
    Ok(())
}

fn results(view: &mut View, root: NodeId, d: &Desktop, tr: &Translator) -> Result<(), UiError> {
    let hits = search::search(&d.overview.query, &d.catalog, tr);
    if hits.is_empty() {
        return muted(view, root, "desk.ov.no_results");
    }
    let list = view
        .ui
        .add(root, List::new().name(Label::key("desk.ov.results")))?;
    for hit in hits {
        let id = view.ui.add(list, overview::hit_item(&hit, d, tr))?;
        view.mark(id, Target::Result(hit));
    }
    Ok(())
}
