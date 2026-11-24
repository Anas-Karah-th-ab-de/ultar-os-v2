//! Board PhoneDial: the dial pad (way, number, suggestion, keys, call buttons) and the call
//! history (filter, settings, entries with call-back and more). Compact: one page at a time.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use super::parts::{heading, mono, notices, small};
use super::{metrics, section, Builder};
use crate::click::{Click, KEYS};
use crate::model::{Filter, Model, Page};
use crate::text::way_short;
use crate::when::when_label;
use ultar_bus::messages::call::{Direction, LogEntry, Way};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::text::Weight;
use ultar_ui::tokens::{size, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Button, ButtonVariant, Card, EmptyState, Icon, IconButton, Label, RadioGroup, RadioLayout,
    Scroll, Tabs, TextDirection, TextField,
};

/// The dial surface.
pub(crate) fn surface(b: &mut Builder<'_>, body: NodeId, model: &Model) -> Result<(), UiError> {
    if b.compact() {
        let col = b.flex(body, Flex::column(), Sizing::grow(1))?;
        let tabs = Tabs::new(
            Label::key("phone.tab.dial"),
            vec![Label::key("phone.tab.dial"), Label::key("phone.tab.log")],
        )
        .selected(usize::from(model.page == Page::Log));
        let tabs = b.click(col, tabs, Sizing::AUTO, Click::Page)?;
        b.built.page = Some(tabs);
        return match model.page {
            Page::Dial => pad_column(b, col, model, Sizing::grow(1)),
            Page::Log => history(b, col, model),
        };
    }
    pad_column(b, body, model, Sizing::width(metrics::dial_width()))?;
    history(b, body, model)
}

fn pad_column(
    b: &mut Builder<'_>,
    parent: NodeId,
    model: &Model,
    sizing: Sizing,
) -> Result<(), UiError> {
    let flex = Flex::column()
        .padding(Padding::all(if b.compact() {
            Space::S16
        } else {
            Space::S24
        }))
        .gap(Space::S12);
    // A scroll container: on a window lower than the pad (a PC's first window, a small
    // screen) the keys and the call button stay reachable instead of being cut off.
    let pad = Scroll::new().flex(flex).name(Label::key("phone.pad"));
    let col = b.add(parent, pad, sizing)?;
    b.built.pad = Some(col);
    b.add(
        col,
        heading(
            Label::key("phone.dial.heading"),
            TypeSize::S24,
            Weight::Bold,
        ),
        Sizing::AUTO,
    )?;
    notices(b, col, model)?;
    ways(b, col, model)?;
    number_row(b, col, model)?;
    if let Some(entry) = model.suggestion() {
        let name = entry.name.as_deref().unwrap_or(&entry.number);
        let s = Button::new(Label::data(name))
            .icon(IconId::User)
            .variant(ButtonVariant::Quiet);
        let id = b.click(
            col,
            s,
            Sizing::grow(0).with_height(metrics::key_height()),
            Click::Suggest,
        )?;
        b.name(id, Label::key_with("phone.suggest", &[("name", name)]))?;
    }
    keys(b, col)?;
    let row = b.flex(col, Flex::row().gap(Space::S8), Sizing::AUTO)?;
    let video = Button::new(Label::key("phone.call.video")).icon(IconId::Video);
    let (video_w, call_w) = call_widths(b.compact());
    let video = b.click(row, video, video_w, Click::CallVideo)?;
    b.ui.set_disabled(video, !model.can_video())?;
    let call = Button::new(Label::key("phone.call"))
        .icon(IconId::Phone)
        .variant(ButtonVariant::Primary);
    let call = b.click(row, call, call_w, Click::Call)?;
    b.ui.set_disabled(call, !model.can_call())?;
    Ok(())
}

/// Widths of the video and call buttons. Wide: the boards' border-box `flex: 1` – equal
/// shares of the column less the edges, then each its own edge (the primary has two
/// borders). Compact: equal growth.
fn call_widths(compact: bool) -> (Sizing, Sizing) {
    if compact {
        return (Sizing::grow(1), Sizing::grow(1));
    }
    let inner = metrics::dial_width() - 2.0 * Space::S24.points();
    let (plain, primary) = (2.0 * size::BORDER, 4.0 * size::BORDER);
    let share = (inner - Space::S8.points() - plain - primary) / 2.0;
    (Sizing::width(share + plain), Sizing::width(share + primary))
}

fn ways(b: &mut Builder<'_>, col: NodeId, model: &Model) -> Result<(), UiError> {
    let cell_detail = match model.state.lines.sims.first() {
        Some(slot) => Label::key_with("phone.way.cell.detail", &[("slot", &slot.to_string())]),
        None => Label::key("phone.way.cell.none"),
    };
    let mut group = RadioGroup::new(
        Label::key("phone.way.group"),
        vec![Label::key("phone.way.ultar"), Label::key("phone.way.cell")],
    )
    .layout(RadioLayout::Choices)
    .detail(0, Label::key("phone.way.ultar.detail"))
    .detail(1, cell_detail)
    .selected(usize::from(model.current_way() != Way::Ultar));
    if !model.cellular_selectable() {
        group = group.unavailable(1, Label::key("phone.line.no_modem"));
    }
    let id = b.click(col, group, Sizing::AUTO, Click::Way)?;
    b.built.way = Some(id);
    Ok(())
}

fn number_row(b: &mut Builder<'_>, col: NodeId, model: &Model) -> Result<(), UiError> {
    let row = b.flex(
        col,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    // The backspace sits inside the field at its end (board); held deletes everything.
    let field = TextField::new(Label::key("phone.number"))
        .direction(TextDirection::Ltr)
        .text(&model.number())
        .end_action(IconId::Backspace, Label::key("phone.backspace"))
        .fill_height();
    let field = b.click(
        row,
        field,
        Sizing::grow(1).with_height(metrics::key_height()),
        Click::Number,
    )?;
    b.built.number = Some(field);
    Ok(())
}

fn keys(b: &mut Builder<'_>, col: NodeId) -> Result<(), UiError> {
    // The pad keeps the telephone order 1 2 3 from the left in both directions.
    let pad = b.flex(col, Flex::column().gap(Space::S8), Sizing::AUTO)?;
    for row_keys in KEYS.chunks(3) {
        let row = b.flex(pad, Flex::row().gap(Space::S8), Sizing::AUTO)?;
        let ordered: Vec<char> = if b.ui.env().direction == ultar_ui::Direction::Rtl {
            row_keys.iter().rev().copied().collect()
        } else {
            row_keys.to_vec()
        };
        for key in ordered {
            let k = Button::new(Label::data(&key.to_string()));
            let sizing = Sizing::grow(1).with_height(metrics::key_height());
            b.click(row, k, sizing, Click::Digit(key))?;
        }
    }
    Ok(())
}

fn history(b: &mut Builder<'_>, parent: NodeId, model: &Model) -> Result<(), UiError> {
    let col = section(b, parent, Space::S12)?;
    let head = b.flex(
        col,
        Flex::row().gap(Space::S16).cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    b.add(
        head,
        heading(Label::key("phone.log.heading"), TypeSize::S24, Weight::Bold),
        Sizing::grow(1),
    )?;
    let tabs = Tabs::new(
        Label::key("phone.filter.name"),
        vec![
            Label::key("phone.filter.all"),
            Label::key("phone.filter.missed"),
        ],
    )
    .pills()
    .selected(usize::from(model.filter == Filter::Missed));
    let tabs = b.click(head, tabs, Sizing::AUTO, Click::Filter)?;
    b.built.filter = Some(tabs);
    let gear = IconButton::new(IconId::Gear, Label::key("phone.settings"));
    b.click(head, gear, Sizing::AUTO, Click::Settings)?;
    let entries = model.entries();
    if entries.is_empty() {
        let key = match model.filter {
            Filter::All => "phone.log.empty",
            Filter::Missed => "phone.log.empty.missed",
        };
        b.add(
            col,
            EmptyState::new(IconId::Phone, Label::key(key)),
            Sizing::grow(1),
        )?;
        return Ok(());
    }
    let card = b.add(col, Card::new().flex(Flex::column()), Sizing::AUTO)?;
    for entry in entries {
        log_row(b, card, model, entry)?;
    }
    Ok(())
}

fn log_row(b: &mut Builder<'_>, card: NodeId, model: &Model, e: &LogEntry) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::symmetric(Space::S16, Space::S8))
        .gap(Space::S12)
        .cross(CrossAlign::Center);
    let row = b.flex(card, flex, Sizing::AUTO)?;
    let (icon, dir) = match (e.direction, e.missed) {
        (_, true) => (IconId::CallMissed, "phone.dir.missed"),
        (Direction::Incoming, false) => (IconId::CallIn, "phone.dir.in"),
        (Direction::Outgoing, false) => (IconId::CallOut, "phone.dir.out"),
    };
    let color = if e.missed {
        ColorRole::Danger
    } else {
        ColorRole::TextMuted
    };
    b.add(row, Icon::new(icon).color(color), Sizing::AUTO)?;
    let texts = b.flex(row, Flex::column(), Sizing::grow(1))?;
    let name = e.name.as_deref().unwrap_or(&e.number);
    let title = if e.name.is_some() {
        super::parts::heading(Label::data(name), TypeSize::S14, Weight::Medium)
    } else {
        mono(Label::data(name), TypeSize::S14)
    };
    b.add(texts, title, Sizing::AUTO)?;
    let tr = way_short(e.way).resolve(b.ui.translator()).into_owned();
    let dir_text = Label::key(dir).resolve(b.ui.translator()).into_owned();
    let sub = Label::key_with("phone.entry.sub", &[("dir", &dir_text), ("way", &tr)]);
    let sub = if e.missed {
        small(sub).color(ColorRole::Danger)
    } else {
        small(sub)
    };
    b.add(texts, sub, Sizing::AUTO)?;
    b.add(
        row,
        small(when_label(
            e.when_s,
            model.wall_s,
            model.utc_offset_s,
            b.ui.translator(),
        )),
        Sizing::AUTO,
    )?;
    let back = IconButton::new(
        IconId::Phone,
        Label::key_with("phone.callback", &[("name", name)]),
    );
    b.click(row, back, Sizing::AUTO, Click::Callback(e.id))?;
    let more = IconButton::new(
        IconId::More,
        Label::key_with("phone.more", &[("name", name)]),
    );
    b.click(row, more, Sizing::AUTO, Click::EntryMore(e.id))?;
    Ok(())
}
