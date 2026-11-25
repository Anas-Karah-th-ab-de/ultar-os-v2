//! The lock screen: the large dot-matrix clock with the date and the Hijri date, the waiting
//! notifications (only app and count – the content stays hidden while locked), torch ·
//! unlock · camera, and the emergency call.

use super::home::small;
use super::{bars, clock, Builder};
use crate::click::Click;
use crate::model::Mobile;
use std::collections::BTreeMap;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Icon, IconButton, Label, Text};

/// Height of the lock screen's clock (points).
pub const CLOCK_HEIGHT: f32 = 64.0;

/// Builds the lock screen.
pub fn build(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    bars::status(b, root, m, false)?;
    let main = Flex::column()
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S16, Space::S24))
        .gap(Space::S8);
    let main = b.flex(root, main, Sizing::grow(1))?;
    if let Some(now) = m.now {
        let tr = b.ui.translator().clone();
        let date = Text::new(Label::data(&now.date_line(&tr))).spec(TextSpec::new(TypeSize::S16));
        b.add(main, date, Sizing::AUTO)?;
        let time = now.clock();
        let (area, width) = clock::area(&time, CLOCK_HEIGHT);
        let id = b.add(main, area, Sizing::width(width).with_height(CLOCK_HEIGHT))?;
        b.built.clocks.push((id, time));
        b.add(main, small(Label::data(&now.hijri_line(&tr))), Sizing::AUTO)?;
    }
    notes(b, main, m)?;
    b.spacer(main)?;
    actions(b, main, m)?;
    let emergency = Button::new(Label::key("mobile.lock.emergency"))
        .variant(ButtonVariant::Quiet)
        .icon(IconId::Sos);
    b.click(main, emergency, Sizing::AUTO, Click::Emergency)?;
    Ok(())
}

/// One card per app with waiting notifications: the app, the count, "content hidden".
fn notes(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let mut by_app: BTreeMap<&str, usize> = BTreeMap::new();
    for n in &m.notes {
        *by_app.entry(n.app.as_str()).or_default() += 1;
    }
    let list = b.flex(
        parent,
        Flex::column()
            .gap(Space::S8)
            .padding(Padding::symmetric(Space::S4, Space::S16)),
        Sizing::grow(0),
    )?;
    for (app_id, count) in by_app {
        let (icon, name) = match m.app(app_id) {
            Some(app) => (app.icon, super::app_label(app)),
            None => (IconId::Bell, Label::data(app_id)),
        };
        let flex = Flex::row()
            .gap(Space::S12)
            .cross(CrossAlign::Center)
            .padding(Padding::all(Space::S12));
        let card = b.add(list, Card::new().flex(flex), Sizing::AUTO)?;
        b.add(card, Icon::new(icon), Sizing::AUTO)?;
        let col = b.flex(card, Flex::column(), Sizing::grow(1))?;
        let spec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
        b.add(col, Text::new(name).spec(spec), Sizing::AUTO)?;
        let n = i64::try_from(count).unwrap_or(i64::MAX);
        b.add(
            col,
            small(Label::key_count("mobile.lock.hidden", "count", n)),
            Sizing::AUTO,
        )?;
    }
    Ok(())
}

/// Torch at the start, unlock in the middle, camera at the end; the swipe hint below.
fn actions(b: &mut Builder<'_>, parent: NodeId, m: &Mobile) -> Result<(), UiError> {
    let width = b.screen().w - 2.0 * Space::S16.points();
    let row = b.flex(
        parent,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center),
        Sizing::width(width),
    )?;
    let on = m.quick.is_on(crate::model::quick::Toggle::Torch);
    if m.hardware.torch {
        let torch = Button::new(Label::key("mobile.qs.torch"))
            .icon(IconId::Sun)
            .toggled(on)
            .tile();
        b.click(row, torch, Sizing::AUTO, Click::Torch)?;
    } else {
        b.spacer(row)?;
    }
    let unlock = Button::new(Label::key("mobile.lock.unlock"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Lock);
    b.click(row, unlock, Sizing::AUTO, Click::Unlock)?;
    let camera = IconButton::new(IconId::Video, Label::key("mobile.lock.camera"));
    b.click(row, camera, Sizing::AUTO, Click::Camera)?;
    let hint = small(Label::key("mobile.lock.swipe")).color(ColorRole::TextMuted);
    b.add(parent, hint, Sizing::AUTO)?;
    Ok(())
}
