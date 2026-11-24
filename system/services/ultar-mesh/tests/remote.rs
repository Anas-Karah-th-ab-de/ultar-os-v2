//! Acting on another device: the helper of a paired device is asked to do something there.
mod common;
use common::World;
use ultar_bus::messages::agent::{Step, StepOutcome};
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_mesh::node::Effect;
use ultar_mesh::trust::DEFAULT_RIGHTS;

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

fn ids(w: &World) -> (String, String) {
    (w.id(0), w.id(1))
}

fn allow(w: &mut World, device: usize, to: usize) {
    let id = w.id(to);
    let mut rights = DEFAULT_RIGHTS.to_vec();
    rights.push(Right::Control);
    assert!(w.nodes[device].set_rights(&id, &rights).0);
}

#[test]
fn a_device_that_did_not_allow_it_never_asks_its_helper() {
    let mut w = pair("remote-refused");
    let (_, pc) = ids(&w);
    let (id, effects) = w.nodes[0].act(&pc, "mute the sound").unwrap();
    w.run(0, effects);
    assert!(w.take(1).is_empty());
    let refused = Effect::Acted {
        id,
        peer: w.id(1),
        ok: false,
        steps: Vec::new(),
    };
    assert_eq!(w.take(0), vec![refused]);
    // An unknown device, an empty or an overlong request are not sent at all.
    assert!(w.nodes[0].act("nobody", "mute").is_none());
    assert!(w.nodes[0].act(&pc, "  ").is_none());
    assert!(w.nodes[0].act(&pc, &"x".repeat(5000)).is_none());
}

#[test]
fn the_helper_of_the_other_device_acts_and_reports_its_steps() {
    let mut w = pair("remote-acts");
    let (phone, pc) = ids(&w);
    allow(&mut w, 1, 0);
    let (id, effects) = w.nodes[0].act(&pc, "mute the sound").unwrap();
    w.run(0, effects);
    let asked = Effect::Act {
        peer: w.id(0),
        id,
        text: "mute the sound".into(),
    };
    assert_eq!(w.take(1), vec![asked]);
    // While the computer carries out a request it passes none on and takes no second.
    assert!(w.nodes[1].act(&phone, "and you").is_none());
    let steps = vec![Step {
        tool: "mute".into(),
        outcome: StepOutcome::Done,
    }];
    let effects = w.nodes[1].acted(&phone, id, true, steps.clone());
    w.run(1, effects);
    let done = Effect::Acted {
        id,
        peer: w.id(1),
        ok: true,
        steps,
    };
    assert_eq!(w.take(0), vec![done]);
    assert!(w.nodes[1].act(&phone, "now you").is_some());
}
