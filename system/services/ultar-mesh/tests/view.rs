//! Remote view: a paired device with the right streams its screen and takes the input of
//! another device; two nodes wired together in memory, exactly as the network would.
mod common;
use common::World;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_mesh::node::Effect;
use ultar_mesh::trust::DEFAULT_RIGHTS;
use ultar_mesh::view::{ViewEnd, WINDOW};

const PHONE: (DeviceKind, &[&str]) = (DeviceKind::Phone, &[]);
const PC: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &[]);

fn pair(tag: &str) -> World {
    let mut w = World::new(tag, &[PHONE, PC]);
    w.link(0, 1);
    w.pair(0, 1, None);
    w.take(0);
    w.take(1);
    w
}

/// Device `device` grants `Right::Control` to device `to`.
fn grant(w: &mut World, device: usize, to: usize) {
    let id = w.id(to);
    let mut rights = DEFAULT_RIGHTS.to_vec();
    rights.push(Right::Control);
    assert!(w.nodes[device].set_rights(&id, &rights).0);
}

#[test]
fn without_the_control_right_the_source_refuses() {
    let mut w = pair("view-refused");
    let peer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&peer).unwrap();
    w.run(0, effects);
    // The source refuses outright and never asks its person.
    assert!(w.take(1).is_empty());
    assert_eq!(
        w.take(0),
        vec![Effect::ViewEnded {
            peer: peer.clone(),
            id,
            end: ViewEnd::Refused,
        }]
    );
    // Without a session nothing further may be sent.
    assert!(w.nodes[0].view_input("k 42 down").is_none());
    assert!(w.nodes[0].view_stop().is_none());
}

#[test]
fn a_granted_device_streams_frames_and_takes_input_end_to_end() {
    let mut w = pair("view-e2e");
    grant(&mut w, 1, 0);
    let peer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&peer).unwrap();
    w.run(0, effects);
    // The person on the source is asked.
    assert_eq!(
        w.take(1),
        vec![Effect::ViewIncoming {
            peer: peer.clone(),
            id
        }]
    );

    // The person accepts; both sides see the stream begin.
    let effects = w.nodes[1].view_accept(&peer, id, 8, 8);
    w.run(1, effects);
    let started = Effect::ViewStarted { peer: peer.clone(), id };
    assert!(w.take(0).contains(&started));
    assert!(w.take(1).contains(&started));

    // A damaged rectangle arrives, decoded to its exact raw bytes.
    let raw = vec![0x11u8; 8 * 8 * 4];
    let effects = w.nodes[1].view_frame([0, 0, 8, 8], &raw);
    w.run(1, effects);
    assert_eq!(
        w.take(0),
        vec![Effect::ViewFrame {
            peer: peer.clone(),
            id,
            region: [0, 0, 8, 8],
            data: raw.clone(),
        }]
    );
    // The viewer's ack came back: the source admits the next frame again.
    assert_eq!(w.nodes[1].view_frame([0, 0, 8, 8], &raw).len(), 1);

    // The viewer's input reaches the source.
    let effects = w.nodes[0].view_input("k 42 down").unwrap();
    w.run(0, effects);
    assert_eq!(
        w.take(1),
        vec![Effect::ViewInput {
            peer: peer.clone(),
            id,
            input: "k 42 down".into(),
        }]
    );

    // The viewer stops; both sides are done.
    let effects = w.nodes[0].view_stop().unwrap();
    w.run(0, effects);
    assert_eq!(
        w.take(0),
        vec![Effect::ViewEnded {
            peer: peer.clone(),
            id,
            end: ViewEnd::Stopped,
        }]
    );
    assert_eq!(
        w.take(1),
        vec![Effect::ViewEnded {
            peer: peer.clone(),
            id,
            end: ViewEnd::Stopped,
        }]
    );
}

#[test]
fn the_source_ends_the_session_and_the_viewer_stops() {
    let mut w = pair("view-stop");
    grant(&mut w, 1, 0);
    let peer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&peer).unwrap();
    w.run(0, effects);
    let _ = w.take(0);
    let _ = w.take(1);
    let effects = w.nodes[1].view_accept(&peer, id, 16, 16);
    w.run(1, effects);
    let _ = w.take(0);
    let _ = w.take(1);

    // The source's person presses Stop: the stream ends for both.
    let effects = w.nodes[1].view_end();
    w.run(1, effects);
    let ended = Effect::ViewEnded {
        peer: peer.clone(),
        id,
        end: ViewEnd::Stopped,
    };
    assert!(w.take(0).contains(&ended));
    assert!(w.take(1).contains(&ended));
}

#[test]
fn a_slow_link_drops_frames_instead_of_buffering() {
    let mut w = pair("view-flow");
    grant(&mut w, 1, 0);
    let peer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&peer).unwrap();
    w.run(0, effects);
    let _ = w.take(0);
    let _ = w.take(1);
    let effects = w.nodes[1].view_accept(&peer, id, 16, 16);
    w.run(1, effects);
    let _ = w.take(0);
    let _ = w.take(1);

    let raw = vec![0u8; 16 * 16 * 4];
    // Without any ack, the source admits exactly WINDOW frames ...
    for _ in 0..WINDOW {
        assert_eq!(
            w.nodes[1].view_frame([0, 0, 16, 16], &raw).len(),
            1,
            "a frame under the window is sent"
        );
    }
    // ... and drops every following one (no unbounded buffering).
    assert!(w.nodes[1].view_frame([0, 0, 16, 16], &raw).is_empty());
    assert!(w.nodes[1].view_frame([0, 0, 16, 16], &raw).is_empty());
}

#[test]
fn a_lost_link_ends_the_session_as_lost() {
    let mut w = pair("view-lost");
    grant(&mut w, 1, 0);
    let peer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&peer).unwrap();
    w.run(0, effects);
    let _ = w.take(0);
    let _ = w.take(1);
    let effects = w.nodes[1].view_accept(&peer, id, 16, 16);
    w.run(1, effects);
    let _ = w.take(0);
    let _ = w.take(1);

    // The channel drops: the viewer reports the session lost.
    let effects = w.nodes[0].link_down(&peer);
    w.run(0, effects);
    assert_eq!(
        w.take(0),
        vec![Effect::ViewEnded {
            peer: peer.clone(),
            id,
            end: ViewEnd::Lost,
        }]
    );
}