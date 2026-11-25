//! The emergency panel on the desktop lock screen; its state lives in `ultar-lock-core`.

pub use ultar_lock_core::emergency::*;

use super::LockState;
use crate::view::{Target, View};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, Text};

/// Width of the panel (`g(100)`, as the notification cards).
const WIDTH: f32 = 100.0 * Space::S4.points();

/// The open panel: only what the person stored for an emergency, and the call button.
pub(super) fn panel(view: &mut View, middle: NodeId, s: &LockState) -> Result<(), UiError> {
    let flex = Flex::column()
        .gap(Space::S12)
        .padding(Padding::symmetric(Space::S16, Space::S16));
    let card = Card::padded().flex(flex);
    let width = view.ui.size().w.min(WIDTH);
    let card = view.ui.add_sized(middle, card, Sizing::width(width))?;
    let title = TextSpec::new(TypeSize::S16).weight(ultar_ui::text::Weight::Semibold);
    view.ui
        .add(card, Text::new(Label::key("lock.emergency")).spec(title))?;
    if !details(view, card, &s.prefs)? {
        view.ui.add(
            card,
            Text::new(Label::key("lock.emergency.empty")).color(ColorRole::TextMuted),
        )?;
    }
    call_row(view, card, s)
}

/// The stored contact and medical details – data, shown exactly as stored; `false`
/// when the person stored nothing.
fn details(view: &mut View, card: NodeId, p: &super::prefs::LockPrefs) -> Result<bool, UiError> {
    let mut any = false;
    for (key, value) in [
        ("lock.emergency.contact", &p.contact_name),
        ("lock.emergency.phone", &p.contact_phone),
        ("lock.emergency.medical", &p.medical),
    ] {
        if value.trim().is_empty() {
            continue;
        }
        any = true;
        let line = view.ui.add(card, Flex::column().gap(Space::S4))?;
        view.ui.add(
            line,
            Text::new(Label::key(key))
                .spec(TextSpec::new(TypeSize::S12))
                .color(ColorRole::TextMuted),
        )?;
        view.ui.add(line, Text::new(Label::data(value)))?;
    }
    Ok(any)
}

/// The call's status line, the call button and back.
fn call_row(view: &mut View, card: NodeId, s: &LockState) -> Result<(), UiError> {
    let p = &s.prefs;
    let status = match s.emergency.phase {
        CallPhase::Calling => Some(Label::key_with(
            "lock.emergency.calling",
            &[("number", &p.number)],
        )),
        CallPhase::Active => Some(Label::key("lock.emergency.active")),
        CallPhase::Failed => Some(Label::key("lock.emergency.failed")),
        CallPhase::Idle => None,
    };
    if let Some(status) = status {
        view.ui
            .add(card, Text::new(status).spec(TextSpec::new(TypeSize::S12)))?;
    }
    let row = view
        .ui
        .add(card, Flex::row().gap(Space::S12).cross(CrossAlign::Center))?;
    let call = Button::new(Label::key_with(
        "lock.emergency.call",
        &[("number", &p.number)],
    ))
    .variant(ButtonVariant::Primary);
    let call = view.ui.add(row, call)?;
    view.mark(call, Target::EmergencyCall);
    view.ui.set_disabled(call, s.emergency.sos_active())?;
    view.ui
        .set_loading(call, s.emergency.phase == CallPhase::Calling)?;
    let back = Button::new(Label::key("lock.back")).variant(ButtonVariant::Quiet);
    let back = view.ui.add(row, back)?;
    view.mark(back, Target::EmergencyBack);
    let _ = view.ui.focus(call);
    Ok(())
}
