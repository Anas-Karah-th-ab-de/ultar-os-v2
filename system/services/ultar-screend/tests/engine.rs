//! A session of the source over a scripted screen: what is sent, how fast, what is injected
//! and when.
mod common;
use common::{Fake, Shared};
use std::collections::VecDeque;
use ultar_bus::messages::mesh_view::{Button, RemoteInput};
use ultar_screend::diff::{Rect, BPP};
use ultar_screend::engine::{Engine, Sink, Step};
use ultar_screend::input::{Injector, Recorder};
use ultar_screend::pace::{IDLE_MAX, MIN};
use ultar_screend::screen::ScreenError;

#[derive(Default)]
struct Mesh {
    taken: Vec<(Rect, Vec<u8>)>,
    refuse: bool,
    offered: usize,
}

impl Sink for Mesh {
    fn picture(&mut self, rect: Rect, data: Vec<u8>) -> bool {
        self.offered += 1;
        if self.refuse {
            return false;
        }
        self.taken.push((rect, data));
        true
    }
}

type Rig = (
    Engine<Fake, Injector<Recorder, Recorder>>,
    Shared,
    Recorder,
    Recorder,
);

fn rig(w: u32, h: u32) -> Rig {
    let screen = Shared::new(w, h, 1);
    let (p, k) = (Recorder::default(), Recorder::default());
    let (pc, kc) = (p.clone(), k.clone());
    let (engine, size) =
        Engine::begin(Fake(screen.clone()), move |s| Injector::new(pc, kc, s)).unwrap();
    assert_eq!(size, (w, h));
    (engine, screen, p, k)
}

fn look(
    e: &mut Engine<Fake, Injector<Recorder, Recorder>>,
    mesh: &mut Mesh,
    input: &mut VecDeque<RemoteInput>,
) -> Step {
    e.step(&mut || input.pop_front(), mesh)
}

#[test]
fn the_first_look_sends_everything_then_only_what_changed() {
    let (mut e, screen, _, _) = rig(200, 70);
    let (mut mesh, mut input) = (Mesh::default(), VecDeque::new());
    assert_eq!(look(&mut e, &mut mesh, &mut input), Step::Wait(MIN));
    let sent: u32 = mesh.taken.iter().map(|(r, _)| r.w * r.h).sum();
    assert_eq!(sent, 200 * 70, "the whole screen once");
    // Quiet: nothing is sent and the next look comes later and later, up to the maximum.
    mesh.taken.clear();
    let mut waits = Vec::new();
    for _ in 0..6 {
        if let Step::Wait(d) = look(&mut e, &mut mesh, &mut input) {
            waits.push(d);
        }
    }
    assert!(mesh.taken.is_empty());
    assert_eq!(*waits.last().unwrap(), IDLE_MAX);
    assert!(waits.windows(2).all(|w| w[0] <= w[1]));
    // One pixel changes: one rectangle with exactly that pixel, and full speed again.
    screen.poke(70, 5, 99);
    assert_eq!(look(&mut e, &mut mesh, &mut input), Step::Wait(MIN));
    assert_eq!(mesh.taken.len(), 1);
    let (rect, data) = &mesh.taken[0];
    // the tile that holds the pixel (x 64..128), cut out of the screen
    assert_eq!(
        *rect,
        Rect {
            x: 64,
            y: 0,
            w: 64,
            h: 64
        }
    );
    assert_eq!(data[(5 * 64 + 6) * BPP], 99);
}

#[test]
fn a_slow_link_is_retried_not_buffered_and_the_rate_drops() {
    let (mut e, screen, _, _) = rig(128, 64);
    let (mut mesh, mut input) = (
        Mesh {
            refuse: true,
            ..Mesh::default()
        },
        VecDeque::new(),
    );
    let mut last = MIN;
    for _ in 0..4 {
        if let Step::Wait(d) = look(&mut e, &mut mesh, &mut input) {
            assert!(d >= last, "never faster while the link refuses");
            last = d;
        }
    }
    assert!(last > MIN);
    assert_eq!(
        mesh.offered, 4,
        "offered again at every look, one try each, nothing queued"
    );
    // The link recovers: the same screen is delivered completely.
    mesh.refuse = false;
    look(&mut e, &mut mesh, &mut input);
    assert_eq!(
        mesh.taken.iter().map(|(r, _)| r.w * r.h).sum::<u32>(),
        128 * 64
    );
    let _ = screen;
}

#[test]
fn the_viewers_input_is_injected_in_order_after_the_screen_proved_unlocked() {
    let (mut e, screen, p, k) = rig(64, 64);
    let (mut mesh, mut input) = (Mesh::default(), VecDeque::new());
    input.push_back(RemoteInput::Move { x: 10, y: 20 });
    input.push_back(RemoteInput::Button {
        x: 10,
        y: 20,
        button: Button::Left,
        down: true,
    });
    input.push_back(RemoteInput::Key {
        code: 30,
        down: true,
    });
    look(&mut e, &mut mesh, &mut input);
    assert!(input.is_empty());
    assert_eq!(screen.gates(), 2, "begin and one look");
    assert!(p.events().len() >= 6 && k.events().len() == 2);
    let first = p.events()[0];
    assert_eq!(first, (3, 0, 10), "the pointer is placed first");
}

#[test]
fn a_locked_screen_ends_the_session_and_nothing_is_injected() {
    let (mut e, screen, p, k) = rig(64, 64);
    let (mut mesh, mut input) = (Mesh::default(), VecDeque::new());
    look(&mut e, &mut mesh, &mut input);
    // A key is held when the lock comes.
    input.push_back(RemoteInput::Key {
        code: 29,
        down: true,
    });
    look(&mut e, &mut mesh, &mut input);
    assert!(input.is_empty());
    let before = (p.events().len(), k.events().len());
    screen.fail_next(ScreenError::Locked);
    input.push_back(RemoteInput::Key {
        code: 31,
        down: true,
    });
    input.push_back(RemoteInput::Button {
        x: 1,
        y: 1,
        button: Button::Left,
        down: true,
    });
    assert_eq!(look(&mut e, &mut mesh, &mut input), Step::Locked);
    // Typing into the lock screen is impossible: the new events were not injected ...
    assert_eq!(input.len(), 2, "still waiting, never applied");
    assert_eq!(p.events().len(), before.0);
    // ... and the key that was held was released.
    assert!(k.events().contains(&(1, 29, 0)));
    assert!(!k.events().contains(&(1, 31, 1)));
}

#[test]
fn a_gone_or_resized_screen_ends_the_session() {
    let (mut e, screen, _, k) = rig(64, 64);
    let (mut mesh, mut input) = (Mesh::default(), VecDeque::new());
    screen.fail_next(ScreenError::Gone);
    assert_eq!(look(&mut e, &mut mesh, &mut input), Step::Gone);
    let (mut e, screen, _, _) = rig(64, 64);
    screen.resize(128, 64);
    assert_eq!(
        look(&mut e, &mut mesh, &mut input),
        Step::Gone,
        "the viewer's picture size is fixed"
    );
    // Ending by hand releases what is held, too.
    let (mut e, _, _, k2) = rig(64, 64);
    input.push_back(RemoteInput::Key {
        code: 42,
        down: true,
    });
    look(&mut e, &mut mesh, &mut input);
    e.finish(Step::Gone);
    assert!(k2.events().contains(&(1, 42, 0)));
    let _ = k;
}

#[test]
fn the_person_in_front_changing_ends_the_session_and_releases_what_is_held() {
    // the service ends the session on session.state (see seat.rs): the engine lets go of everything
    let (mut e, _, p, k) = rig(64, 64);
    let (mut mesh, mut input) = (Mesh::default(), VecDeque::new());
    input.push_back(RemoteInput::Key {
        code: 42,
        down: true,
    });
    input.push_back(RemoteInput::Button {
        x: 3,
        y: 3,
        button: Button::Left,
        down: true,
    });
    look(&mut e, &mut mesh, &mut input);
    assert_eq!(e.finish(Step::Locked), Step::Locked);
    assert!(k.events().contains(&(1, 42, 0)), "key released");
    assert!(p.events().contains(&(1, 0x110, 0)), "button released");
}
