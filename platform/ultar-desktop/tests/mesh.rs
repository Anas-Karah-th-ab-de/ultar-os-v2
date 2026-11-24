//! The mesh in the shell: the tile follows `mesh.state` and switches it, and "continue on
//! another device" is in the active app's menu and on Super+M.

mod common;

use common::*;
use ultar_bus::messages::mesh::{DeviceKind, MeshState, Peer, Stage, Target};
use ultar_desktop::model::quick::{tiles, TileId};
use ultar_desktop::model::services::Call;
use ultar_desktop::model::Open;
use ultar_desktop::shell::{open_menu, Effect};

fn mesh(enabled: bool, reachable: bool) -> MeshState {
    MeshState {
        enabled,
        id: "me".into(),
        name: "pc".into(),
        peers: vec![Peer {
            id: "phone".into(),
            name: "KM One".into(),
            kind: DeviceKind::Phone,
            capabilities: Vec::new(),
            paired: true,
            reachable,
            rights: Vec::new(),
        }],
    }
}

fn calls(effects: &[Effect]) -> Vec<Call> {
    let service = |e: &Effect| match e {
        Effect::Service(call) => Some(call.clone()),
        _ => None,
    };
    effects.iter().filter_map(service).collect()
}

#[test]
fn super_m_offers_what_is_in_front_when_a_paired_device_is_connected() {
    let mut h = Harness::new(ultar_i18n::Lang::En, ultar_ui::tokens::ThemeKind::Dark);
    let s = &mut h.shell;
    assert!(s.on_shell_key(50).effects.is_empty(), "no mesh service");
    s.d.svc.mesh.state = Some(mesh(true, false));
    assert!(s.on_shell_key(50).effects.is_empty(), "nobody connected");
    s.d.svc.mesh.state = Some(mesh(false, true));
    assert!(s.on_shell_key(50).effects.is_empty(), "mesh off");
    s.d.svc.mesh.state = Some(mesh(true, true));
    let sent = calls(&s.on_shell_key(50).effects);
    assert!(
        matches!(sent.as_slice(), [Call::MeshSend(None, _)]),
        "{sent:?}"
    );
}

#[test]
fn the_choice_of_devices_sends_to_the_chosen_one() {
    let mut h = Harness::new(ultar_i18n::Lang::Ar, ultar_ui::tokens::ThemeKind::Dark);
    let s = &mut h.shell;
    s.d.svc.mesh.state = Some(mesh(true, true));
    let target = |device: &str, stage| Target {
        device: device.into(),
        name: device.into(),
        kind: DeviceKind::Phone,
        stage,
    };
    s.d.svc.mesh.targets = vec![target("phone", Stage::State), target("tv", Stage::File)];
    s.d.open = Open::AppMenu;
    let (_, menu) = open_menu(s).expect("the choice is a menu");
    assert_eq!(menu.items.len(), 3, "a heading and two devices");
    let out = s.on_menu(501);
    let sent = calls(&out.effects);
    assert!(
        matches!(sent.as_slice(), [Call::MeshSend(Some(d), _)] if d == "tv"),
        "{sent:?}"
    );
    assert!(s.d.svc.mesh.targets.is_empty(), "the choice is over");
}

#[test]
fn the_tile_shows_and_switches_the_mesh() {
    let mut h = Harness::new(ultar_i18n::Lang::En, ultar_ui::tokens::ThemeKind::Dark);
    let s = &mut h.shell;
    s.d.presence.mesh = true;
    s.d.svc.mesh.state = Some(mesh(true, true));
    let all = tiles(&s.d.presence, &s.d.values, &s.d.svc);
    let tile = all.iter().find(|t| t.id == TileId::Mesh).expect("tile");
    assert!(tile.on);
    s.d.svc.mesh.state = Some(mesh(false, true));
    let all = tiles(&s.d.presence, &s.d.values, &s.d.svc);
    assert!(!all.iter().find(|t| t.id == TileId::Mesh).expect("tile").on);
}
