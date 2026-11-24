//! Parts every surface shares: the header with the own number, notices about the ways and
//! the last call, and the dialogs (ready messages, screen sharing).
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use super::Builder;
use crate::click::{Click, WindowOp};
use crate::model::{format_duration, Dialog, Model};
use crate::text::{end_label, line_notice, ready_message, TEXTS};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{FontRole, Overflow, Weight};
use ultar_ui::tokens::{size, ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{
    BarEdge, Button, ButtonVariant, Card, Dialog as UiDialog, IconButton, InfoBar, Label, Text,
};

/// Small muted text.
pub(crate) fn small(label: Label) -> Text {
    Text::new(label)
        .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap))
        .color(ColorRole::TextMuted)
}

/// Fixed-width text (numbers).
pub(crate) fn mono(label: Label, size: TypeSize) -> Text {
    Text::new(label).spec(TextSpec::new(size).role(FontRole::Mono))
}

/// A heading (title font, semibold).
pub(crate) fn heading(label: Label, size: TypeSize, weight: Weight) -> Text {
    Text::new(label).spec(TextSpec::title(size).weight(weight))
}

/// Header: own number with copy button at the start, title in the middle, window buttons at
/// the end (not on a phone, where the app fills the screen).
pub(crate) fn header(b: &mut Builder<'_>, root: NodeId, model: &Model) -> Result<(), UiError> {
    let flex = Flex::row()
        .padding(Padding::symmetric(Space::S4, Space::S4))
        .cross(CrossAlign::Center);
    let bar = Card::bar(BarEdge::Bottom).flex(flex);
    let bar = b.add(
        root,
        bar,
        Sizing::grow(0).with_height(size::TITLEBAR_HEIGHT),
    )?;
    // Phone: title at the start, own number at the end (no window buttons, no centring).
    let title = heading(Label::key("phone.title"), TypeSize::S14, Weight::Semibold);
    if b.compact() {
        b.add(bar, title.clone(), Sizing::grow(1))?;
    }
    let start = b.flex(
        bar,
        Flex::row().gap(Space::S8).cross(CrossAlign::Center),
        Sizing::grow(u8::from(!b.compact())),
    )?;
    if let Some(own) = model.state.lines.own_number.as_deref() {
        b.add(start, small(Label::key("phone.my_number")), Sizing::AUTO)?;
        let number = mono(Label::data(own), TypeSize::S12).color(ColorRole::TextMuted);
        b.add(start, number, Sizing::AUTO)?;
        let copy = IconButton::new(IconId::Copy, Label::key("phone.copy"));
        b.click(start, copy, Sizing::AUTO, Click::CopyNumber)?;
    }
    if b.compact() {
        return Ok(());
    }
    b.add(bar, title, Sizing::AUTO)?;
    let end = b.flex(
        bar,
        Flex::row().gap(Space::S4).main(MainAlign::End),
        Sizing::grow(1),
    )?;
    {
        for (icon, key, op) in [
            (IconId::Min, "phone.win.min", WindowOp::Minimize),
            (IconId::Max, "phone.win.max", WindowOp::Maximize),
            (IconId::Close, "phone.win.close", WindowOp::Close),
        ] {
            b.click(
                end,
                IconButton::new(icon, Label::key(key)),
                Sizing::AUTO,
                Click::Window(op),
            )?;
        }
    }
    Ok(())
}

/// Notice about the chosen way (no SIM, airplane mode, …) and about the last call.
pub(crate) fn notices(b: &mut Builder<'_>, parent: NodeId, model: &Model) -> Result<(), UiError> {
    if let Some((tone, key)) = line_notice(model) {
        b.add(parent, InfoBar::new(tone, Label::key(key)), Sizing::AUTO)?;
    }
    if let Some(ended) = model.ended() {
        let time = format_duration(model.duration_s(ended));
        let (tone, label) = end_label(ended, &time);
        let bar = b.add(parent, InfoBar::new(tone, label), Sizing::AUTO)?;
        let again = Button::new(Label::key("phone.again")).variant(ButtonVariant::Quiet);
        b.click(bar, again, Sizing::AUTO, Click::Again)?;
    }
    Ok(())
}

/// The open dialog of the model, if any.
pub(crate) fn dialogs(b: &mut Builder<'_>, root: NodeId, model: &Model) -> Result<(), UiError> {
    let Some(dialog) = &model.dialog else {
        return Ok(());
    };
    let column = Flex::column().gap(Space::S12);
    let id = match dialog {
        Dialog::Text { name, .. } => {
            let title = Label::key_with("phone.text.title", &[("name", name)]);
            let d = b.add(root, UiDialog::new(title).flex(column), Sizing::AUTO)?;
            for (i, key) in TEXTS.iter().enumerate() {
                let item = Button::new(ready_message(key)).icon(IconId::Message);
                b.click(d, item, Sizing::grow(1), Click::Text(i))?;
            }
            cancel(b, d, d)?;
            d
        }
        Dialog::Share(call) => {
            let who = model
                .call(*call)
                .map(Model::who)
                .unwrap_or_default()
                .to_owned();
            let title = Label::key_with("phone.share.title", &[("name", &who)]);
            let d = b.add(root, UiDialog::new(title).flex(column), Sizing::AUTO)?;
            b.add(d, Text::new(Label::key("phone.share.body")), Sizing::AUTO)?;
            let row = b.flex(
                d,
                Flex::row().gap(Space::S8).main(MainAlign::End),
                Sizing::AUTO,
            )?;
            cancel(b, row, d)?;
            let ok = Button::new(Label::key("phone.share.confirm"))
                .variant(ButtonVariant::Primary)
                .icon(IconId::ScreenShare);
            let ok = b.click(row, ok, Sizing::AUTO, Click::ShareConfirm(*call))?;
            b.ui.set_default_action(d, Some(ok))?;
            d
        }
    };
    b.ui.open_dialog(id)?;
    b.built.dialog = Some(id);
    Ok(())
}

/// The cancel button of `dialog` (also what Escape does).
fn cancel(b: &mut Builder<'_>, parent: NodeId, dialog: NodeId) -> Result<NodeId, UiError> {
    let c = Button::new(Label::key("phone.cancel"));
    let id = b.click(parent, c, Sizing::AUTO, Click::CloseDialog)?;
    b.ui.set_cancel_action(dialog, Some(id))?;
    Ok(id)
}
