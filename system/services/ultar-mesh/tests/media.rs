//! "Play there" over the mesh, wired as two nodes exactly as two devices would be: the
//! sound of a session goes to the paired device in order, pause, volume and stop work
//! from both ends, and nothing plays without the right "Anrufe und Ton".

mod common;

use common::World;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_bus::messages::mesh_media::{
    MediaControl, MediaCtl, MediaIncoming, MediaPcmIn, MediaPhase, MediaProgress,
};
use ultar_mesh::audio_link;
use ultar_mesh::node::Effect;

fn take<T>(world: &mut World, at: usize, f: impl Fn(Effect) -> Option<T>) -> Vec<T> {
    world.take(at).into_iter().filter_map(f).collect()
}

fn incoming(world: &mut World, at: usize) -> Vec<MediaIncoming> {
    take(world, at, |e| match e {
        Effect::MediaIncoming(i) => Some(i),
        _ => None,
    })
}

fn pcm_in(world: &mut World, at: usize) -> Vec<MediaPcmIn> {
    take(world, at, |e| match e {
        Effect::MediaPcm(p) => Some(p),
        _ => None,
    })
}

fn controls(world: &mut World, at: usize) -> Vec<MediaControl> {
    take(world, at, |e| match e {
        Effect::MediaControl(c) => Some(c),
        _ => None,
    })
}

fn ended(world: &mut World, at: usize) -> Vec<MediaProgress> {
    take(world, at, |e| match e {
        Effect::MediaProgress(p) => Some(p),
        _ => None,
    })
}

fn frame(mark: i16) -> Vec<u8> {
    audio_link::encode_frame(&vec![mark; audio_link::FRAME_SAMPLES])
}

/// Pairs the two nodes; `grant` gives the right "Anrufe und Ton" both ways.
fn pair(world: &mut World, grant: bool) -> (String, String) {
    world.link(0, 1);
    world.pair(0, 1, None);
    let (source, sink) = (world.id(0), world.id(1));
    if grant {
        let mut rights = vec![Right::Handoff, Right::Files, Right::Clipboard];
        rights.push(Right::Call);
        let (ok, a) = world.nodes[0].set_rights(&sink, &rights);
        assert!(ok);
        world.run(0, a);
        let (ok, b) = world.nodes[1].set_rights(&source, &rights);
        assert!(ok);
        world.run(1, b);
    }
    world.take(0);
    world.take(1);
    (source, sink)
}

macro_rules! run {
    ($world:expr, $at:expr, $effects:expr) => {{
        let effects = $effects;
        $world.run($at, effects);
    }};
}

fn two(tag: &str) -> World {
    World::new(
        tag,
        &[(DeviceKind::Desktop, &[]), (DeviceKind::Desktop, &[])],
    )
}

#[test]
fn sound_goes_there_in_order_and_both_ends_control_it() {
    let mut world = two("media-flow");
    let (source, sink) = pair(&mut world, true);
    let (id, effects) = world.nodes[0].media_open(&sink, "فيروز", "x");
    assert!(id != 0);
    world.run(0, effects);
    let got = incoming(&mut world, 1);
    assert_eq!(
        (got[0].from.as_str(), got[0].title.as_str(), got[0].id),
        (source.as_str(), "فيروز", id)
    );
    for seq in 0..4u64 {
        run!(
            world,
            0,
            world.nodes[0].media_pcm(&sink, id, seq, &frame(seq as i16))
        );
    }
    let heard = pcm_in(&mut world, 1);
    assert_eq!(
        heard.iter().map(|p| p.seq).collect::<Vec<_>>(),
        [0, 1, 2, 3]
    );
    let samples = audio_link::decode_frame(&ultar_mesh::device::decode(&heard[2].pcm).unwrap());
    assert_eq!(samples.unwrap()[0], 2);
    // The source pauses and sets the volume of the sink.
    run!(
        world,
        0,
        world.nodes[0].media_ctl(&sink, id, MediaCtl::Pause)
    );
    run!(
        world,
        0,
        world.nodes[0].media_ctl(&sink, id, MediaCtl::Volume(35))
    );
    let at_sink = controls(&mut world, 1);
    assert_eq!(
        at_sink.iter().map(|c| c.ctl).collect::<Vec<_>>(),
        [MediaCtl::Pause, MediaCtl::Volume(35)]
    );
    // The sink resumes from its side: the source hears it.
    run!(
        world,
        1,
        world.nodes[1].media_ctl(&source, id, MediaCtl::Resume)
    );
    let at_source = controls(&mut world, 0);
    assert_eq!(at_source[0].ctl, MediaCtl::Resume);
    assert_eq!(at_source[0].from, sink);
}

#[test]
fn stop_from_either_end_ends_both() {
    for stopper in [0usize, 1] {
        let mut world = two(&format!("media-stop-{stopper}"));
        let (source, sink) = pair(&mut world, true);
        let (id, effects) = world.nodes[0].media_open(&sink, "t", "a");
        world.run(0, effects);
        let _ = incoming(&mut world, 1);
        let other = if stopper == 0 { &sink } else { &source };
        run!(
            world,
            stopper,
            world.nodes[stopper].media_ctl(other, id, MediaCtl::Stop)
        );
        assert!(ended(&mut world, stopper)
            .iter()
            .any(|p| p.phase == MediaPhase::Ended));
        assert!(ended(&mut world, 1 - stopper)
            .iter()
            .any(|p| p.phase == MediaPhase::Ended));
        // Nothing flows after the end.
        let late = world.nodes[0].media_pcm(&sink, id, 9, &frame(1));
        assert!(late.is_empty());
    }
}

#[test]
fn no_right_nothing_plays() {
    let mut world = two("media-no-right");
    let (_source, sink) = pair(&mut world, false);
    let (id, effects) = world.nodes[0].media_open(&sink, "t", "a");
    assert!(id != 0, "the offer goes out, the other side stays silent");
    world.run(0, effects);
    assert!(
        incoming(&mut world, 1).is_empty(),
        "no right: nothing plays"
    );
    run!(world, 0, world.nodes[0].media_pcm(&sink, id, 0, &frame(1)));
    assert!(pcm_in(&mut world, 1).is_empty());
}

#[test]
fn wrong_sizes_and_unknown_sessions_are_refused() {
    let mut world = two("media-limits");
    let (_source, sink) = pair(&mut world, true);
    let (id, effects) = world.nodes[0].media_open(&sink, "t", "a");
    world.run(0, effects);
    let _ = incoming(&mut world, 1);
    assert!(world.nodes[0]
        .media_pcm(&sink, id, 0, &[0u8; audio_link::FRAME_BYTES + 2])
        .is_empty());
    assert!(world.nodes[0]
        .media_pcm(&sink, id + 1, 0, &frame(1))
        .is_empty());
    assert!(world.nodes[0]
        .media_ctl(&sink, id + 1, MediaCtl::Pause)
        .is_empty());
    // A device that is not paired is no target.
    assert_eq!(world.nodes[0].media_open("nobody", "t", "a").0, 0);
}

#[test]
fn a_lost_link_ends_the_session_and_ids_of_two_sources_never_meet() {
    let mut world = World::new(
        "media-down",
        &[
            (DeviceKind::Desktop, &[]),
            (DeviceKind::Desktop, &[]),
            (DeviceKind::Phone, &[]),
        ],
    );
    world.link(0, 1);
    world.pair(0, 1, None);
    let (zero, one) = (world.id(0), world.id(1));
    let rights = vec![Right::Handoff, Right::Files, Right::Clipboard, Right::Call];
    let (_, a) = world.nodes[0].set_rights(&one, &rights);
    world.run(0, a);
    let (_, b) = world.nodes[1].set_rights(&zero, &rights);
    world.run(1, b);
    world.take(0);
    world.take(1);
    let (id, effects) = world.nodes[0].media_open(&one, "t", "a");
    world.run(0, effects);
    let _ = incoming(&mut world, 1);
    // Ids carry the device's own number: the other device's first session differs.
    let (other, effects) = world.nodes[1].media_open(&zero, "t", "a");
    world.run(1, effects);
    assert!(other != 0 && other != id);
    assert!(
        id > u64::from(u32::MAX),
        "ids start at the device's own number"
    );
    let down = world.nodes[1].link_down(&zero);
    world.run(1, down);
    assert!(ended(&mut world, 1).iter().any(|p| p.id == id));
    assert!(ended(&mut world, 1).is_empty(), "taken once");
}
