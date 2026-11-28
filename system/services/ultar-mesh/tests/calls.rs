//! Calls, sound and notifications over the mesh, wired as two nodes exactly as two
//! devices would be: a ring offered by the phone, answered on the desk, voice both ways
//! in order, hung up from each side; a notification mirrored (a confidential one never).

mod common;

use common::World;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_bus::messages::mesh_calls::{
    CallAudio, CallIncoming, CallPhase, CallProgress, NoteRemote,
};
use ultar_bus::messages::notify::Urgency;
use ultar_mesh::audio_link;
use ultar_mesh::node::Effect;

const BASE: [Right; 3] = [Right::Handoff, Right::Files, Right::Clipboard];

fn rights(with: Right) -> Vec<Right> {
    let mut list = BASE.to_vec();
    list.push(with);
    list
}

fn pcm(mark: u8) -> Vec<u8> {
    audio_link::encode_frame(&vec![i16::from(mark); audio_link::FRAME_SAMPLES])
}

fn progress(world: &mut World, at: usize) -> Vec<CallProgress> {
    take(world, at, |e| match e {
        Effect::CallProgress(p) => Some(p),
        _ => None,
    })
}

fn audio(world: &mut World, at: usize) -> Vec<CallAudio> {
    take(world, at, |e| match e {
        Effect::CallAudio(a) => Some(a),
        _ => None,
    })
}

fn incoming(world: &mut World, at: usize) -> Vec<CallIncoming> {
    take(world, at, |e| match e {
        Effect::CallIncoming(i) => Some(i),
        _ => None,
    })
}

fn take<T>(world: &mut World, at: usize, f: impl Fn(Effect) -> Option<T>) -> Vec<T> {
    world.take(at).into_iter().filter_map(f).collect()
}

/// Links and pairs the two nodes, grants `Right::Call` both ways and clears the noise.
fn talk(world: &mut World) -> (String, String) {
    world.link(0, 1);
    world.pair(0, 1, None);
    let (phone, desk) = (world.id(0), world.id(1));
    let (ok, a) = world.nodes[0].set_rights(&desk, &rights(Right::Call));
    assert!(ok);
    world.run(0, a);
    let (ok, b) = world.nodes[1].set_rights(&phone, &rights(Right::Call));
    assert!(ok);
    world.run(1, b);
    world.take(0);
    world.take(1);
    (phone, desk)
}

/// Calls `world.nodes[at].method(...)` first, then runs its effects on that node.
macro_rules! run {
    ($world:expr, $at:expr, $method:ident($($arg:expr),*)) => {{
        let effects = $world.nodes[$at].$method($($arg),*);
        $world.run($at, effects);
    }};
}

/// Offers a call from the phone and lets the desk accept it; returns their ids and the
/// call's id. Both sides see `Accepted`.
fn offer_accepted(world: &mut World, caller: &str, video: bool) -> (String, String, u64) {
    let (phone, desk) = talk(world);
    let (id, effects) = world.nodes[0].call_offer(&desk, caller, video);
    assert!(id != 0);
    world.run(0, effects);
    let got = incoming(world, 1);
    assert_eq!(
        (got[0].from.as_str(), got[0].caller.as_str()),
        (phone.as_str(), caller)
    );
    run!(world, 1, call_answer(id, true));
    assert!(progress(world, 0)
        .iter()
        .any(|p| p.phase == CallPhase::Accepted));
    assert!(progress(world, 1)
        .iter()
        .any(|p| p.phase == CallPhase::Accepted));
    (phone, desk, id)
}

/// Voice both ways, three frames each, in order.
fn voice_both_ways(world: &mut World, phone: &str, desk: &str, id: u64) {
    for seq in 0..3u64 {
        run!(world, 0, call_audio(desk, id, seq, &pcm(seq as u8)));
    }
    let to_desk = audio(world, 1);
    assert_eq!(to_desk.len(), 3);
    for a in &to_desk {
        let samples =
            audio_link::decode_frame(&ultar_mesh::device::decode(&a.pcm).unwrap()).unwrap();
        assert_eq!(samples[0], a.seq as i16);
    }
    for seq in 0..3u64 {
        run!(world, 1, call_audio(phone, id, seq, &pcm(0x10 + seq as u8)));
    }
    let to_phone = audio(world, 0);
    assert_eq!(to_phone.len(), 3);
    assert_eq!(to_phone[2].seq, 2);
}

#[test]
fn no_right_no_ring() {
    let mut world = World::new(
        "calls-no-right",
        &[(DeviceKind::Phone, &[]), (DeviceKind::Desktop, &[])],
    );
    world.link(0, 1);
    world.pair(0, 1, None);
    world.take(0);
    world.take(1);
    let desk = world.id(1);
    // The desk is paired and can take calls, the offer goes out, but no right: no ring.
    let (id, effects) = world.nodes[0].call_offer(&desk, "Anas", false);
    assert!(id != 0);
    world.run(0, effects);
    assert!(
        world.take(1).is_empty(),
        "nothing may ring without Right::Call"
    );
}

#[test]
fn ring_answer_talk_and_hang_up_on_the_desk() {
    let mut world = World::new(
        "calls-talk",
        &[(DeviceKind::Phone, &[]), (DeviceKind::Desktop, &[])],
    );
    let (phone, desk, id) = offer_accepted(&mut world, "+963 000", false);
    voice_both_ways(&mut world, &phone, &desk, id);
    run!(world, 1, call_end(id));
    assert!(progress(&mut world, 1)
        .iter()
        .any(|p| p.phase == CallPhase::Ended));
    assert!(progress(&mut world, 0)
        .iter()
        .any(|p| p.phase == CallPhase::Ended));
}

#[test]
fn the_phone_can_hang_up_too() {
    let mut world = World::new(
        "calls-hangup",
        &[(DeviceKind::Phone, &[]), (DeviceKind::Desktop, &[])],
    );
    let (_phone, desk) = talk(&mut world);
    let (id, effects) = world.nodes[0].call_offer(&desk, "Anas", true);
    world.run(0, effects);
    let _ = incoming(&mut world, 1);
    let _ = progress(&mut world, 0);
    run!(world, 1, call_answer(id, true));
    let _ = progress(&mut world, 0);
    let _ = progress(&mut world, 1);
    run!(world, 0, call_end(id));
    assert!(progress(&mut world, 1)
        .iter()
        .any(|p| p.phase == CallPhase::Ended));
}

#[test]
fn confidential_notes_never_leave_the_device() {
    let mut world = World::new(
        "calls-note",
        &[(DeviceKind::Phone, &[]), (DeviceKind::Desktop, &[])],
    );
    let (_phone, _desk) = talk(&mut world);
    let sent = world.nodes[0].note_mirror("OTP", "123456", "bank", Urgency::High, true, false);
    assert!(sent.is_empty());
    world.run(0, sent);
    assert!(
        world.take(1).is_empty(),
        "a confidential notification never leaves"
    );
    let sent = world.nodes[0].note_mirror("SMS", "مرحبا", "messages", Urgency::Normal, false, true);
    world.run(0, sent);
    let notes: Vec<NoteRemote> = take(&mut world, 1, |e| match e {
        Effect::Note(n) => Some(n),
        _ => None,
    });
    assert_eq!(
        (notes[0].title.as_str(), notes[0].text.as_str()),
        ("SMS", "مرحبا")
    );
    assert!(notes[0].hide_on_locked);
}

#[test]
fn a_frame_of_the_wrong_size_is_refused() {
    let mut world = World::new(
        "calls-size",
        &[(DeviceKind::Phone, &[]), (DeviceKind::Desktop, &[])],
    );
    let (_phone, desk) = talk(&mut world);
    let (id, effects) = world.nodes[0].call_offer(&desk, "Anas", false);
    world.run(0, effects);
    let _ = incoming(&mut world, 1);
    let _ = progress(&mut world, 0);
    run!(world, 1, call_answer(id, true));
    let _ = progress(&mut world, 0);
    let _ = progress(&mut world, 1);
    let bad = world.nodes[0].call_audio(&desk, id, 0, &[0u8; audio_link::FRAME_BYTES - 1]);
    assert!(bad.is_empty(), "a frame of the wrong size is refused");
    let good = world.nodes[0].call_audio(&desk, id, 0, &pcm(7));
    assert!(!good.is_empty());
}
