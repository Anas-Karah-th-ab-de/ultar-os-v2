//! The choice on the sending device (board MeshSend, docs/ui/mesh.md §2.5): a centred
//! dialog with a radio group of the devices (name, then what happens there; a device that is
//! not connected cannot be chosen), then Cancel and Send. Opens only when two or more
//! devices can take the activity.

use super::geometry::{self, Placement};
use super::{new_ui, Look, Target, View};
use crate::model::apps::installed;
use crate::model::{mesh, Desktop};
use ultar_i18n::Translator;
use ultar_ui::geom::Size;
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, MainAlign, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{Overflow, Weight};
use ultar_ui::tokens::{ColorRole, Space, TypeSize};
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, RadioGroup, RadioLayout, Text};

/// Width of the dialog (board MeshSend).
const WIDTH: f32 = 480.0;
/// Longest device name shown (characters).
const MAX_NAME: usize = 64;

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
    let card = ui.add_sized(
        root,
        Card::new().floating().flex(flex),
        Sizing::width(width),
    )?;
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
    let front = mesh::in_front(d);
    let what = front.as_ref().map(|(_, t)| t.as_str()).unwrap_or_default();
    let what = what.chars().take(96).collect::<String>();
    let text = Label::key_with("desk.mesh.dialog.text", &[("name", what.as_str())]);
    let text = Text::new(text)
        .spec(TextSpec::new(TypeSize::S14).overflow(Overflow::Wrap))
        .color(ColorRole::TextMuted);
    view.ui.add(card, text)?;
    let app = front
        .map(|(id, _)| installed(&id).display_name(tr))
        .unwrap_or_default();
    devices(view, card, d, &app)?;
    buttons(view, card)
}

/// The radio group of the devices that can take it; paired devices that are not connected
/// follow, not available, with "not connected now".
fn devices(view: &mut View, card: NodeId, d: &Desktop, app: &str) -> Result<(), UiError> {
    let name = |n: &str| Label::data(&n.chars().take(MAX_NAME).collect::<String>());
    let mut options = Vec::new();
    let mut details = Vec::new();
    for t in &d.svc.mesh.targets {
        options.push(name(&t.name));
        details.push(Label::key_with(mesh::stage_key(t.stage), &[("app", app)]));
    }
    let offline = d
        .svc
        .mesh
        .state
        .iter()
        .flat_map(|m| &m.peers)
        .filter(|p| p.paired && !p.reachable);
    let first_offline = options.len();
    options.extend(offline.map(|p| name(&p.name)));
    let mut group = RadioGroup::new(Label::key("desk.mesh.dialog.title"), options)
        .layout(RadioLayout::Cards)
        .selected(d.svc.mesh.chosen);
    for (i, detail) in details.into_iter().enumerate() {
        group = group.detail(i, detail);
    }
    for i in first_offline..group.options.len() {
        group = group.unavailable(i, Label::key("desk.mesh.target.offline"));
    }
    let id = view.ui.add(card, group)?;
    view.mark(id, Target::MeshTargets);
    Ok(())
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
