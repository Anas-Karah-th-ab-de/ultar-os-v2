//! The on-screen keyboard: it hears about a focused text field before it shows its panel,
//! types text and editing keys into that field only while its panel is shown, and nothing
//! without the right `ui.input-method`.

mod common;
use common::*;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{Event, ImeKey, Request, Rights, Role, SurfaceId};

fn my_uid() -> u32 {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

fn config_with(rights: Rights) -> Config {
    let mut table = RightsTable::default();
    table.grant(my_uid(), rights);
    Config {
        rights: table,
        ..test_config()
    }
}

/// Creates a surface with `role` and fills it with one colour.
fn surface(p: &mut Peer, comp: &mut Compositor, (id, role): (u32, Role), size: (u32, u32)) {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    let serial = p.last_configure(s).map_or(0, |c| c.serial);
    p.fill(comp, s, serial, size, 0x333333);
}

fn ime_state(p: &Peer) -> Option<(bool, u8)> {
    p.events.iter().rev().find_map(|e| match e {
        Event::ImeState { active, purpose } => Some((*active, *purpose)),
        _ => None,
    })
}

fn keys(p: &Peer, code: u16) -> Vec<u8> {
    p.events
        .iter()
        .filter_map(|e| match e {
            Event::Key { code: c, state, .. } if *c == code => Some(*state),
            _ => None,
        })
        .collect()
}

#[test]
fn the_keyboard_follows_the_field_and_types_into_it_only_with_its_panel() {
    let mut comp = compositor(config_with(Rights::INPUT_METHOD), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    let mut osk = Peer::hello(&mut comp);
    app.send(&Request::TextInput {
        surface: s,
        enabled: true,
        purpose: 2,
    });
    app.pump(&mut comp);
    osk.pump(&mut comp);
    assert_eq!(
        ime_state(&osk),
        Some((true, 2)),
        "told before it has a panel"
    );
    // Without a panel an input method may not type (a refused request ends that client).
    let mut bare = Peer::hello(&mut comp);
    bare.send(&Request::ImeKey {
        key: ImeKey::Backspace,
    });
    bare.pump(&mut comp);
    app.pump(&mut comp);
    assert!(keys(&app, 14).is_empty());
    // With the panel shown: Backspace arrives as a press and a release, the text as text.
    surface(&mut osk, &mut comp, (2, Role::InputPanel), (800, 240));
    osk.send(&Request::ImeKey {
        key: ImeKey::Backspace,
    });
    osk.send(&Request::ImeKey { key: ImeKey::Enter });
    osk.send(&Request::ImeCommit {
        text: "مرحبا".into(),
    });
    osk.pump(&mut comp);
    app.pump(&mut comp);
    assert_eq!(keys(&app, 14), vec![1, 0]);
    assert_eq!(keys(&app, 28), vec![1, 0]);
    assert!(app
        .events
        .iter()
        .any(|e| matches!(e, Event::Text { text, .. } if text == "مرحبا")));
    // The field loses focus: the keyboard is told.
    app.send(&Request::TextInput {
        surface: s,
        enabled: false,
        purpose: 0,
    });
    app.pump(&mut comp);
    osk.pump(&mut comp);
    assert_eq!(ime_state(&osk).map(|s| s.0), Some(false));
}

#[test]
fn a_client_without_the_right_hears_nothing_and_types_nothing() {
    let mut comp = compositor(config_with(Rights::NONE), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    let mut other = Peer::hello(&mut comp);
    app.send(&Request::TextInput {
        surface: s,
        enabled: true,
        purpose: 0,
    });
    app.pump(&mut comp);
    other.pump(&mut comp);
    assert_eq!(ime_state(&other), None);
    other.send(&Request::ImeKey { key: ImeKey::Enter });
    other.pump(&mut comp);
    app.pump(&mut comp);
    assert!(keys(&app, 28).is_empty());
}
