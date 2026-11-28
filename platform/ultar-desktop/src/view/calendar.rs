//! The calendar and notifications sheet (board CalendarSheet): notifications with "do not
//! disturb" and "clear all" at the start, the month with its Hijri month, the day grid and
//! today's appointments at the end.

use super::geometry::{self, Placement, CALENDAR_WIDTH};
use super::{Look, Target, View};
use crate::model::calendar::{self, Month};
use crate::model::Desktop;
use ultar_bus::{Text as BusText, TextArg};
use ultar_i18n::calendar::{Date, WeekStart};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, DayCell, DayGrid, EmptyState, IconButton, Label, List, ListItem, Switch,
    Text,
};

/// Width of the calendar column (`g(64)`).
pub const CALENDAR_COLUMN: f32 = 64.0 * Space::S4.points();

/// What the sheet needs besides the desktop state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Today {
    /// Today's date.
    pub date: Date,
    /// Milliseconds since the epoch (undo deadline of "clear all").
    pub now_ms: u64,
    /// Offset of the local time zone in seconds.
    pub offset_s: i64,
}

/// A notification text as a label: the bus carries the arguments in order; they are given
/// to the catalog as `arg0`, `arg1`, …; key arguments are translated first.
pub fn label_of(text: &BusText, tr: &Translator) -> Label {
    let values: Vec<(String, String)> = text
        .args
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let v = match a {
                TextArg::Verbatim(s) => s.clone(),
                TextArg::Number(n) => n.to_string(),
                TextArg::Key(k) => tr.t(k).into_owned(),
            };
            (format!("arg{i}"), v)
        })
        .collect();
    let args: Vec<(&str, &str)> = values
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    Label::key_with(text.key.clone(), &args)
}

/// Builds the sheet.
pub fn build(
    d: &Desktop,
    today: Today,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    super::card_sheet(
        look,
        screen,
        CALENDAR_WIDTH,
        tr,
        geometry::sheet,
        |view, card| {
            let row = view
                .ui
                .add(card, Flex::row().gap(Space::S24).cross(CrossAlign::Start))?;
            notes_column(view, row, d, today, tr)?;
            calendar_column(view, row, d, today, look, tr)
        },
    )
}

fn heading(label: Label, size: TypeSize) -> Text {
    Text::new(label).spec(TextSpec::new(size).weight(Weight::Semibold))
}

fn small(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
}

fn notes_column(
    view: &mut View,
    row: NodeId,
    d: &Desktop,
    today: Today,
    tr: &Translator,
) -> Result<(), UiError> {
    let column = view
        .ui
        .add_sized(row, Flex::column().gap(Space::S12), Sizing::grow(1))?;
    let head = view.ui.add(
        column,
        Flex::row().gap(Space::S12).cross(CrossAlign::Center),
    )?;
    view.ui.add_sized(
        head,
        heading(Label::key("desk.cal.notes"), TypeSize::S16),
        Sizing::grow(1),
    )?;
    view.ui.add(head, small(Label::key("desk.cal.dnd")))?;
    let dnd = view
        .ui
        .add(head, Switch::new(Label::key("desk.cal.dnd"), d.values.dnd))?;
    view.mark(dnd, Target::Dnd);
    if !d.presence.settings {
        view.ui.set_disabled(dnd, true)?;
        view.ui
            .set_tooltip(dnd, Some(Label::key("desk.reason.no_settings")))?;
    }
    let clear = Button::new(Label::key("desk.cal.clear")).variant(ButtonVariant::Quiet);
    let clear = view.ui.add(head, clear)?;
    view.mark(clear, Target::ClearAll);
    if d.notes.list().is_empty() {
        view.ui.set_disabled(clear, true)?;
        view.ui
            .set_tooltip(clear, Some(Label::key("desk.cal.no_notes")))?;
    }
    if d.notes.can_undo(today.now_ms) {
        let undo = Button::new(Label::key("desk.cal.undo")).variant(ButtonVariant::Quiet);
        let undo = view.ui.add(column, undo)?;
        view.mark(undo, Target::Undo);
    }
    if d.notes.list().is_empty() {
        let empty = EmptyState::new(IconId::Bell, Label::key("desk.cal.no_notes"));
        view.ui.add(column, empty)?;
        return Ok(());
    }
    for note in d.notes.list().iter().take(8) {
        super::note::card(
            view,
            column,
            (d, note),
            (today.now_ms, today.offset_s),
            super::note::Style::InSheet,
            tr,
        )?;
    }
    Ok(())
}

/// The day the calendar sheet shows the appointments of: the selected one, else today.
pub fn shown_day(d: &Desktop, today: Date) -> Date {
    let Some((month, selected)) = d.notes.calendar else {
        return today;
    };
    let start = WeekStart::default_for(d.lang);
    calendar::grid(month, today, start)
        .get(selected)
        .map_or(today, |day| day.date)
}

/// The appointments of the shown day (cal.event): time and title, a click opens it in the
/// Calendar app; without the calendar service (or none that day) "no appointments".
fn events(view: &mut View, column: NodeId, d: &Desktop, today: Date) -> Result<(), UiError> {
    let shown = shown_day(d, today);
    let key = (shown.year() as u16, shown.month(), shown.day());
    let list = match &d.svc.day {
        Some((date, events)) if *date == key && !events.is_empty() => events,
        _ => {
            view.ui
                .add(column, small(Label::key("desk.cal.no_events")))?;
            return Ok(());
        }
    };
    let items = view
        .ui
        .add(column, List::new().name(Label::key("desk.cal.events")))?;
    for e in list {
        let time = format!("{:02}:{:02}", e.start_minute / 60, e.start_minute % 60);
        let item = ListItem::new(Label::data(&e.title))
            .sublabel(Label::data(&time))
            .icon(IconId::Clock);
        let id = view.ui.add(items, item)?;
        view.mark(id, Target::Event(e.id));
    }
    Ok(())
}

fn day_grid(d: &Desktop, today: Today, look: Look, tr: &Translator) -> (DayGrid, Month) {
    let (month, selected) = d
        .notes
        .calendar
        .unwrap_or((Month::of(today.date), usize::MAX));
    let start = WeekStart::default_for(look.lang);
    let days = calendar::grid(month, today.date, start);
    let headers = start
        .days()
        .iter()
        .map(|w| Label::key(format!("desk.cal.head.{}", w.id())))
        .collect();
    let cells = days
        .iter()
        .map(|day| {
            let month_name = tr
                .t(&format!("desk.month.long.{}", day.date.month()))
                .into_owned();
            let number = day.date.day().to_string();
            let key = if day.today {
                "desk.cal.day_today"
            } else {
                "desk.cal.day"
            };
            DayCell {
                number: Label::data(&number),
                name: Label::key_with(key, &[("day", &number), ("month", &month_name)]),
                muted: !day.current,
                today: day.today,
            }
        })
        .collect();
    let mut grid = DayGrid::new(Label::data(&calendar::title(month, tr)), headers, cells);
    if selected < grid.cells.len() {
        grid.selected = selected;
    }
    (grid, month)
}

fn calendar_column(
    view: &mut View,
    row: NodeId,
    d: &Desktop,
    today: Today,
    look: Look,
    tr: &Translator,
) -> Result<(), UiError> {
    let column = view.ui.add_sized(
        row,
        Flex::column().gap(Space::S12),
        Sizing::width(CALENDAR_COLUMN),
    )?;
    let (grid, month) = day_grid(d, today, look, tr);
    let head = view
        .ui
        .add(column, Flex::row().gap(Space::S4).cross(CrossAlign::Center))?;
    let titles = view.ui.add_sized(head, Flex::column(), Sizing::grow(1))?;
    view.ui.add(
        titles,
        heading(Label::data(&calendar::title(month, tr)), TypeSize::S16),
    )?;
    let first = Date::new(month.year, month.month, 15).unwrap_or(today.date);
    view.ui
        .add(titles, small(Label::data(&hijri_month(first, tr))))?;
    let prev = view.ui.add(
        head,
        IconButton::new(IconId::Back, Label::key("desk.cal.prev")),
    )?;
    view.mark(prev, Target::PrevMonth);
    let next = view.ui.add(
        head,
        IconButton::new(IconId::Forward, Label::key("desk.cal.next")),
    )?;
    view.mark(next, Target::NextMonth);
    let grid = view.ui.add(column, grid)?;
    view.mark(grid, Target::Days);
    view.ui.add(
        column,
        heading(Label::key("desk.cal.events"), TypeSize::S14),
    )?;
    events(view, column, d, today.date)
}

/// `جمادى الآخرة 1448` – the Hijri month around the middle of the month shown.
fn hijri_month(date: Date, tr: &Translator) -> String {
    let Ok(h) = date.to_hijri() else {
        return String::new();
    };
    let name = ultar_i18n::datefmt::hijri_month_name(h.month(), tr);
    tr.t_args(
        "desk.cal.hijri_month",
        &[
            ("month", ultar_i18n::Arg::Str(&name)),
            ("year", ultar_i18n::Arg::Int(i64::from(h.year()))),
        ],
    )
}
