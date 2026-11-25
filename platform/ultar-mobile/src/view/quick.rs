//! The quick settings, pulled down from the status bar: time and date with Settings and the
//! power menu, the two large tiles (Wi-Fi, mobile data), the round switches in four columns,
//! brightness and volume, the notifications, and the handle that closes them.

use super::home::small;
use super::{metrics, Builder};
use crate::click::Click;
use crate::model::quick::{Quick, Toggle};
use crate::model::Mobile;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, Card, Icon, IconButton, Label, Scroll, Slider, Text,
};

/// Builds the body of the quick settings.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S8))
        .gap(Space::S16);
    let main = b.add(root, Scroll::new().flex(main), Sizing::grow(1))?;
    header(b, main, m)?;
    let (large, round) = Quick::shown(m.hardware);
    large_tiles(b, main, m, &large)?;
    round_tiles(b, main, m, &round)?;
    sliders(b, main, m)?;
    notes(b, main, m)?;
    let handle = Brick::new(
        BrickKind::Handle {
            width: Space::S48.points(),
        },
        Label::key("mobile.qs.close"),
    );
    b.click(
        root,
        handle,
        Sizing::AUTO.with_height(Space::S24.points()),
        Click::Home,
    )?;
    Ok(())
}

/// Time and date at the start, Settings and the power menu at the end.
fn header(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let row = b.flex(
        parent,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    let col = b.flex(row, Flex::column(), Sizing::grow(1))?;
    if let Some(now) = m.now {
        let spec = TextSpec::title(TypeSize::S32).weight(Weight::Bold);
        b.add(
            col,
            Text::new(Label::data(&now.clock())).spec(spec),
            Sizing::AUTO,
        )?;
        let tr = b.ui.translator().clone();
        b.add(col, small(Label::data(&now.date_line(&tr))), Sizing::AUTO)?;
    }
    let end = b.flex(row, Flex::row().gap(Space::S4), Sizing::AUTO)?;
    let settings = IconButton::new(IconId::Gear, Label::key("mobile.qs.settings"));
    b.click(end, settings, Sizing::AUTO, Click::Settings)?;
    let power = IconButton::new(IconId::Power, Label::key("mobile.qs.power"));
    b.click(end, power, Sizing::AUTO, Click::Power)?;
    Ok(())
}

/// The state line of a large tile.
fn state(m: &Mobile, t: Toggle) -> Label {
    if t == Toggle::Mesh {
        let n = i64::try_from(m.mesh.connected()).unwrap_or(0);
        return match m.mesh.enabled() {
            true => Label::key_count("mobile.qs.mesh.on", "count", n),
            false => Label::key("mobile.qs.off"),
        };
    }
    let on = m.quick.is_on(t);
    match (t, on, &m.quick.network) {
        (Toggle::Wifi, true, Some(net)) => Label::data(net),
        (Toggle::Wifi, true, None) => Label::key("mobile.qs.not_connected"),
        (_, true, _) => Label::key("mobile.qs.on"),
        (_, false, _) => Label::key("mobile.qs.off"),
    }
}

/// Wi-Fi and mobile data side by side, each with its "more" part.
fn large_tiles(
    b: &mut Builder<'_>,
    parent: NodeId,
    m: &Mobile,
    tiles: &[Toggle],
) -> Result<(), UiError> {
    let rows = b.flex(parent, Flex::column().gap(Space::S8), Sizing::AUTO)?;
    for pair in tiles.chunks(2) {
        large_row(b, rows, m, pair)?;
    }
    Ok(())
}

/// Two large tiles side by side (a single one keeps its half).
fn large_row(
    b: &mut Builder<'_>,
    parent: NodeId,
    m: &Mobile,
    tiles: &[Toggle],
) -> Result<(), UiError> {
    let row = b.flex(parent, Flex::row().gap(Space::S8), Sizing::AUTO)?;
    for t in tiles {
        let on = if *t == Toggle::Mesh {
            m.mesh.enabled()
        } else {
            m.quick.is_on(*t)
        };
        let pair = b.flex(row, Flex::row(), Sizing::grow(1))?;
        let tile = Brick::new(
            BrickKind::QuickTile {
                icon: t.icon(),
                state: state(m, *t),
                on,
                joined: true,
            },
            Label::key(t.key()),
        );
        b.click(pair, tile, Sizing::grow(1), Click::Toggle(*t))?;
        let more_key = if *t == Toggle::Mesh {
            "mobile.mesh.continue"
        } else {
            "mobile.qs.more"
        };
        let more = Brick::new(BrickKind::QuickMore { on }, Label::key(more_key));
        let id = b.click(pair, more, Sizing::AUTO, Click::ToggleMore(*t))?;
        if *t == Toggle::Mesh && !(m.mesh.can_send() && !m.recents.is_empty()) {
            b.ui.set_disabled(id, true)?;
        }
    }
    Ok(())
}

/// The round switches, four in a row.
fn round_tiles(
    b: &mut Builder<'_>,
    parent: NodeId,
    m: &Mobile,
    tiles: &[Toggle],
) -> Result<(), UiError> {
    let col = b.flex(parent, Flex::column().gap(Space::S8), Sizing::AUTO)?;
    for chunk in tiles.chunks(metrics::COLUMNS) {
        let row = b.flex(col, Flex::row().gap(Space::S8), Sizing::AUTO)?;
        for t in chunk {
            let button = Button::new(Label::key(t.key()))
                .icon(t.icon())
                .toggled(m.quick.is_on(*t))
                .tile();
            let sizing = Sizing::grow(1).with_height(metrics::grid(19.0));
            b.click(row, button, sizing, Click::Toggle(*t))?;
        }
        for _ in chunk.len()..metrics::COLUMNS {
            b.spacer(row)?;
        }
    }
    Ok(())
}

/// Brightness and volume.
fn sliders(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    for (icon, key, value, slot) in [
        (IconId::Sun, "mobile.qs.brightness", m.quick.brightness, 0),
        (IconId::Volume, "mobile.qs.volume", m.quick.volume, 1),
    ] {
        let row = b.flex(
            parent,
            Flex::row().gap(Space::S12).cross(CrossAlign::Center),
            Sizing::AUTO,
        )?;
        b.add(row, Icon::new(icon), Sizing::AUTO)?;
        let slider = Slider::new(Label::key(key), 0.0, 100.0, 1.0).value(f32::from(value));
        let id = b.add(row, slider, Sizing::grow(1))?;
        if slot == 0 {
            b.built.brightness = Some(id);
        } else {
            b.built.volume = Some(id);
        }
    }
    Ok(())
}

/// The notifications with "clear all", or a quiet line when there are none.
fn notes(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let head = b.flex(
        parent,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center),
        Sizing::AUTO,
    )?;
    let spec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
    b.add(
        head,
        Text::new(Label::key("mobile.qs.notes")).spec(spec),
        Sizing::AUTO,
    )?;
    if m.notes.is_empty() {
        let empty = b.flex(
            parent,
            Flex::column()
                .cross(CrossAlign::Center)
                .gap(Space::S8)
                .padding(Padding::all(Space::S16)),
            Sizing::AUTO,
        )?;
        b.add(empty, Icon::new(IconId::Bell), Sizing::AUTO)?;
        b.add(empty, small(Label::key("mobile.qs.no_notes")), Sizing::AUTO)?;
        return Ok(());
    }
    let clear = Button::new(Label::key("mobile.qs.clear")).variant(ButtonVariant::Quiet);
    b.click(head, clear, Sizing::AUTO, Click::ClearAll)?;
    for n in &m.notes {
        let flex = Flex::row()
            .gap(Space::S12)
            .cross(CrossAlign::Start)
            .padding(Padding::all(Space::S12));
        let card = b.add(parent, Card::new().flex(flex), Sizing::AUTO)?;
        let icon = m.app(&n.app).map_or(IconId::Bell, |a| a.icon);
        b.add(card, Icon::new(icon), Sizing::AUTO)?;
        let col = b.flex(card, Flex::column().gap(Space::S4), Sizing::grow(1))?;
        let top = b.flex(col, Flex::row().main(MainAlign::SpaceBetween), Sizing::AUTO)?;
        let app = match m.app(&n.app) {
            Some(a) => super::app_label(a),
            None => Label::data(&n.app),
        };
        b.add(top, small(app), Sizing::AUTO)?;
        b.add(top, small(Label::data(&n.time)), Sizing::AUTO)?;
        let spec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
        b.add(
            col,
            Text::new(Label::data(&n.title)).spec(spec),
            Sizing::AUTO,
        )?;
        let body = Text::new(Label::data(&n.body))
            .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap));
        b.add(col, body, Sizing::AUTO)?;
        let close = IconButton::new(IconId::Close, Label::key("mobile.qs.dismiss"));
        b.click(card, close, Sizing::AUTO, Click::Dismiss(n.id))?;
    }
    Ok(())
}

/// The power menu over the quick settings: restart, power off, lock now, cancel.
pub fn power_menu(b: &mut Builder<'_>, root: NodeId) -> Result<(), UiError> {
    let column = Flex::column().gap(Space::S8);
    let dialog = ultar_ui::widgets::Dialog::new(Label::key("mobile.power.title")).flex(column);
    let d = b.add(root, dialog, Sizing::AUTO)?;
    for (icon, key, click) in [
        (IconId::Reload, "mobile.power.restart", Click::PowerRestart),
        (IconId::Power, "mobile.power.off", Click::PowerOff),
        (IconId::Lock, "mobile.power.lock", Click::LockNow),
    ] {
        let item = Button::new(Label::key(key)).icon(icon);
        b.click(d, item, Sizing::grow(1), click)?;
    }
    let cancel = Button::new(Label::key("mobile.power.cancel")).variant(ButtonVariant::Quiet);
    let cancel = b.click(d, cancel, Sizing::AUTO, Click::PowerCancel)?;
    b.ui.set_cancel_action(d, Some(cancel))?;
    b.ui.open_dialog(d)?;
    Ok(())
}
