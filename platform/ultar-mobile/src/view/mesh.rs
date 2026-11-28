//! The choice on the sending device (board CompactMeshSend, docs/ui/mesh.md §2.5) and the
//! notices about an offer (board MeshStates, phone form).
//!
//! The choice: the page of the app with its back arrow behind a dimmed scrim, and the sheet
//! "Continue on another device" at the bottom – a radio group of the devices (name, then
//! what happens there), Send (primary, above) and Cancel. Opens only when two or more
//! devices can take the activity. The notice sits above the navigation bar: what happened,
//! its button (Cancel while waiting, Retry when nobody answered or the connection was lost,
//! Open for a file that arrived) and Close.

use super::home::small;
use super::Builder;
use crate::click::Click;
use crate::model::mesh::{stage_key, Act};
use crate::model::{Mobile, Screen};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    Brick, BrickKind, Button, ButtonVariant, Card, IconButton, Label, RadioGroup, RadioLayout, Text,
};

/// Longest device name shown (characters).
const MAX_NAME: usize = 64;

/// Builds the choice.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    // The page behind the sheet: its back arrow and the app's name (board: 24/700).
    let head = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .padding(Padding::symmetric(Space::S16, Space::S12));
    let head = b.flex(root, head, Sizing::AUTO)?;
    let back = IconButton::new(IconId::Back, Label::key("mobile.nav.back"));
    b.click(head, back, Sizing::AUTO, Click::MeshBack)?;
    let app = m.mesh.choice.as_ref().and_then(|c| m.app(&c.app));
    if let Some(app) = app {
        let spec = TextSpec::title(TypeSize::S24).weight(Weight::Bold);
        let name = Text::new(super::app_label(app)).spec(spec);
        b.add(head, name, Sizing::grow(1))?;
    }
    // The dimmed page, then the sheet.
    let scrim = Brick::new(BrickKind::Scrim, Label::key("mobile.dialog.scrim"));
    b.add(root, scrim, Sizing::grow(1))?;
    let padding = Padding {
        start: Some(Space::S16),
        end: Some(Space::S16),
        top: Some(Space::S16),
        bottom: Some(Space::S24),
    };
    let column = Flex::column().padding(padding).gap(Space::S12);
    let sheet = b.add(root, Card::new().raised().flex(column), Sizing::AUTO)?;
    let heading = TextSpec::title(TypeSize::S20)
        .weight(Weight::Semibold)
        .overflow(Overflow::Wrap);
    let title = Text::new(Label::key("mobile.mesh.choose")).spec(heading);
    b.add(sheet, title, Sizing::AUTO)?;
    let hint = small(Label::key("mobile.mesh.choose.hint"))
        .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap));
    b.add(sheet, hint, Sizing::AUTO)?;
    if let Some(choice) = &m.mesh.choice {
        let names = choice
            .targets
            .iter()
            .map(|t| Label::data(&t.name.chars().take(MAX_NAME).collect::<String>()))
            .collect();
        let mut group = RadioGroup::new(Label::key("mobile.mesh.devices"), names)
            .layout(RadioLayout::Cards)
            .selected(choice.chosen);
        for (i, t) in choice.targets.iter().enumerate() {
            group = group.detail(i, Label::key(stage_key(t.stage)));
        }
        let id = b.add(sheet, group, Sizing::AUTO)?;
        b.built.targets = Some(id);
    }
    let send = Button::new(Label::key("mobile.mesh.send"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Mesh);
    b.click(sheet, send, Sizing::AUTO, Click::MeshConfirm)?;
    let cancel = Button::new(Label::key("mobile.mesh.cancel"));
    b.click(sheet, cancel, Sizing::AUTO, Click::MeshCancel)?;
    Ok(())
}

/// The notice about an offer, above the navigation bar.
pub fn notice(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    if m.screen == Screen::Mesh {
        return Ok(());
    }
    let Some(n) = m.mesh.notice_card(m.now_ms) else {
        return Ok(());
    };
    let row = Flex::row()
        .gap(Space::S8)
        .cross(CrossAlign::Center)
        .padding(Padding::all(Space::S12));
    let card = b.add(root, Card::new().floating().flex(row), Sizing::AUTO)?;
    let spec = TextSpec::new(TypeSize::S14).overflow(Overflow::Wrap);
    let text = Text::new(Label::key(n.key))
        .spec(spec)
        .color(ColorRole::Text);
    b.add(card, text, Sizing::grow(1))?;
    let button = match &n.act {
        Some(Act::Cancel(_)) => Some(("mobile.mesh.cancel", Click::MeshOfferCancel)),
        Some(Act::Retry) => Some(("mobile.mesh.retry", Click::MeshRetry)),
        Some(Act::Open(_)) => Some(("mobile.mesh.open", Click::MeshOpen)),
        None => None,
    };
    if let Some((key, click)) = button {
        b.click(card, Button::new(Label::key(key)), Sizing::AUTO, click)?;
    }
    let close = IconButton::new(IconId::Close, Label::key("mobile.mesh.notice_close"));
    b.click(card, close, Sizing::AUTO, Click::MeshNoticeClose)?;
    Ok(())
}
