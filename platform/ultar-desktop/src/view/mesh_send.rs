//! The choice on the sending device (board MeshSend, docs/ui/mesh.md §2.5): a centred
//! dialog with a radio group of the devices that can take the activity, then Send and
//! Cancel. Opens only when two or more devices can take it.

use super::geometry::{self, Placement};
use super::{mesh, new_ui, Look, Target, View};
use crate::model::Desktop;
use ultar_bus::messages::mesh::DeviceKind;
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, ListItem, Text};

/// Width of the dialog (board MeshSend).
const WIDTH: f32 = 480.0;

/// The icon of a device kind (as the Account board does).
fn kind_icon(kind: DeviceKind) -> IconId {
    match kind {
        DeviceKind::Phone | DeviceKind::Tablet => IconId::Phone,
        DeviceKind::Desktop => IconId::Monitor,
        DeviceKind::Server => IconId::Server,
        _ => IconId::Device,
    }
}

/// Builds the choice of devices; the safe answer (Cancel) keeps the focus.
pub fn build(
    d: &Desktop,
    look: Look,
    screen: Size,
    tr: &Translator,
) -> Result<(View, Placement), UiError> {
    let width = (WIDTH.min(screen.w - 2.0 * Space::S32.points())).max(1.0);
    let mut ui = new_ui(look, screen, tr)?;
    ui.set_transparent_background();
    ui.set_root_flex(Flex::column().cross(CrossAlign::Start));
    // Board MeshSend: padding 24, gap 16.
    let flex = Flex::column()
        .padding(Padding::all(Space::S24))
        .gap(Space::S16);
    let root = ui.root();
    let card = ui.add_sized(root, Card::new().floating().flex(flex), Sizing::width(width))?;
    let mut view = View::new(ui);
    fill(&mut view, card, d, tr)?;
    let height = view.ui.content_size().h;
    let rtl = look.lang.direction().is_rtl();
    let placement = geometry::centred(screen, Size::new(width, height), rtl);
    view.ui
        .set_root_flex(Flex::column().cross(CrossAlign::Start).padding(Padding {
            start: Some(placement.start),
            end: None,
            top: Some(placement.top),
            bottom: None,
        }));
    view.ui.set_size(placement.surface)?;
    Ok((view, placement))
}

fn fill(view: &mut View, card: NodeId, d: &Desktop, tr: &Translator) -> Result<(), UiError> {
    let title = Text::new(Label::key("desk.mesh.dialog.title"))
        .spec(TextSpec::new(TypeSize::S16).weight(Weight::Semibold));
    view.ui.add(card, title)?;
    // The description names what is in front.
    let what = mesh::in_front(d).map(|(_, title)| title).unwrap_or_default();
    let what = what.chars().take(96).collect::<String>();
    let text = Label::key_with("desk.mesh.dialog.text", &[("name", what.as_str())]);
    let text = Text::new(text)
        .spec(TextSpec::new(TypeSize::S12).overflow(Overflow::Wrap))
        .color(ColorRole::TextMuted);
    view.ui.add(card, text)?;
    devices(view, card, d)?;
    let _ = tr;
    buttons(view, card)
}

/// The radio group of the devices that can take it; offline paired devices follow,
/// disabled with "not connected".
fn devices(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let chosen = d.svc.mesh.chosen;
    for (i, t) in d.svc.mesh.targets.iter().enumerate() {
        let name: String = t.name.chars().take(64).collect();
        let what = Label::key(mesh::stage_key(t.stage));
        let id = device(view, card, name, kind_icon(t.kind), what, i == chosen)?;
        view.mark(id, Target::MeshTarget(i));
    }
    for p in d
        .svc
        .mesh
        .state
        .iter()
        .flat_map(|m| &m.peers)
        .filter(|p| p.paired && !p.reachable)
    {
        let name: String = p.name.chars().take(64).collect();
        let id = device(
            view,
            card,
            name,
            kind_icon(p.kind),
            Label::key("desk.mesh.target.offline"),
            false,
        )?;
        view.ui.set_disabled(id, true)?;
    }
    Ok(())
}

/// One device row: name, then the line what will happen there. `chosen` adds the radio's
/// check.
fn device(
    view: &mut View,
    card: NodeId,
    name: String,
    icon: IconId,
    what: Label,
    chosen: bool,
) -> Result<NodeId, UiError> {
    let mut item = ListItem::new(Label::data(&name))
        .sublabel(what)
        .icon(icon);
    if chosen {
        item = item.selected();
    }
    view.ui.add(card, item)
}

/// Cancel (safe) and Send (primary).
fn buttons(view: &mut View, card: NodeId) -> Result<(), UiError> {
    let row = Flex::row().gap(Space::S8).main(MainAlign::End);
    let row = view.ui.add(card, row)?;
    let cancel = Button::new(Label::key("desk.mesh.cancel"));
    let cancel = view.ui.add(row, cancel)?;
    view.mark(cancel, Target::MeshSendCancel);
    let send = Button::new(Label::key("desk.mesh.send"))
        .variant(ButtonVariant::Primary)
        .icon(IconId::Mesh);
    let send = view.ui.add(row, send)?;
    view.mark(send, Target::MeshSend);
    // The safe answer has the focus.
    view.ui.focus(cancel)?;
    Ok(())
}