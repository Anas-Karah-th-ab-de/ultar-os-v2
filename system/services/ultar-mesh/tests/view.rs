//! Remote view: a paired device with the right streams its screen and takes the input of
//! another device; two nodes wired together in memory, exactly as the network would.
//! Node 0 is the viewer, node 1 the source.
mod common;
use common::World;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_mesh::node::Effect;
use ultar_mesh::trust::DEFAULT_RIGHTS;
use ultar_mesh::view::{ViewEnd, MAX_INPUT, WINDOW};

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

/// A running session: viewer 0 watches source 1 (8x8 picture); the effects are cleared.
fn running(tag: &str) -> (World, u64) {
    let mut w = pair(tag);
    grant(&mut w, 1, 0);
    let source = w.id(1);
    let viewer = w.id(0);
    let (id, effects) = w.nodes[0].view_start(&source).unwrap();
    w.run(0, effects);
    let effects = w.nodes[1].view_accept(&viewer, id, 64, 48);
    w.run(1, effects);
    w.take(0);
    w.take(1);
    (w, id)
}

#[test]
fn without_the_control_right_the_source_refuses_and_never_asks_its_person() {
    let mut w = pair("view-refused");
    let source = w.id(1);
    let (id, effects) = w.nodes[0].view_start(&source).unwrap();
    w.run(0, effects);
    assert!(w.take(1).is_empty(), "the source's person is not asked");
    let refused = Effect::ViewEnded {
        peer: source,
        id,
        end: ViewEnd::Refused,
    };
    assert_eq!(w.take(0), vec![refused]);
    // Without a session nothing further may be sent.
    assert!(w.nodes[0].view_input("k 42 down").is_none());
    assert!(w.nodes[0].view_stop().is_none());
    // An unknown device cannot be asked at all.
    assert!(w.nodes[0].view_start("nobody").is_none());
}

#[test]
fn a_granted_device_streams_frames_and_takes_input_end_to_end() {
    let mut w = pair("view-e2e");
    grant(&mut w, 1, 0);
    let (viewer, source) = (w.id(0), w.id(1));
    let (id, effects) = w.nodes[0].view_start(&source).unwrap();
    w.run(0, effects);
    // The person on the source is asked.
    let incoming = Effect::ViewIncoming {
        peer: viewer.clone(),
        id,
    };
    assert_eq!(w.take(1), vec![incoming]);
    // Input before the source accepted goes nowhere.
    assert!(w.nodes[0].view_input("m 1 1").is_none());

    // The person accepts; both sides see the stream begin with the picture size.
    let effects = w.nodes[1].view_accept(&viewer, id, 64, 48);
    w.run(1, effects);
    let at = |peer: &str| Effect::ViewStarted {
        peer: peer.to_owned(),
        id,
        width: 64,
        height: 48,
    };
    assert!(w.take(0).contains(&at(&source)));
    assert!(w.take(1).contains(&at(&viewer)));

    // A damaged rectangle arrives, decoded to its exact raw bytes.
    let raw: Vec<u8> = (0..8 * 8 * 4).map(|i| (i * 7) as u8).collect();
    let effects = w.nodes[1].view_frame([4, 2, 8, 8], &raw);
    w.run(1, effects);
    let frame = Effect::ViewFrame {
        peer: source.clone(),
        id,
        region: [4, 2, 8, 8],
        data: raw.clone(),
    };
    assert_eq!(w.take(0), vec![frame]);
    // The viewer's ack came back: the window is open again.
    assert!(w.nodes[1].view_room());

    // The viewer's input reaches the source.
    let effects = w.nodes[0].view_input("k 42 down").unwrap();
    w.run(0, effects);
    let input = Effect::ViewInput {
        peer: viewer.clone(),
        id,
        input: "k 42 down".into(),
    };
    assert_eq!(w.take(1), vec![input]);
    assert!(w.nodes[0].view_input("").is_none());
    assert!(w.nodes[0].view_input(&"x".repeat(MAX_INPUT + 1)).is_none());
    let now = w.nodes[1].view_now();
    assert_eq!(now.watched_by.as_deref(), Some(viewer.as_str()));
    assert_eq!(
        w.nodes[0].view_now().watching.as_deref(),
        Some(source.as_str())
    );

    // The viewer stops; both sides are done.
    let effects = w.nodes[0].view_stop().unwrap();
    w.run(0, effects);
    let ended = |peer: &str| Effect::ViewEnded {
        peer: peer.to_owned(),
        id,
        end: ViewEnd::Stopped,
    };
    assert_eq!(w.take(0), vec![ended(&source)]);
    assert_eq!(w.take(1), vec![ended(&viewer)]);
    assert_eq!(w.nodes[1].view_now().watched_by, None);
}

#[test]
fn the_source_person_declines_and_the_viewer_hears_refused() {
    let mut w = pair("view-declined");
    grant(&mut w, 1, 0);
    let (viewer, source) = (w.id(0), w.id(1));
    let (id, effects) = w.nodes[0].view_start(&source).unwrap();
    w.run(0, effects);
    w.take(1);
    let effects = w.nodes[1].view_refuse(&viewer, id);
    w.run(1, effects);
    let refused = Effect::ViewEnded {
        peer: source,
        id,
        end: ViewEnd::Refused,
    };
    assert_eq!(w.take(0), vec![refused]);
    assert!(!w.nodes[1].view_room());
    // A second answer for the same session does nothing.
    assert!(w.nodes[1].view_refuse(&viewer, id).is_empty());
    assert!(w.nodes[1].view_accept(&viewer, id, 8, 8).is_empty());
}

#[test]
fn a_zero_or_absurd_picture_size_refuses() {
    for (tag, size) in [("zero", (0, 5)), ("huge", (9000, 10))] {
        let mut w = pair(&format!("view-size-{tag}"));
        grant(&mut w, 1, 0);
        let (viewer, source) = (w.id(0), w.id(1));
        let (id, effects) = w.nodes[0].view_start(&source).unwrap();
        w.run(0, effects);
        let effects = w.nodes[1].view_accept(&viewer, id, size.0, size.1);
        w.run(1, effects);
        let refused = Effect::ViewEnded {
            peer: source,
            id,
            end: ViewEnd::Refused,
        };
        assert!(w.take(0).contains(&refused), "{tag}");
    }
}

#[test]
fn the_source_ends_the_session_when_its_person_stops_or_the_screen_locks() {
    for locked in [false, true] {
        let (mut w, id) = running(if locked { "view-lock" } else { "view-stop" });
        let (viewer, source) = (w.id(0), w.id(1));
        let effects = if locked {
            w.nodes[1].view_locked()
        } else {
            w.nodes[1].view_end()
        };
        w.run(1, effects);
        let end = if locked {
            ViewEnd::Locked
        } else {
            ViewEnd::Stopped
        };
        assert_eq!(
            w.take(1),
            vec![Effect::ViewEnded {
                peer: viewer,
                id,
                end
            }]
        );
        let seen = Effect::ViewEnded {
            peer: source,
            id,
            end,
        };
        assert_eq!(w.take(0), vec![seen]);
        // Nothing streams any more.
        assert!(w.nodes[1].view_frame([0, 0, 1, 1], &[0; 4]).is_empty());
    }
}

#[test]
fn taking_the_control_right_back_or_unpairing_ends_a_running_session() {
    // The right taken back.
    let (mut w, id) = running("view-revoke");
    let (viewer, source) = (w.id(0), w.id(1));
    let (_, effects) = w.nodes[1].set_rights(&viewer, &DEFAULT_RIGHTS);
    let ended = Effect::ViewEnded {
        peer: viewer.clone(),
        id,
        end: ViewEnd::Stopped,
    };
    assert!(effects.contains(&ended));
    w.run(1, effects);
    let seen = Effect::ViewEnded {
        peer: source.clone(),
        id,
        end: ViewEnd::Stopped,
    };
    assert!(w.take(0).contains(&seen));
    assert_eq!(w.nodes[1].view_now().watched_by, None);
    // Unpairing the viewer ends it the same way.
    let (mut w, id) = running("view-unpair");
    let (ok, effects) = w.nodes[1].unpair(&viewer);
    assert!(ok);
    assert!(effects.contains(&Effect::ViewEnded {
        peer: viewer,
        id,
        end: ViewEnd::Stopped
    }));
    w.run(1, effects);
    assert!(w.take(0).contains(&Effect::ViewEnded {
        peer: source,
        id,
        end: ViewEnd::Stopped
    }));
}
#[test]
fn a_slow_link_drops_frames_instead_of_buffering() {
    let (mut w, _) = running("view-flow");
    let raw = vec![0u8; 16 * 16 * 4];
    // The viewer's acks never come back (cut): the source sends exactly WINDOW frames ...
    w.cut = Some((0, 1));
    for _ in 0..WINDOW {
        assert!(w.nodes[1].view_room());
        let effects = w.nodes[1].view_frame([0, 0, 16, 16], &raw);
        assert_eq!(effects.len(), 1, "a frame under the window is sent");
        w.run(1, effects);
    }
    // ... and drops every following one (no unbounded buffering).
    assert!(!w.nodes[1].view_room());
    assert!(w.nodes[1].view_frame([0, 0, 16, 16], &raw).is_empty());
    assert!(w.nodes[1].view_frame([0, 0, 16, 16], &raw).is_empty());
    // The link recovers: the viewer's ack opens the window again.
    w.cut = None;
    assert!(
        w.nodes[1].view_frame([0, 0, 16, 16], &raw).is_empty(),
        "closed until an ack"
    );
    let (viewer, id) = (w.id(0), 1);
    let ack = ultar_mesh::wire::Frame::ViewAck { id, seq: WINDOW };
    w.nodes[1].frame(&viewer, ack, w.now);
    assert!(w.nodes[1].view_room());
    assert_eq!(w.nodes[1].view_frame([0, 0, 16, 16], &raw).len(), 1);
}

#[test]
fn frames_that_do_not_fit_are_never_sent_and_a_bad_ack_changes_nothing() {
    let (mut w, id) = running("view-bad");
    let (viewer, source) = (w.id(0), w.id(1));
    // Too large for one frame, wrong byte count, empty.
    assert!(w.nodes[1]
        .view_frame([0, 0, 200, 200], &vec![0; 200 * 200 * 4])
        .is_empty());
    assert!(w.nodes[1].view_frame([0, 0, 4, 4], &[0; 10]).is_empty());
    assert!(w.nodes[1].view_frame([0, 0, 0, 4], &[]).is_empty());
    // A rectangle outside the picture is dropped by the viewer (64 x 48).
    let raw = vec![1u8; 8 * 8 * 4];
    let effects = w.nodes[1].view_frame([60, 0, 8, 8], &raw);
    w.run(1, effects);
    assert!(w.take(0).is_empty());
    // An ack for frames never sent cannot open or break the window.
    let frame = ultar_mesh::wire::Frame::ViewAck { id, seq: u32::MAX };
    assert!(w.nodes[1].frame(&viewer, frame, w.now).is_empty());
    // Someone else's frames are ignored: wrong session id.
    let stray = ultar_mesh::wire::Frame::ViewStop {
        id: id + 99,
        locked: false,
    };
    assert!(w.nodes[0].frame(&source, stray, w.now).is_empty());
}

#[test]
fn a_lost_link_ends_the_session_as_lost_on_both_sides() {
    let (mut w, id) = running("view-lost");
    let (viewer, source) = (w.id(0), w.id(1));
    let effects = w.nodes[0].link_down(&source);
    w.run(0, effects);
    assert!(w.take(0).contains(&Effect::ViewEnded {
        peer: source,
        id,
        end: ViewEnd::Lost
    }));
    let effects = w.nodes[1].link_down(&viewer);
    w.run(1, effects);
    assert!(w.take(1).contains(&Effect::ViewEnded {
        peer: viewer,
        id,
        end: ViewEnd::Lost
    }));
}

#[test]
fn only_one_session_at_a_time() {
    let (mut w, _) = running("view-one");
    let source = w.id(1);
    assert!(w.nodes[0].view_start(&source).is_none(), "already viewing");
    // A second viewer is refused while the source serves one.
    let mut w3 = World::new("view-three", &[PHONE, PC, PHONE]);
    w3.link(0, 1);
    w3.link(2, 1);
    w3.pair(0, 1, None);
    w3.pair(2, 1, None);
    grant(&mut w3, 1, 0);
    grant(&mut w3, 1, 2);
    w3.take(0);
    w3.take(1);
    let (src, first) = (w3.id(1), w3.id(0));
    let (id, effects) = w3.nodes[0].view_start(&src).unwrap();
    w3.run(0, effects);
    let effects = w3.nodes[1].view_accept(&first, id, 8, 8);
    w3.run(1, effects);
    w3.take(2);
    let (id2, effects) = w3.nodes[2].view_start(&src).unwrap();
    w3.run(2, effects);
    let busy = Effect::ViewEnded {
        peer: src,
        id: id2,
        end: ViewEnd::Refused,
    };
    assert_eq!(w3.take(2), vec![busy]);
}
