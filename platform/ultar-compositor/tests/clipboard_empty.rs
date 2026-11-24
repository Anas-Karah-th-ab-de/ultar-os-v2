//! Ctrl+V with nothing on the clipboard (or not the wanted type) gets a cancelled transfer;
//! the app keeps its connection (found in VMware: pasting with an empty clipboard ended Files).

mod common;

use common::*;
use std::time::Duration;
use ultar_surface::proto::{Event, Request};

fn key_serial(p: &Peer) -> u32 {
    p.events
        .iter()
        .rev()
        .find_map(|e| match e {
            Event::Key {
                serial, state: 1, ..
            } => Some(*serial),
            _ => None,
        })
        .expect("key delivered")
}

#[test]
fn pasting_from_an_empty_clipboard_is_cancelled_not_fatal() {
    let mut comp = compositor(test_config(), &[(320, 200)]);
    let mut b = Peer::hello(&mut comp);
    b.window(&mut comp, 1, 0x0000ff);
    let kb = comp.state.windows[0].key;
    comp.state.focus_window(kb);
    key(&mut comp, 47, true);
    b.pump(&mut comp);
    b.take();
    let serial = key_serial_of(&mut b, &mut comp);
    b.send(&Request::ReadSelection {
        serial,
        mime: "text/plain;charset=utf-8".into(),
    });
    b.pump(&mut comp);
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    b.pump(&mut comp);
    assert!(b
        .events
        .iter()
        .any(|e| matches!(e, Event::TransferCancelled { .. })));
    assert!(
        !b.events.iter().any(|e| matches!(e, Event::Error { .. })),
        "no protocol error"
    );
    assert!(comp.state.clients.len() == 1, "still connected");
}

fn key_serial_of(b: &mut Peer, comp: &mut ultar_compositor::Compositor) -> u32 {
    key(comp, 47, false);
    key(comp, 47, true);
    b.pump(comp);
    key_serial(b)
}
