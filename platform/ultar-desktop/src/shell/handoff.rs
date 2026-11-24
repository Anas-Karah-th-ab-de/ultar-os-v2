//! "Continue on another device" on the sending side (docs/ui/mesh.md §2.5): the entry of
//! the active app's menu, the choice when more than one device can take it, and the call.

use super::{Effect, Outcome, Shell};
use crate::model::services::Call;
use crate::model::{mesh, Open};
use ultar_ui::icon::IconId;
use ultar_ui::widgets::{Label, Menu, MenuItem};

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

/// The choice: a heading, then one line per device saying what will happen there.
pub(super) fn targets_menu(s: &Shell, head: u32, base: u32) -> Menu {
    let head = MenuItem::new(head, Label::key("desk.mesh.choose")).disabled();
    let devices = s.d.svc.targets.iter().enumerate().map(|(i, t)| {
        let label = Label::key_with(mesh::stage_key(t.stage), &[("name", t.name.as_str())]);
        MenuItem::new(base + i as u32, label)
    });
    Menu::new(std::iter::once(head).chain(devices).collect())
}

/// Continue what is in front on `device`, or wherever it can go.
pub(super) fn send(s: &mut Shell, device: Option<String>) -> Outcome {
    if !mesh::can_send(&s.d.svc) {
        return Outcome::none();
    }
    let front = mesh::in_front(&s.d);
    s.d.open = Open::None;
    Outcome::effect_and_close(Effect::Service(Call::MeshSend(device, front)))
}
