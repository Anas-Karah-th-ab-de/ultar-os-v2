//! One notification as a card (boards Toasts and CalendarSheet): the app's icon, its name
//! and the time, "important" for urgent ones, the title, the body and the close button.
//! The body is not clickable (CLICKS `note.dismiss`).

use super::calendar::label_of;
use super::{Target, View};
use crate::model::notes::Note;
use crate::model::Desktop;
use ultar_bus::messages::notify::MAX_ACTIONS;
use ultar_bus::messages::notify::{Body, Urgency};
use ultar_i18n::Translator;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::Weight;
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Badge, Button, ButtonVariant, Card, CardTone, Icon, IconButton, Label, Text, Tone,
};

/// Senders of the system that are not apps: id, name, icon.
const SYSTEM_SENDERS: [(&str, &str, IconId); 3] = [
    ("org.ultar.power", "desk.note.app.power", IconId::Battery),
    ("org.ultar.shell", "brand.product", IconId::Mark),
    ("org.ultar.update", "desk.note.app.update", IconId::Download),
];

/// Minutes below which the time reads "now".
const NOW_MS: u64 = 60_000;

/// How the card sits: inside the calendar sheet, or floating as a banner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A raised card in the sheet.
    InSheet,
    /// A floating banner (with the shadow).
    Banner,
    /// A raised card in the phone's quick settings (inner spacing 12).
    Compact,
}

fn small(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12))
        .color(ColorRole::TextMuted)
}

/// "now" or the local time of the notification.
fn when(note: &Note, (now_ms, offset_s): (u64, i64)) -> Label {
    if now_ms.saturating_sub(note.at_ms) < NOW_MS {
        return Label::key("desk.note.now");
    }
    let local = note.at_ms as i64 / 1000 + offset_s;
    let minutes = local.rem_euclid(86_400) / 60;
    Label::data(&format!("{:02}:{:02}", minutes / 60, minutes % 60))
}

/// Icon and name of the sender: an app of the catalog, a part of the system, or its id.
fn sender(d: &Desktop, app_id: &str) -> (IconId, Label) {
    if let Some(a) = d.catalog.iter().find(|a| a.id == app_id) {
        return (a.icon, a.label());
    }
    match SYSTEM_SENDERS.iter().find(|(id, ..)| *id == app_id) {
        Some((_, key, icon)) => (*icon, Label::key(*key)),
        None => (IconId::Bell, Label::data(app_id)),
    }
}

/// Adds the card of `note` to `parent`; `now` is the time and the zone offset.
pub fn card(
    view: &mut View,
    parent: NodeId,
    (d, note): (&Desktop, &Note),
    now: (u64, i64),
    style: Style,
    tr: &Translator,
) -> Result<NodeId, UiError> {
    let flex = Flex::row()
        .gap(Space::S12)
        .padding(Padding::all(if style == Style::Compact {
            Space::S12
        } else {
            Space::S16
        }))
        .cross(CrossAlign::Start);
    let card = Card {
        flex,
        floating: style == Style::Banner,
        raised: style != Style::Banner,
        tone: CardTone::Plain,
    };
    let card = view.ui.add_sized(parent, card, Sizing::AUTO)?;
    let (icon, name) = sender(d, &note.app_id);
    view.ui
        .add(card, Icon::new(icon).color(ColorRole::TextMuted))?;
    let body = view
        .ui
        .add_sized(card, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    header(view, body, (name, note), now, style)?;
    let title = Text::new(label_of(&note.title, tr))
        .spec(TextSpec::new(TypeSize::S14).weight(Weight::Semibold));
    view.ui.add(body, title)?;
    match &note.body {
        Body::Empty => {}
        Body::Localized(t) => {
            view.ui.add(body, small(label_of(t, tr)))?;
        }
        Body::Verbatim(s) => {
            view.ui.add(body, small(Label::data(s)))?;
        }
    }
    actions(view, body, note, tr)?;
    let title = label_of(&note.title, tr).resolve(tr).into_owned();
    let name = Label::key_with("desk.cal.dismiss", &[("title", &title)]);
    let close = view.ui.add(card, IconButton::new(IconId::Close, name))?;
    view.mark(close, Target::Dismiss(note.id));
    Ok(card)
}

/// note.action: the notification's buttons (≤ 3, board Toasts: the first primary, the
/// others plain, 8 apart).
fn actions(view: &mut View, body: NodeId, note: &Note, tr: &Translator) -> Result<(), UiError> {
    if note.actions.is_empty() {
        return Ok(());
    }
    let row = view.ui.add(
        body,
        Flex::row().gap(Space::S8).padding(Padding {
            start: None,
            end: None,
            top: Some(Space::S8),
            bottom: None,
        }),
    )?;
    for (i, action) in note.actions.iter().take(MAX_ACTIONS).enumerate() {
        let variant = if i == 0 {
            ButtonVariant::Primary
        } else {
            ButtonVariant::Plain
        };
        let button = Button::new(label_of(&action.label, tr)).variant(variant);
        let id = view.ui.add(row, button)?;
        view.mark(id, Target::NoteAction(note.id, i as u8));
    }
    Ok(())
}

/// The first line of a card: who sent it and when, "important" for urgent ones.
fn header(
    view: &mut View,
    body: NodeId,
    (name, note): (Label, &Note),
    now: (u64, i64),
    style: Style,
) -> Result<(), UiError> {
    let head = view.ui.add(
        body,
        Flex::row()
            .gap(Space::S8)
            .cross(CrossAlign::Center)
            .main(MainAlign::SpaceBetween),
    )?;
    let from = view
        .ui
        .add(head, Flex::row().gap(Space::S4).cross(CrossAlign::Center))?;
    if style == Style::Compact {
        // Board CompactQuick: the sender at the start, the time at the end.
        view.ui.add(from, small(name))?;
        view.ui.add(head, small(when(note, now)))?;
    } else {
        for part in [name, Label::data("·"), when(note, now)] {
            view.ui.add(from, small(part))?;
        }
    }
    if note.urgency == Urgency::High {
        let badge = Badge::new(Label::key("desk.note.important")).tone(Tone::Warning);
        view.ui.add(head, badge)?;
    }
    Ok(())
}
