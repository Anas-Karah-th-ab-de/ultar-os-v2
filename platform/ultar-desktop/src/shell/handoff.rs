//! "Continue on another device" on the sending side (docs/ui/mesh.md §2.5): the entry of
//! the active app's menu, the choice dialog when more than one device can take it (board
//! MeshSend), the send call, and the buttons of the mesh notices (board MeshStates).

use super::{Effect, Outcome, Shell};
use crate::model::services::Call;
use crate::model::{mesh, Open};
use crate::view::Target;
use ultar_ui::event::ActionKind;
use ultar_ui::icon::IconId;
use ultar_ui::widgets::{Label, MenuItem};

/// The menu entry: usable while the mesh is on and a paired device is connected.
pub(super) fn item(s: &Shell, id: u32) -> MenuItem {
    let item = MenuItem::new(id, Label::key("desk.mesh.continue"))
        .icon(IconId::Mesh)
        .separated();
    if mesh::can_send(&s.d.svc) {
        item
    } else {
        item.disabled()
    }
}

/// Continue what is in front on `device`, or wherever it can go (`mesh.handoff_send`).
pub(super) fn send(s: &mut Shell, device: Option<String>) -> Outcome {
    if !mesh::can_send(&s.d.svc) {
        return Outcome::none();
    }
    let front = mesh::in_front(&s.d);
    s.d.open = Open::None;
    Outcome::effect_and_close(Effect::Service(Call::MeshSend(device, front)))
}

/// A click inside the open choice dialog.
pub(super) fn on_popup(s: &mut Shell, target: &Target, kind: &ActionKind) -> Option<Outcome> {
    match (target, kind) {
        (Target::MeshTarget(i), ActionKind::Activated) if s.d.svc.mesh.choose(*i) => {
            Some(Outcome::surfaces())
        }
        (Target::MeshSend, ActionKind::Activated) => {
            let device = s.d.svc.mesh.confirm();
            Some(confirm(s, device))
        }
        (Target::MeshSendCancel, ActionKind::Activated) => Some(s.close()),
        _ => None,
    }
}

/// Sends to the chosen device and closes the dialog.
fn confirm(s: &mut Shell, device: Option<String>) -> Outcome {
    s.d.svc.mesh.targets.clear();
    s.d.svc.mesh.chosen = 0;
    send(s, device)
}

/// A mesh notice button (`mesh:…`); `None` when the action is not the mesh's.
pub(super) fn note_action(s: &mut Shell, action: &str) -> Option<Outcome> {
    let rest = action.strip_prefix("mesh:")?;
    if let Some(offer) = rest.strip_prefix("cancel:") {
        let offer = offer.parse().ok()?;
        return Some(Outcome::effect(Effect::Service(Call::OfferCancel(offer))));
    }
    if let Some(device) = rest.strip_prefix("retry:") {
        // Nobody answered or the connection was lost: send the offer again.
        let front = mesh::in_front(&s.d);
        return Some(Outcome::effect(Effect::Service(Call::MeshSend(
            Some(device.to_owned()),
            front,
        ))));
    }
    None
}