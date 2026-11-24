//! Lending the language model between paired devices.
mod common;
use common::World;
use ultar_bus::messages::llm::ModelClass;
use ultar_bus::messages::mesh::{Capability, DeviceKind, Right};
use ultar_mesh::node::Effect;
use ultar_mesh::trust::DEFAULT_RIGHTS;

const PHONE: (DeviceKind, &[&str]) = (DeviceKind::Phone, &[]);
const PC: (DeviceKind, &[&str]) = (DeviceKind::Desktop, &[]);
const SERVER: (DeviceKind, &[&str]) = (DeviceKind::Server, &[]);

fn meshed(tag: &str) -> World {
    let mut w = World::new(tag, &[PHONE, PC, SERVER]);
    for other in 1..3 {
        w.link(0, other);
        w.pair(0, other, None);
    }
    w
}

fn lend(w: &mut World, lender: usize, to: usize) {
    let id = w.id(to);
    let mut rights = DEFAULT_RIGHTS.to_vec();
    rights.push(Right::Model);
    assert!(w.nodes[lender].set_rights(&id, &rights).0);
}

fn announce(w: &mut World, i: usize, class: Option<ModelClass>) {
    let effects = w.nodes[i].set_model(class);
    w.run(i, effects);
}

#[test]
fn a_device_learns_which_paired_devices_have_a_model_and_prefers_the_best() {
    let mut w = meshed("model-list");
    assert!(w.nodes[0].lenders().is_empty());
    announce(&mut w, 1, Some(ModelClass::Small));
    announce(&mut w, 2, Some(ModelClass::Large));
    let lenders = w.nodes[0].lenders();
    assert_eq!(lenders.len(), 2);
    assert_eq!(
        (lenders[0].class, lenders[0].device.clone()),
        (ModelClass::Large, w.id(2))
    );
    // The model goes away (battery, heat): the phone is told.
    announce(&mut w, 2, None);
    assert_eq!(w.nodes[0].lenders().len(), 1);
    let state = w.nodes[0].state();
    let pc = state.peers.iter().find(|p| p.id == w.id(1)).unwrap();
    assert!(pc.capabilities.contains(&Capability::ModelSmall));
}

#[test]
fn the_model_answers_only_a_device_that_was_granted_it() {
    let mut w = meshed("model-right");
    announce(&mut w, 1, Some(ModelClass::Medium));
    for i in 0..3 {
        w.take(i);
    }
    // Not granted: the answer is "no" and the model is never asked.
    let (id, to, effects) = w.nodes[0].think(None, "rules", "hello", 64).unwrap();
    assert_eq!(to, w.id(1));
    w.run(0, effects);
    assert!(w.take(1).is_empty());
    assert_eq!(
        w.take(0),
        vec![Effect::Thought {
            id,
            peer: w.id(1),
            ok: false,
            text: String::new()
        }]
    );
    // Granted: the lending device lets its model think and sends the answer back.
    lend(&mut w, 1, 0);
    w.take(1);
    let (id, _, effects) = w.nodes[0].think(None, "rules", "hello", 64).unwrap();
    w.run(0, effects);
    let asked = w.take(1);
    let phone = w.id(0);
    let Effect::Think {
        peer,
        id: theirs,
        prompt,
        ..
    } = &asked[0]
    else {
        panic!("{asked:?}");
    };
    assert_eq!((peer.as_str(), prompt.as_str()), (phone.as_str(), "hello"));
    let effects = w.nodes[1].thought(&phone, *theirs, true, "hi".into());
    w.run(1, effects);
    assert_eq!(
        w.take(0),
        vec![Effect::Thought {
            id,
            peer: w.id(1),
            ok: true,
            text: "hi".into()
        }]
    );
}

#[test]
fn without_a_lender_or_with_too_much_text_nothing_is_sent() {
    let mut w = meshed("model-none");
    assert!(w.nodes[0].think(None, "s", "p", 8).is_none());
    announce(&mut w, 1, Some(ModelClass::Small));
    let other = w.id(2);
    assert!(
        w.nodes[0].think(Some(&other), "s", "p", 8).is_none(),
        "the server has no model"
    );
    let long = "x".repeat(ultar_bus::messages::llm::MAX_PROMPT);
    assert!(w.nodes[0].think(None, "s", &long, 8).is_none());
}
