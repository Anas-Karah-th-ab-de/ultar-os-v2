//! "Continue on another device" on the sending side (docs/ui/mesh.md §2.5): the entry of
//! the active app's menu, the choice dialog when more than one device can take it (board
//! MeshSend), the send call, and the buttons of the mesh notices (board MeshStates).

use super::{Effect, Opening, Outcome, Shell};
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

/// A click on a paired device in the list of the Mesh tile (qs.mesh.list): a connected one
/// gets what is in front (a notice says when there is nothing to hand over), one that is not
/// connected opens the Mesh area of Settings.
pub(super) fn list_device(s: &mut Shell, id: &str) -> Outcome {
    let here =
        s.d.svc
            .mesh
            .state
            .iter()
            .flat_map(|m| &m.peers)
            .any(|p| p.id == id && p.reachable);
    if here && mesh::can_send(&s.d.svc) {
        return send(s, Some(id.to_owned()));
    }
    super::bar::launch(s, super::bar::SETTINGS, Opening::settings_page("mesh"))
}

/// A click inside the open choice dialog.
pub(super) fn on_popup(s: &mut Shell, target: &Target, kind: &ActionKind) -> Option<Outcome> {
    match (target, kind) {
        (Target::MeshTargets, ActionKind::Selected(i)) if s.d.svc.mesh.choose(*i) => {
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

/// The button of a mesh toast (board MeshStates): take the waiting offer back, send it
/// again, or open what arrived.
pub fn toast_action(s: &mut Shell, act: mesh::Act) -> Outcome {
    match act {
        mesh::Act::Cancel(offer) => Outcome::effect(Effect::Service(Call::OfferCancel(offer))),
        // Nobody answered or the connection was lost: send the offer again.
        mesh::Act::Retry(device) => {
            let front = mesh::in_front(&s.d);
            Outcome::effect(Effect::Service(Call::MeshSend(Some(device), front)))
        }
        // What arrived opens in Files, which hands it to the file's app.
        mesh::Act::Open(uri) => Outcome::effect(Effect::Launch {
            app: super::bar::FILES.to_owned(),
            open: Opening::uri(uri),
        }),
    }
}
