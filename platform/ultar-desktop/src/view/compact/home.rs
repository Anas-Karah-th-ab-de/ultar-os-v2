//! The phone's home screen (board CompactHome): the clock with the date and the Hijri
//! date, the search entry, the apps of the home screen in four columns, and the favourites
//! row with "all apps". It is the desktop background of the phone: apps open above it and
//! the gesture bar brings it back.

use super::{app_cell, cell_rows, GESTURE_HEIGHT, STATUS_HEIGHT};
use crate::model::apps;
use crate::model::time::{self, LocalTime};
use crate::model::Desktop;
use crate::view::{new_ui, Look, Target, View};
use ultar_i18n::datefmt::ClockStyle;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{FontRole, TextAlign, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Brick, BrickKind, Card, Label, Text};

/// Favourites shown before "all apps" (board: three).
pub const MAX_FAVOURITES: usize = 3;
/// Rows of apps on the home screen (the rest is under "all apps").
pub const MAX_ROWS: usize = 3;

/// The clock of the home and the lock screen: the time, the date line and the Hijri date.
pub(crate) fn clock_block(
    view: &mut View,
    parent: NodeId,
    lines: (&str, &str, &str),
    top: Space,
) -> Result<(), UiError> {
    let flex = Flex::column().cross(CrossAlign::Center).padding(Padding {
        start: None,
        end: None,
        top: Some(top),
        bottom: None,
    });
    let clock = view.ui.add(parent, flex)?;
    let big = TextSpec {
        role: FontRole::Title,
        ..TextSpec::new(TypeSize::S32).weight(Weight::Bold)
    };
    let time = Text::new(Label::data(lines.0)).spec(big);
    view.ui.add(clock, time.align(TextAlign::Center))?;
    let date = Text::new(Label::data(lines.1)).spec(TextSpec::new(TypeSize::S16));
    view.ui.add(clock, date)?;
    let hijri = Text::new(Label::data(lines.2)).spec(TextSpec::new(TypeSize::S12));
    view.ui.add(clock, hijri.color(ColorRole::TextMuted))?;
    Ok(())
}

/// Builds the home screen for the whole `screen` (the bars lie over its top and bottom).
pub fn build(
    d: &Desktop,
    now: Option<LocalTime>,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<View, UiError> {
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_background(ColorRole::Ground);
    ui.set_root_flex(Flex::column());
    let mut view = View::new(ui);
    let root = view.ui.root();
    view.ui
        .add_sized(root, Flex::row(), Sizing::AUTO.with_height(STATUS_HEIGHT))?;
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S12))
        .gap(Space::S16);
    let main = view.ui.add_sized(root, main, Sizing::grow(1))?;
    view.ui
        .add_sized(root, Flex::row(), Sizing::AUTO.with_height(GESTURE_HEIGHT))?;
    if let Some(t) = now {
        let (clock, date) = time::bar_lines(t, ClockStyle::H24, tr);
        let hijri = time::hijri_line(t.date, tr);
        clock_block(&mut view, main, (&clock, &date, &hijri), Space::S24)?;
    }
    let width = (screen.w - 2.0 * Space::S16.points()).max(0.0);
    let search = Brick::new(
        BrickKind::SearchPill { width },
        Label::key("desk.mobile.search"),
    );
    let id = view.ui.add(main, search)?;
    view.mark(id, Target::Search);
    let grid = view.ui.add(main, Flex::column().gap(Space::S8))?;
    let mut on_home = apps::listed(&d.home.apps, &d.catalog);
    on_home.truncate(MAX_ROWS * super::COLUMNS);
    cell_rows(&mut view, grid, &on_home, |v, id, app| {
        app_cell(v, id, d, app)
    })?;
    view.ui.add_sized(main, Flex::column(), Sizing::grow(1))?;
    let hint = Text::new(Label::key("desk.mobile.swipe_hint"))
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
        .align(TextAlign::Center);
    view.ui.add(main, hint)?;
    favourites(&mut view, main, d)?;
    Ok(view)
}

/// The favourites row: the first pinned apps and "all apps" at the end (CLICKS
/// `mobile.app`, `mobile.apps`).
fn favourites(view: &mut View, main: NodeId, d: &Desktop) -> Result<(), UiError> {
    let flex = Flex::row().padding(Padding::all(Space::S8)).gap(Space::S8);
    let card = view.ui.add(main, Card::new().flex(flex))?;
    // A free place of the row takes a dragged app.
    view.mark(card, Target::Favourites);
    view.ui.set_drop_target(card, true)?;
    let mut shown = apps::listed(&d.pinned, &d.catalog);
    shown.truncate(MAX_FAVOURITES);
    for app in &shown {
        let cell = Brick::new(
            BrickKind::PhoneCell {
                icon: app.icon,
                open: false,
            },
            app.label(),
        );
        let id = view.ui.add_sized(card, cell, Sizing::grow(1))?;
        app_cell(view, id, d, app)?;
    }
    for _ in shown.len()..MAX_FAVOURITES {
        view.ui.add_sized(card, Flex::row(), Sizing::grow(1))?;
    }
    let all = Brick::new(
        BrickKind::PhoneCell {
            icon: IconId::Grid,
            open: matches!(d.open, crate::model::Open::Apps(_)),
        },
        Label::key("desk.bar.apps"),
    );
    let id = view.ui.add_sized(card, all, Sizing::grow(1))?;
    view.mark(id, Target::AllApps);
    Ok(())
}
