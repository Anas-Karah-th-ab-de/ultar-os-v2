//! The Files window: sidebar of places and drives, the folder toolbar, the items, the job
//! bar, dialogs. Compact screens (phones) drop the sidebar for a places menu.

use super::{dialogs, items, place_icon, Built, SEARCH, SIDEBAR, TOOLBAR};
use crate::action::Target;
use crate::fs::AreaId;
use crate::model::State;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{typography, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, Ui, UiError};
use ultar_ui::widgets::{
    BarEdge, Button, ButtonVariant, Card, IconButton, Label, List, ListItem, Progress, SearchField,
    Text,
};

/// Builds the whole window for `s` (`compact`: phone layout).
pub fn build(ui: &mut Ui, s: &State, compact: bool, fs: &crate::fs::Fs) -> Result<Built, UiError> {
    ui.clear();
    ui.set_root_flex(Flex::row().cross(CrossAlign::Stretch));
    let mut b = Built::default();
    let root = ui.root();
    if !compact {
        sidebar(ui, &mut b, root, s)?;
    }
    let main = ui.add_sized(root, Flex::column(), Sizing::grow(1))?;
    toolbar(ui, &mut b, main, s, compact)?;
    let line = Card::bar(BarEdge::Top);
    ui.add_sized(
        main,
        line,
        Sizing::AUTO.with_height(ultar_ui::tokens::size::BORDER),
    )?;
    items::build(ui, &mut b, main, s)?;
    if let Some(job) = &s.job {
        job_bar(ui, &mut b, main, job.progress.fraction())?;
    }
    dialogs::build(ui, &mut b, s, fs)?;
    Ok(b)
}

fn sidebar(ui: &mut Ui, b: &mut Built, root: NodeId, s: &State) -> Result<(), UiError> {
    let flex = Flex::column()
        .padding(Padding::all(Space::S12))
        .gap(Space::S4);
    let nav = ui.add_sized(
        root,
        Card::bar(BarEdge::End).flex(flex),
        Sizing::width(SIDEBAR),
    )?;
    let list = ui.add_sized(
        nav,
        List::navigation().name(Label::key("files.places")),
        Sizing::grow(1),
    )?;
    let last = s.places.len().saturating_sub(1);
    for (i, area) in s.places.iter().enumerate() {
        let mut item = ListItem::new(Label::key(area.label_key())).icon(place_icon(area));
        if s.folder.area == *area {
            item = item.selected();
        }
        if i == last && (!s.drives.is_empty() || s.has_trash) {
            item = item.separator();
        }
        let id = ui.add(list, item)?;
        b.mark(id, Target::Place(i));
    }
    for (i, d) in s.drives.iter().enumerate() {
        let mut item = ListItem::new(Label::key_with("files.place.drive", &[("name", &d.name)]))
            .icon(IconId::Usb);
        if s.folder.area == d.area {
            item = item.selected();
        }
        let id = ui.add(list, item)?;
        b.mark(id, Target::Drive(i));
        let name = Label::key_with("files.eject", &[("name", &d.name)]);
        let eject = ui.add(id, IconButton::new(IconId::Eject, name))?;
        b.mark(eject, Target::Eject(i));
    }
    if s.has_trash {
        let mut item = ListItem::new(Label::key("files.place.trash")).icon(IconId::Bin);
        if s.in_trash() {
            item = item.selected();
        }
        let id = ui.add(list, item)?;
        b.mark(id, Target::TrashPlace);
    }
    Ok(())
}

fn title(s: &State) -> Label {
    match (&s.folder.area, s.folder.name()) {
        (_, Some(name)) => Label::data(name),
        (AreaId::Drive(d), None) => Label::key_with("files.place.drive", &[("name", d)]),
        (area, None) => Label::key(area.label_key()),
    }
}

fn toolbar(
    ui: &mut Ui,
    b: &mut Built,
    main: NodeId,
    s: &State,
    compact: bool,
) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding {
            start: Some(Space::S12),
            end: Some(Space::S12),
            top: None,
            bottom: None,
        })
        .gap(Space::S8)
        .cross(CrossAlign::Center);
    let bar = ui.add_sized(main, flex, Sizing::AUTO.with_height(TOOLBAR))?;
    let back = ui.add(bar, IconButton::new(IconId::Back, Label::key("files.back")))?;
    b.mark(back, Target::Back);
    ui.set_disabled(back, s.history.back.is_empty())?;
    let forward = ui.add(
        bar,
        IconButton::new(IconId::Forward, Label::key("files.forward")),
    )?;
    b.mark(forward, Target::Forward);
    ui.set_disabled(forward, s.history.forward.is_empty())?;
    if compact {
        let places = ui.add(
            bar,
            IconButton::new(IconId::Home, Label::key("files.places")),
        )?;
        b.mark(places, Target::PlacesMenu);
    }
    let heading = TextSpec::new(typography::BODY).weight(Weight::Semibold);
    ui.add(bar, Text::new(title(s)).spec(heading))?;
    let count = crate::update::visible(s).len() as i64;
    let small = TextSpec::new(TypeSize::S12);
    ui.add(
        bar,
        Text::new(Label::key_count("files.count", "count", count))
            .spec(small)
            .color(ColorRole::TextMuted),
    )?;
    tools(ui, b, bar, s, compact)
}

/// The end of the toolbar: search, new folder (or empty trash), view.
fn tools(ui: &mut Ui, b: &mut Built, bar: NodeId, s: &State, compact: bool) -> Result<(), UiError> {
    ui.add_sized(bar, Flex::row(), Sizing::grow(1))?;
    let name = Label::key_with(
        "files.search",
        &[("place", &ui.translator().t(title_key(s)))],
    );
    let field = SearchField::new(name.clone())
        .placeholder(name)
        .text(&s.query);
    let width = if compact {
        Sizing::grow(1)
    } else {
        Sizing::width(SEARCH)
    };
    let search = ui.add_sized(bar, field, width)?;
    b.search = Some(b.mark(search, Target::Search));
    if s.in_trash() {
        let empty = Button::new(Label::key("files.cmd.empty_trash")).variant(ButtonVariant::Danger);
        let id = ui.add(bar, empty)?;
        ui.set_disabled(id, s.items.is_empty() || s.job.is_some())?;
        b.mark(id, Target::EmptyTrash);
    } else {
        let new = ui.add(
            bar,
            IconButton::new(IconId::Plus, Label::key("files.cmd.new_folder")),
        )?;
        ui.set_disabled(new, s.error.is_some())?;
        b.mark(new, Target::NewFolder);
    }
    let view = ui.add(
        bar,
        IconButton::new(IconId::Grid, Label::key("files.view_mode")),
    )?;
    b.mark(view, Target::ViewMode);
    Ok(())
}

fn title_key(s: &State) -> &'static str {
    s.folder.area.label_key()
}

fn job_bar(ui: &mut Ui, b: &mut Built, main: NodeId, fraction: f32) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::all(Space::S12))
        .gap(Space::S12)
        .cross(CrossAlign::Center);
    let row = ui.add_sized(main, Card::bar(BarEdge::Top).flex(flex), Sizing::AUTO)?;
    let progress = ui.add_sized(
        row,
        Progress::new(Label::key("files.job.progress")).value(fraction),
        Sizing::grow(1),
    )?;
    b.progress = Some(progress);
    let cancel = ui.add(row, Button::new(Label::key("files.job.cancel")))?;
    b.mark(cancel, Target::JobCancel);
    Ok(())
}
