//! The clipboard bridge (`ui.clipboard-bridge`): the mesh service is told of a
//! copy, may read text and pictures, and offers entries copied on another
//! device. A confidential selection (a password field) never leaves, and
//! nothing is read or set while the session is locked.

mod common;

use common::*;
use ultar_compositor::config::RightsTable;
use ultar_compositor::Config;
use ultar_surface::proto::{Event, Request};

const BRIDGE_UID: u32 = 833;

fn config() -> Config {
    let rights = RightsTable::parse("833 = \"ui.clipboard-bridge\"\n").unwrap();
    Config {
        lang: ultar_ui::Lang::En,
        inline_only: true,
        cursor: false,
        rights,
        ..Config::default()
    }
}

/// A focused toplevel that copies and pastes.
fn focused_app(comp: &mut ultar_compositor::Compositor) -> Peer {
    let mut app = Peer::hello(comp);
    app.window(comp, 1, 0x0000ff);
    let key = comp.state.windows[0].key;
    comp.state.focus_window(key);
    app
}

/// Supplies a fresh user-action serial to `peer` (presses a key).
fn serial(peer: &mut Peer, comp: &mut ultar_compositor::Compositor) -> u32 {
    key(comp, 47, true);
    peer.pump(comp);
    let s = peer
        .events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Key {
                serial,
                state: 1,
                ..
            } => Some(*serial),
            _ => None,
        })
        .expect("key delivered");
    peer.take();
    s
}

fn txt(mime: &str) -> Vec<String> {
    vec![mime.to_owned()]
}

#[test]
fn without_the_right_both_requests_are_refused() {
    let mut comp = compositor(Config::default(), &[(320, 200)]);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    bridge.send(&Request::BridgeSetSelection {
        mimes: txt("text/plain;charset=utf-8"),
    });
    bridge.pump(&mut comp);
    assert_eq!(
        bridge.error(),
        Some(ultar_surface::proto::ErrorCode::Permission)
    );

    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    bridge.send(&Request::BridgeReadSelection {
        mime: "text/plain;charset=utf-8".into(),
    });
    bridge.pump(&mut comp);
    assert_eq!(
        bridge.error(),
        Some(ultar_surface::proto::ErrorCode::Permission)
    );
}

#[test]
fn a_bridge_reads_what_the_focused_app_copied() {
    let mut comp = compositor(config(), &[(320, 200)]);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    let mut app = focused_app(&mut comp);
    let s = serial(&mut app, &mut comp);
    app.send(&Request::SetSelection {
        serial: s,
        mimes: txt("text/plain;charset=utf-8"),
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    let change = bridge.take();
    assert!(change.iter().any(|e| matches!(
        e,
        Event::SelectionChange { confidential: false, mimes } if mimes == &txt("text/plain;charset=utf-8")
    )));

    bridge.send(&Request::BridgeReadSelection {
        mime: "text/plain;charset=utf-8".into(),
    });
    app.pump(&mut comp);
    let transfer = app
        .events
        .iter()
        .find_map(|e| match e {
            Event::SendRequest { transfer, .. } => Some(*transfer),
            _ => None,
        })
        .expect("source is asked");
    app.take();
    app.send(&Request::SendData {
        transfer,
        data: b"hello".to_vec(),
        last: true,
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    assert!(bridge.take().iter().any(|e| matches!(
        e,
        Event::Data { data, last: true, .. } if data == b"hello"
    )));
}

#[test]
fn a_password_is_never_offered_to_the_bridge() {
    let mut comp = compositor(config(), &[(320, 200)]);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    let mut app = focused_app(&mut comp);
    let key = comp.state.windows[0].key;
    app.send(&Request::TextInput {
        surface: key.surface,
        enabled: true,
        purpose: 2,
    });
    app.pump(&mut comp);
    let s = serial(&mut app, &mut comp);
    app.send(&Request::SetSelection {
        serial: s,
        mimes: txt("text/plain;charset=utf-8"),
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    let change = bridge.take();
    assert!(change.iter().any(|e| matches!(
        e,
        Event::SelectionChange { confidential: true, .. }
    )));

    bridge.send(&Request::BridgeReadSelection {
        mime: "text/plain;charset=utf-8".into(),
    });
    bridge.pump(&mut comp);
    assert!(bridge
        .take()
        .iter()
        .any(|e| matches!(e, Event::TransferCancelled { .. })));
}

#[test]
fn locked_sessions_give_the_bridge_nothing() {
    let mut comp = compositor(config(), &[(320, 200)]);
    let mut app = focused_app(&mut comp);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    let s = serial(&mut app, &mut comp);
    app.send(&Request::SetSelection {
        serial: s,
        mimes: txt("text/plain;charset=utf-8"),
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    bridge.take();

    comp.state.session.locked = true;
    bridge.send(&Request::BridgeReadSelection {
        mime: "text/plain;charset=utf-8".into(),
    });
    bridge.pump(&mut comp);
    assert_eq!(
        bridge.error(),
        Some(ultar_surface::proto::ErrorCode::Permission)
    );
}

#[test]
fn a_bridge_set_selection_is_offered_to_the_focused_app() {
    let mut comp = compositor(config(), &[(320, 200)]);
    let mut app = focused_app(&mut comp);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    bridge.send(&Request::BridgeSetSelection {
        mimes: txt("text/plain;charset=utf-8"),
    });
    app.pump(&mut comp);
    assert!(app.take().iter().any(|e| matches!(
        e,
        Event::SelectionOffer { mimes } if mimes == &txt("text/plain;charset=utf-8")
    )));

    let s = serial(&mut app, &mut comp);
    app.send(&Request::ReadSelection {
        serial: s,
        mime: "text/plain;charset=utf-8".into(),
    });
    bridge.pump(&mut comp);
    let transfer = bridge
        .events
        .iter()
        .find_map(|e| match e {
            Event::SendRequest { transfer, .. } => Some(*transfer),
            _ => None,
        })
        .expect("bridge is asked for the data");
    bridge.take();
    bridge.send(&Request::SendData {
        transfer,
        data: b"remote".to_vec(),
        last: true,
    });
    app.pump(&mut comp);
    assert!(app.events.iter().any(|e| matches!(
        e,
        Event::Data { data, last: true, .. } if data == b"remote"
    )));
}

#[test]
fn a_text_entry_over_sixteen_kib_is_cancelled() {
    let mut comp = compositor(config(), &[(320, 200)]);
    let mut bridge = Peer::hello_as(&mut comp, BRIDGE_UID);
    let mut app = focused_app(&mut comp);
    let s = serial(&mut app, &mut comp);
    app.send(&Request::SetSelection {
        serial: s,
        mimes: txt("text/plain;charset=utf-8"),
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    bridge.take();

    bridge.send(&Request::BridgeReadSelection {
        mime: "text/plain;charset=utf-8".into(),
    });
    app.pump(&mut comp);
    let transfer = app
        .events
        .iter()
        .find_map(|e| match e {
            Event::SendRequest { transfer, .. } => Some(*transfer),
            _ => None,
        })
        .expect("source is asked");
    app.take();
    app.send(&Request::SendData {
        transfer,
        data: vec![b'x'; 16 * 1024 + 1],
        last: true,
    });
    app.pump(&mut comp);
    bridge.pump(&mut comp);
    assert!(bridge
        .take()
        .iter()
        .any(|e| matches!(e, Event::TransferCancelled { .. })));
}