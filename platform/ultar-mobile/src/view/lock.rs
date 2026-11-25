//! The lock screen's cover: the large dot-matrix clock with the date and the Hijri date, the
//! waiting notifications (only app and count – the content stays hidden while locked),
//! torch · unlock · camera, and the emergency call. Fed by the shell model ([`body`]) and by
//! the phone's lock program ([`cover`]).

use super::home::small;
use super::{clock, Builder};
use crate::click::Click;
use crate::model::apps::App;
use crate::model::quick::Toggle;
use crate::model::time::LocalTime;
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

/// What the cover shows.
#[derive(Debug, Clone, Default)]
pub struct Cover<'a> {
    /// Now.
    pub now: Option<LocalTime>,
    /// Waiting notifications per app identifier.
    pub counts: Vec<(String, usize)>,
    /// The torch: `None` without a camera light, else whether it is on.
    pub torch: Option<bool>,
    /// The installed apps (names and icons of the notification cards).
    pub apps: &'a [App],
    /// Offer the camera (a camera app that works while locked).
    pub camera: bool,
    /// Offer the emergency call (a phone that can call while locked).
    pub emergency: bool,
}

/// Builds the cover from the shell's model.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for n in &m.notes {
        *counts.entry(n.app.clone()).or_default() += 1;
    }
    let c = Cover {
        now: m.now,
        counts: counts.into_iter().collect(),
        torch: m.hardware.torch.then(|| m.quick.is_on(Toggle::Torch)),
        apps: &m.launcher.apps,
        camera: true,
        emergency: m.hardware.modem,
    };
    cover(b, root, &c)
}

/// Builds the cover.
pub fn cover(b: &mut Builder<'_>, root: NodeId, c: &Cover<'_>) -> Result<(), UiError> {
    let main = Flex::column()
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S16, Space::S24))
        .gap(Space::S8);
    let main = b.flex(root, main, Sizing::grow(1))?;
    if let Some(now) = c.now {
        let tr = b.ui.translator().clone();
        let date = Text::new(Label::data(&now.date_line(&tr))).spec(TextSpec::new(TypeSize::S16));
        b.add(main, date, Sizing::AUTO)?;
        let time = now.clock();
        let (area, width) = clock::area(&time, CLOCK_HEIGHT);
        let id = b.add(main, area, Sizing::width(width).with_height(CLOCK_HEIGHT))?;
        b.built.clocks.push((id, time));
        b.add(main, small(Label::data(&now.hijri_line(&tr))), Sizing::AUTO)?;
    }
    notes(b, main, c)?;
    b.spacer(main)?;
    actions(b, main, c)?;
    if c.emergency {
        let emergency = Button::new(Label::key("mobile.lock.emergency"))
            .variant(ButtonVariant::Quiet)
            .icon(IconId::Sos);
        b.click(main, emergency, Sizing::AUTO, Click::Emergency)?;
    }
    Ok(())
}

/// One card per app with waiting notifications: the app, the count, "content hidden".
fn notes(b: &mut Builder<'_>, parent: NodeId, c: &Cover<'_>) -> Result<(), UiError> {
    let list = b.flex(
        parent,
        Flex::column()
            .gap(Space::S8)
            .padding(Padding::symmetric(Space::S4, Space::S16)),
        Sizing::grow(0),
    )?;
    for (app_id, count) in &c.counts {
        let (icon, name) = match c.apps.iter().find(|a| &a.id == app_id) {
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
        let n = i64::try_from(*count).unwrap_or(i64::MAX);
        let hidden = Label::key_count("mobile.lock.hidden", "count", n);
        b.add(col, small(hidden), Sizing::AUTO)?;
    }
    Ok(())
}

/// Torch at the start, unlock in the middle, camera at the end; the swipe hint below.
fn actions(b: &mut Builder<'_>, parent: NodeId, c: &Cover<'_>) -> Result<(), UiError> {
    let width = b.screen().w - 2.0 * Space::S16.points();
    let row = b.flex(
        parent,
        Flex::row()
            .main(MainAlign::SpaceBetween)
            .cross(CrossAlign::Center),
        Sizing::width(width),
    )?;
    match c.torch {
        Some(on) => {
            let torch = Button::new(Label::key("mobile.qs.torch"))
                .icon(IconId::Sun)
                .toggled(on)
                .tile();
            b.click(row, torch, Sizing::AUTO, Click::Torch)?;
        }
        None => {
            b.spacer(row)?;
        }
    }
    let unlock = Button::new(Label::key("mobile.lock.unlock"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Lock);
    b.click(row, unlock, Sizing::AUTO, Click::Unlock)?;
    if c.camera {
        let camera = IconButton::new(IconId::Video, Label::key("mobile.lock.camera"));
        b.click(row, camera, Sizing::AUTO, Click::Camera)?;
    } else {
        b.spacer(row)?;
    }
    let hint = small(Label::key("mobile.lock.swipe")).color(ColorRole::TextMuted);
    b.add(parent, hint, Sizing::AUTO)?;
    Ok(())
}
