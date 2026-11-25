//! The choice on the sending device (board CompactMeshSend, docs/ui/mesh.md §2.5): which
//! device continues the app. Opens only when two or more devices can take it. A list of the
//! devices (name, what will happen there), then Send (primary, above) and Cancel.

use super::home::small;
use super::Builder;
use crate::click::Click;
use crate::model::mesh::stage_key;
use crate::model::Mobile;
use ultar_bus::messages::mesh::DeviceKind;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Icon, Label, Text};

/// The icon of a device kind (as the Account board does).
pub fn kind_icon(kind: DeviceKind) -> IconId {
    match kind {
        DeviceKind::Phone | DeviceKind::Tablet => IconId::Phone,
        DeviceKind::Desktop => IconId::Monitor,
        DeviceKind::Server => IconId::Server,
        _ => IconId::Device,
    }
}

/// Builds the choice.
pub fn body(b: &mut Builder<'_>, root: NodeId, m: &Mobile) -> Result<(), UiError> {
    let main = Flex::column()
        .gap(Space::S12)
        .padding(Padding::symmetric(Space::S16, Space::S16));
    let main = b.flex(root, main, Sizing::grow(1))?;
    let spec = TextSpec::title(TypeSize::S20).weight(Weight::Semibold);
    b.add(
        main,
        Text::new(Label::key("mobile.mesh.choose")).spec(spec),
        Sizing::AUTO,
    )?;
    let hint = small(Label::key("mobile.mesh.choose.hint"))
        .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap));
    b.add(main, hint, Sizing::AUTO)?;
    if let Some(app) = m.mesh.choice.as_ref().and_then(|c| m.app(&c.app)) {
        let line = Button::new(super::app_label(app))
            .icon(app.icon)
            .variant(ButtonVariant::Quiet);
        let id = b.add(main, line, Sizing::AUTO)?;
        b.ui.set_disabled(id, true)?;
    }
    let width = b.screen().w - 2.0 * Space::S16.points();
    if let Some(choice) = &m.mesh.choice {
        for (i, t) in choice.targets.iter().enumerate() {
            device(b, main, i, t, i == choice.chosen, width)?;
        }
    }
    b.spacer(main)?;
    let send = Button::new(Label::key("mobile.mesh.send"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Mesh);
    b.click(main, send, Sizing::width(width), Click::MeshConfirm)?;
    let cancel = Button::new(Label::key("mobile.mesh.cancel"));
    b.click(main, cancel, Sizing::width(width), Click::MeshCancel)?;
    Ok(())
}

/// One device of the choice: chosen = toggled on (the accent edge and a check).
fn device(
    b: &mut Builder<'_>,
    parent: NodeId,
    i: usize,
    t: &ultar_bus::messages::mesh::Target,
    chosen: bool,
    width: f32,
) -> Result<(), UiError> {
    let flex = Flex::row()
        .gap(Space::S12)
        .cross(CrossAlign::Center)
        .padding(Padding::all(Space::S8));
    let card = b.add(parent, Card::new().flex(flex), Sizing::width(width))?;
    b.add(card, Icon::new(kind_icon(t.kind)), Sizing::AUTO)?;
    let col = b.flex(card, Flex::column().gap(Space::S4), Sizing::grow(1))?;
    let name = t.name.chars().take(64).collect::<String>();
    let pick = Button::new(Label::data(&name))
        .variant(ButtonVariant::Quiet)
        .toggled(chosen);
    let pick = if chosen {
        pick.icon(IconId::Check)
    } else {
        pick
    };
    b.click(col, pick, Sizing::AUTO, Click::MeshChoose(i))?;
    let what = Label::key(stage_key(t.stage));
    b.add(col, small(what), Sizing::AUTO)?;
    Ok(())
}
