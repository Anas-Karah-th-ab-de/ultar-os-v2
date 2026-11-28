//! The mesh in the shell: the tile follows `mesh.state` and switches it, and "continue on
//! another device" is in the active app's menu and on Super+Shift+M.

mod common;

use common::*;
use ultar_bus::messages::mesh::{DeviceKind, MeshState, Peer};
use ultar_desktop::model::quick::{tiles, TileId};
use ultar_desktop::model::services::Call;
use ultar_desktop::shell::Effect;

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
fn super_shift_m_offers_what_is_in_front_when_a_paired_device_is_connected() {
    let mut h = Harness::new(ultar_i18n::Lang::En, ultar_ui::tokens::ThemeKind::Dark);
    let s = &mut h.shell;
    assert!(s.on_shell_key_with(50, true).effects.is_empty(), "no mesh service");
    s.d.svc.mesh.state = Some(mesh(true, false));
    assert!(s.on_shell_key_with(50, true).effects.is_empty(), "nobody connected");
    s.d.svc.mesh.state = Some(mesh(false, true));
    assert!(s.on_shell_key_with(50, true).effects.is_empty(), "mesh off");
    s.d.svc.mesh.state = Some(mesh(true, true));
    assert!(s.on_shell_key(50).effects.is_empty(), "Super+M alone is not the shortcut");
    let sent = calls(&s.on_shell_key_with(50, true).effects);
    assert!(
        matches!(sent.as_slice(), [Call::MeshSend(None, _)]),
        "{sent:?}"
    );
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

#[test]
fn whatever_is_typed_in_the_overview_can_be_asked_of_the_helper() {
    use ultar_desktop::model::search::{search_in, Hit, Sources};
    let h = Harness::new(ultar_i18n::Lang::Ar, ultar_ui::tokens::ThemeKind::Dark);
    let tr = &h.shell.tr;
    let ask = |query: &str, helper: bool| {
        let sources = Sources {
            helper,
            ..Sources::default()
        };
        search_in(query, &h.shell.d.catalog, tr, sources)
    };
    // without the helper service nothing is offered; two letters are no request
    assert!(!ask("اكتم الصوت", false)
        .iter()
        .any(|x| matches!(x, Hit::Helper(_))));
    assert!(!ask("ab", true).iter().any(|x| matches!(x, Hit::Helper(_))));
    let hits = ask("اكتم الصوت", true);
    assert_eq!(hits.last(), Some(&Hit::Helper("اكتم الصوت".into())));
    // the helper's own texts are in the shell's catalogs, in the shell's language
    assert_ne!(tr.t("agent.done"), "agent.done");
    assert_ne!(tr.t("agent.help.pairing"), "agent.help.pairing");
}
