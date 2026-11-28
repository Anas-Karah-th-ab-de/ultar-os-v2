//! qs.mesh.list: the paired devices inside the quick-settings sheet (the arrow of the Mesh
//! tile). Connected devices first; a click on a connected one continues what is in front
//! there, a click on one that is not connected opens its place in Settings. The names are
//! data from the network: plain text, never interpreted.

use super::{Target, View};
use crate::model::Desktop;
use ultar_bus::messages::mesh::{DeviceKind, Peer};
use ultar_ui::icon::IconId;
use ultar_ui::layout::{CrossAlign, Flex, Sizing};
use ultar_ui::tokens::Space;
use ultar_ui::ui::{NodeId, UiError};
use ultar_ui::widgets::{IconButton, Label, List, ListItem, Text};

/// Most devices listed.
pub const MAX_DEVICES: usize = 12;
/// Longest device name shown (characters).
pub const MAX_NAME: usize = 48;

fn icon(kind: DeviceKind) -> IconId {
    match kind {
        DeviceKind::Phone => IconId::Phone,
        DeviceKind::Desktop | DeviceKind::Server => IconId::Monitor,
        _ => IconId::Mesh,
    }
}

/// The paired devices, connected ones first (the order of the service otherwise).
pub fn paired(d: &Desktop) -> Vec<&Peer> {
    let mut peers: Vec<&Peer> = d
        .svc
        .mesh
        .state
        .iter()
        .flat_map(|m| &m.peers)
        .filter(|p| p.paired)
        .collect();
    peers.sort_by_key(|p| !p.reachable);
    peers
}

/// The page: back and title, then one row per paired device.
pub fn page(view: &mut View, card: NodeId, d: &Desktop) -> Result<(), UiError> {
    let head = view
        .ui
        .add(card, Flex::row().gap(Space::S8).cross(CrossAlign::Center))?;
    let back = view.ui.add(
        head,
        IconButton::new(IconId::Back, Label::key("desk.qs.back")),
    )?;
    view.mark(back, Target::QuickBack);
    view.ui.add(head, Text::new(Label::key("desk.qs.mesh")))?;
    let peers = paired(d);
    if peers.is_empty() {
        view.ui
            .add(card, Text::new(Label::key("desk.mesh.list.empty")))?;
        return Ok(());
    }
    let list = List::new().name(Label::key("desk.mesh.list.devices"));
    let list = view.ui.add_sized(card, list, Sizing::grow(1))?;
    for peer in peers.into_iter().take(MAX_DEVICES) {
        let name: String = peer.name.chars().take(MAX_NAME).collect();
        let state = if peer.reachable {
            Label::key("desk.mesh.list.here")
        } else {
            Label::key("desk.mesh.target.offline")
        };
        let item = ListItem::new(Label::data(&name))
            .sublabel(state)
            .icon(icon(peer.kind));
        let id = view.ui.add(list, item)?;
        view.mark(id, Target::MeshDevice(peer.id.clone()));
    }
    Ok(())
}
