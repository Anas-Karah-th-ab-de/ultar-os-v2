//! Pairing and what an unpaired device may do (nothing).
mod common;
use common::World;
use ultar_bus::messages::mesh::{Activity, DeviceKind, Right, SendOutcome};
use ultar_mesh::node::Effect;
use ultar_mesh::trust::MAX_TRIES;
use ultar_mesh::wire::Frame;

const PC: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &["editor"]);
const PHONE: (DeviceKind, &[&str]) = (DeviceKind::Phone, &["editor"]);

#[test]
fn the_right_code_pairs_both_devices() {
    let mut w = World::new("pair-ok", &[PHONE, PC]);
    w.link(0, 1);
    assert!(!w.paired(0, 1) && !w.paired(1, 0));
    w.pair(0, 1, None);
    assert!(w.paired(0, 1) && w.paired(1, 0));
    let peer = &w.nodes[0].state().peers[0];
    assert_eq!(peer.name, "dev1");
    assert_eq!(peer.kind, DeviceKind::Desktop);
    assert!(peer.rights.contains(&Right::Handoff) && !peer.rights.contains(&Right::Control));
}

#[test]
fn a_wrong_code_pairs_nobody_and_five_wrong_codes_end_the_code() {
    let mut w = World::new("pair-bad", &[PHONE, PC]);
    w.link(0, 1);
    let code = w.nodes[1].pair_begin(w.now).code;
    let wrong = if code == "000000" { "000001" } else { "000000" };
    let ib = w.id(1);
    for _ in 0..MAX_TRIES {
        let (_, effects) = w.nodes[0].pair_join(&ib, wrong);
        w.run(0, effects);
        assert!(!w.paired(0, 1) && !w.paired(1, 0));
    }
    // The right code comes too late: the attempts are used up.
    let (_, effects) = w.nodes[0].pair_join(&ib, &code);
    w.run(0, effects);
    assert!(!w.paired(0, 1) && !w.paired(1, 0));
}

#[test]
fn without_a_shown_code_nobody_can_pair() {
    let mut w = World::new("pair-none", &[PHONE, PC]);
    w.link(0, 1);
    let ib = w.id(1);
    let (_, effects) = w.nodes[0].pair_join(&ib, "123456");
    w.run(0, effects);
    assert!(!w.paired(0, 1) && !w.paired(1, 0));
}

#[test]
fn an_unpaired_device_gets_no_offer_no_clipboard_and_no_job() {
    let mut w = World::new("pair-stranger", &[PHONE, PC]);
    w.link(0, 1);
    w.take(1);
    let stranger = w.id(0);
    let frames = vec![
        Frame::Offer {
            id: 1,
            title: "x".into(),
            app: "editor".into(),
            stage: "state".into(),
            uri: Some("https://example.org".into()),
            file: None,
        },
        Frame::Clip {
            seq: 1,
            text: "secret".into(),
        },
        Frame::Job {
            id: "j".into(),
            program: "true".into(),
            args: vec![],
        },
    ];
    for frame in frames {
        assert_eq!(
            w.nodes[1].frame(&stranger, frame, w.now),
            Vec::<Effect>::new()
        );
    }
    // And this device does not offer to a stranger either.
    w.nodes[0].set_activity(Some(Activity {
        app: "editor".into(),
        title: "t".into(),
        uri: Some("https://example.org".into()),
        file: None,
    }));
    assert_eq!(w.nodes[0].send(None, w.now).0, SendOutcome::NoTarget);
}

#[test]
fn a_link_with_a_key_that_does_not_match_the_id_is_ignored() {
    let mut w = World::new("pair-id", &[PHONE, PC]);
    let ib = w.id(1);
    assert!(w.nodes[0].link_up(&ib, &[0xEE; 32]).is_empty());
    assert!(w.nodes[0].state().peers.is_empty());
}

#[test]
fn unpairing_forgets_the_device_and_switching_off_drops_every_link() {
    let mut w = World::new("pair-off", &[PHONE, PC]);
    w.link(0, 1);
    w.pair(0, 1, None);
    let ib = w.id(1);
    assert!(w.nodes[0].unpair(&ib).0);
    assert!(!w.paired(0, 1));
    assert!(!w.nodes[0].unpair(&ib).0);
    w.nodes[1].set_enabled(false);
    let state = w.nodes[1].state();
    assert!(!state.enabled);
    assert!(state.peers.iter().all(|p| !p.reachable));
    let ia = w.id(0);
    assert!(w.nodes[1].link_up(&ia, &common::key(0)).is_empty());
}
