//! The phone's quick settings (board CompactQuick): a sheet from the top edge with the
//! time and date, battery, Settings and power; the tiles in two columns; brightness and
//! volume with their values; the notifications; and the handle that closes it. The rest of
//! the screen is dimmed and closes the sheet too (CLICKS `sheet.close`).

use super::{row_of, GESTURE_HEIGHT, SHEET_HANDLE};
use crate::model::notes::Note;
use crate::model::quick;
use crate::model::{Desktop, Open, QuickPage};
use crate::view::note::{self, Style};
use crate::view::quick::add_tile;
use crate::view::quick::{output_reason, volume_reason};
use crate::view::{new_ui, Look, Target, View};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{FontRole, Weight};
use ultar_ui::tokens::{size, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, Card, Icon, IconButton, Label, Slider, Text,
};

/// Notifications shown in the sheet (newest first; the rest after clearing these).
pub const MAX_NOTES: usize = 3;

/// The lines of the sheet's head and the moment for the notifications' times.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    /// The time.
    pub time: String,
    /// The date line.
    pub date: String,
    /// Now (ms) and the zone offset (s).
    pub now: (u64, i64),
}

/// Builds the sheet over the screen above the gesture bar; returns the view and its size.
pub fn build(
    d: &Desktop,
    head: &Head,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Size), UiError> {
    let area = Size::new(screen.w, (screen.h - GESTURE_HEIGHT).max(0.0));
    let mut ui = new_ui(look, area, tr)?;
    ui.set_transparent_background();
    ui.set_root_flex(Flex::column());
    let flex = Flex::column()
        .padding(Padding {
            start: Some(Space::S16),
            end: Some(Space::S16),
            top: Some(Space::S16),
            bottom: None,
        })
        .gap(Space::S12);
    let root = ui.root();
    let sheet = ui.add(root, Card::new().flex(flex).top_sheet())?;
    let mut view = View::new(ui);
    header(&mut view, sheet, d, head)?;
    // A list part (qs.wifi.list, qs.bluetooth.list …) takes the place of the tiles,
    // sliders and notifications, in the same sheet (compact rows: full sheet width).
    match d.open {
        Open::Quick(QuickPage::Wifi) => crate::view::wifi::page(&mut view, sheet, d)?,
        Open::Quick(QuickPage::WifiJoin) => crate::view::wifi::join(&mut view, sheet, d)?,
        Open::Quick(QuickPage::Vpn) => crate::view::wifi::vpn_page(&mut view, sheet, d)?,
        Open::Quick(QuickPage::Bluetooth) => crate::view::bluetooth::page(&mut view, sheet, d)?,
        _ => tiles(&mut view, sheet, d, head.now, tr)?,
    }
    let handle = Brick::new(
        BrickKind::Handle {
            width: SHEET_HANDLE,
        },
        Label::key("desk.mobile.close_quick"),
    );
    let handle = view
        .ui
        .add_sized(sheet, handle, row_of(size::TARGET_POINTER_MIN))?;
    view.mark(handle, Target::SheetClose);
    view.ui.set_draggable(handle, true)?;
    let dim = Brick::new(BrickKind::Scrim, Label::key("desk.mobile.close_quick"));
    let dim = view.ui.add_sized(root, dim, Sizing::grow(1))?;
    view.mark(dim, Target::SheetClose);
    Ok((view, area))
}

/// The tiles in two columns, the sliders and the notifications (the sheet's first page).
fn tiles(
    view: &mut View,
    sheet: NodeId,
    d: &Desktop,
    now: (u64, i64),
    tr: &Translator,
) -> Result<(), UiError> {
    let grid = view.ui.add(sheet, Flex::column().gap(Space::S8))?;
    let tiles = quick::compact_tiles(&d.presence, &d.values, &d.svc);
    for pair in tiles.chunks(2) {
        let row = view.ui.add(grid, Flex::row().gap(Space::S8))?;
        for tile in pair {
            add_tile(view, row, tile, &d.svc)?;
        }
        if pair.len() == 1 {
            view.ui.add_sized(row, Flex::row(), Sizing::grow(1))?;
        }
    }
    sliders(view, sheet, d)?;
    notes(view, sheet, d, now, tr)
}

fn header(view: &mut View, sheet: NodeId, d: &Desktop, head: &Head) -> Result<(), UiError> {
    let row = view
        .ui
        .add(sheet, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let lines = view.ui.add_sized(row, Flex::column(), Sizing::grow(1))?;
    let big = TextSpec {
        role: FontRole::Title,
        ..TextSpec::new(TypeSize::S20).weight(Weight::Bold)
    };
    view.ui
        .add(lines, Text::new(Label::data(&head.time)).spec(big))?;
    let date = Text::new(Label::data(&head.date)).spec(TextSpec::new(TypeSize::S12));
    view.ui.add(lines, date.color(ColorRole::TextMuted))?;
    if let Some((percent, _)) = d.values.battery {
        let value = percent.to_string();
        let button = Button::new(Label::key_with("desk.percent", &[("value", &value)]))
            .variant(ButtonVariant::Quiet)
            .icon(IconId::Battery);
        let id = view.ui.add(row, button)?;
        view.mark(id, Target::Battery);
        let name = Label::key_with("desk.qs.battery", &[("percent", &value)]);
        view.ui.set_tooltip(id, Some(name))?;
        if !d.presence.launcher {
            view.ui.set_disabled(id, true)?;
        }
    }
    let gear = IconButton::new(IconId::Gear, Label::key("desk.qs.settings"));
    let gear = view.ui.add(row, gear)?;
    view.mark(gear, Target::Settings);
    if !d.presence.launcher {
        view.ui.set_disabled(gear, true)?;
        view.ui
            .set_tooltip(gear, Some(Label::key("desk.reason.no_launcher")))?;
    }
    let power = IconButton::new(IconId::Power, Label::key("desk.qs.power"));
    let power = view.ui.add(row, power)?;
    view.mark(power, Target::Power);
    Ok(())
}

/// One slider line: the icon, the slider and its value in percent (`None`: unknown).
fn slider_line(
    view: &mut View,
    parent: NodeId,
    icon: IconId,
    (name, value): (&str, Option<u8>),
) -> Result<NodeId, UiError> {
    let flex = Flex::row().gap(Space::S12).cross(CrossAlign::Center);
    let row = view.ui.add_sized(parent, flex, Sizing::grow(1))?;
    view.ui
        .add(row, Icon::new(icon).color(ColorRole::TextMuted))?;
    // The slider shows its value at its end (board: 48 wide, muted).
    let slider =
        Slider::new(Label::key(name), 0.0, 100.0, 1.0).value(f32::from(value.unwrap_or(0)));
    view.ui.add_sized(row, slider, Sizing::grow(1))
}

fn sliders(view: &mut View, sheet: NodeId, d: &Desktop) -> Result<(), UiError> {
    let line = view.ui.add(sheet, Flex::row())?;
    let level = d.values.brightness;
    let id = slider_line(view, line, IconId::Sun, ("desk.qs.brightness", level))?;
    view.mark(id, Target::Brightness);
    if level.is_none() || !d.presence.settings {
        view.ui.set_disabled(id, true)?;
        view.ui
            .set_tooltip(id, Some(Label::key("desk.reason.no_display_setting")))?;
    }
    let line = view
        .ui
        .add(sheet, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let volume = d.svc.audio.as_ref().map(|a| a.volume_percent.min(100));
    let id = slider_line(view, line, IconId::Volume, ("desk.qs.volume", volume))?;
    view.mark(id, Target::Volume);
    let out = IconButton::new(IconId::Speaker, Label::key("desk.qs.output"));
    let out = view.ui.add(line, out)?;
    view.mark(out, Target::Output);
    for (node, reason) in [(id, volume_reason(d)), (out, output_reason(d))] {
        if let Some(reason) = reason {
            view.ui.set_disabled(node, true)?;
            view.ui.set_tooltip(node, Some(Label::key(reason)))?;
        }
    }
    Ok(())
}

fn notes(
    view: &mut View,
    sheet: NodeId,
    d: &Desktop,
    now: (u64, i64),
    tr: &Translator,
) -> Result<(), UiError> {
    let head = view
        .ui
        .add(sheet, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let title = Text::new(Label::key("desk.cal.notes"))
        .spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(head, title)?;
    view.ui.add_sized(head, Flex::row(), Sizing::grow(1))?;
    let list: Vec<&Note> = d.notes.list().iter().rev().take(MAX_NOTES).collect();
    if d.notes.can_undo(now.0) {
        let undo = Button::new(Label::key("desk.cal.undo")).variant(ButtonVariant::Quiet);
        let undo = view.ui.add(head, undo)?;
        view.mark(undo, Target::Undo);
    } else {
        let clear = Button::new(Label::key("desk.cal.clear")).variant(ButtonVariant::Quiet);
        let clear = view.ui.add(head, clear)?;
        view.mark(clear, Target::ClearAll);
        if list.is_empty() {
            view.ui.set_disabled(clear, true)?;
            view.ui
                .set_tooltip(clear, Some(Label::key("desk.cal.no_notes")))?;
        }
    }
    if list.is_empty() {
        let none = Text::new(Label::key("desk.cal.no_notes"))
            .spec(TextSpec::new(TypeSize::S12))
            .color(ColorRole::TextMuted);
        view.ui.add(sheet, none)?;
        return Ok(());
    }
    let column = view.ui.add(sheet, Flex::column().gap(Space::S8))?;
    for n in list {
        note::card(view, column, (d, n), now, Style::Compact, tr)?;
    }
    Ok(())
}
