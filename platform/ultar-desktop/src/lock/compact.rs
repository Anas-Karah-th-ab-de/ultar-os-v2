//! The lock screen's cover on a phone (board CompactLock): the clock with the date and the
//! Hijri date, per app how many notifications wait (never their content), "Unlock" with
//! the swipe hint, and the emergency actions at the bottom.

use super::LockState;
use crate::model::time::{self, LocalTime};
use crate::view::compact::{home::clock_block, STATUS_HEIGHT};
use crate::view::{Target, View};
use ultar_i18n::datefmt::ClockStyle;
use ultar_i18n::Translator;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Icon, Label, Text};

/// Apps whose waiting notifications are counted on the cover.
const MAX_NOTE_CARDS: usize = 4;

/// Fills the root of the lock screen's tree with the phone's cover.
pub(super) fn cover(
    view: &mut View,
    s: &LockState,
    t: Option<LocalTime>,
    tr: &Translator,
) -> Result<(), UiError> {
    view.ui.set_root_flex(Flex::column());
    let root = view.ui.root();
    let status = Flex::row()
        .main(MainAlign::End)
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S12, Space::S8));
    let status = view
        .ui
        .add_sized(root, status, Sizing::AUTO.with_height(STATUS_HEIGHT))?;
    let offline = Icon::new(IconId::Offline).small();
    view.ui.add(status, offline.color(ColorRole::TextMuted))?;
    let main = Flex::column()
        .padding(Padding::symmetric(Space::S16, Space::S12))
        .gap(Space::S16);
    let main = view.ui.add_sized(root, main, Sizing::grow(1))?;
    if let Some(t) = t {
        let clock = ultar_i18n::datefmt::format_time(t.hour, t.minute, ClockStyle::H24, tr);
        let (date, hijri) = time::lock_lines(t, tr);
        clock_block(view, main, (&clock, &date, &hijri), Space::S32)?;
    }
    notes(view, main, s)?;
    super::view::media_card(view, main, s)?;
    view.ui.add_sized(main, Flex::column(), Sizing::grow(1))?;
    let middle = Flex::column().gap(Space::S8).cross(CrossAlign::Center);
    let middle = view.ui.add(main, middle)?;
    let unlock = Button::new(Label::key("lock.unlock"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Lock);
    let id = view.ui.add(middle, unlock)?;
    view.mark(id, Target::Unlock);
    let _ = view.ui.focus(id);
    let hint = Text::new(Label::key("lock.swipe_hint")).spec(TextSpec::new(TypeSize::S12));
    view.ui.add(middle, hint.color(ColorRole::TextMuted))?;
    emergency(view, main, s)
}

/// Per app how many notifications wait – never their content (DESKTOP-SPEC §5.3).
fn notes(view: &mut View, main: NodeId, s: &LockState) -> Result<(), UiError> {
    let list = view.ui.add(main, Flex::column().gap(Space::S8))?;
    for (app, count) in s.notes.iter().take(MAX_NOTE_CARDS) {
        let flex = Flex::row()
            .gap(Space::S12)
            .cross(CrossAlign::Center)
            .padding(Padding::all(Space::S12));
        let card = view.ui.add(list, Card::new().flex(flex))?;
        let known = crate::model::apps::base_apps()
            .into_iter()
            .chain(["org.ultar.phone", "org.ultar.assistant"].map(crate::model::apps::installed))
            .find(|a| &a.id == app);
        let icon = known.as_ref().map_or(IconId::Bell, |a| a.icon);
        view.ui
            .add(card, Icon::new(icon).color(ColorRole::TextMuted))?;
        let texts = view.ui.add_sized(card, Flex::column(), Sizing::grow(1))?;
        let name = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);
        let label = known.map_or_else(|| Label::data(app), |a| a.label());
        view.ui.add(texts, Text::new(label).spec(name))?;
        let hidden = Label::key_with("lock.notes_hidden", &[("count", &count.to_string())]);
        let hidden = Text::new(hidden).spec(TextSpec::new(TypeSize::S12));
        view.ui.add(texts, hidden.color(ColorRole::TextMuted))?;
    }
    Ok(())
}

/// Emergency (no telephony service yet: disabled with the reason) and emergency power-off
/// (the power service).
fn emergency(view: &mut View, main: NodeId, s: &LockState) -> Result<(), UiError> {
    let row = Flex::row().gap(Space::S8).main(MainAlign::SpaceBetween);
    let row = view.ui.add(main, row)?;
    let items = [
        (
            "lock.emergency",
            IconId::Sos,
            Target::Emergency,
            "lock.reason.no_emergency",
        ),
        (
            "lock.power_off",
            IconId::Power,
            Target::EmergencyOff,
            "lock.reason.no_power",
        ),
    ];
    for (key, icon, target, reason) in items {
        let id = view.ui.add(row, Button::new(Label::key(key)).icon(icon))?;
        let power = target == Target::EmergencyOff;
        view.mark(id, target);
        if power {
            super::view::power_gate(view, id, s)?;
        } else {
            view.ui.set_disabled(id, true)?;
            view.ui.set_tooltip(id, Some(Label::key(reason)))?;
        }
    }
    Ok(())
}
