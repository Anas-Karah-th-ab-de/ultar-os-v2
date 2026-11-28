//! Two mesh nodes (viewer and source) wired in memory, the screen service's engine over a
//! scripted screen on the source: the frames arrive pixel-exact, the viewer's input arrives
//! in a recording injector, the lock of the screen ends the session on both sides.
mod common;
use common::{Fake, Shared};
use std::collections::VecDeque;
use std::time::Instant;
use ultar_bus::messages::mesh::{DeviceKind, Right};
use ultar_bus::messages::mesh_view::{Button, RemoteInput};
use ultar_mesh::device::{default_capabilities, id_of};
use ultar_mesh::node::{Config, Effect, Node};
use ultar_mesh::trust::{Trust, DEFAULT_RIGHTS};
use ultar_mesh::view::ViewEnd;
use ultar_screend::diff::Rect;
use ultar_screend::engine::{Engine, Sink, Step};
use ultar_screend::input::{Injector, Recorder};

const W: u32 = 200;
const H: u32 = 300;

fn key(n: u8) -> [u8; 32] {
    [n + 1; 32]
}

/// Node 0 views node 1.
struct Wire {
    nodes: [Node; 2],
    seen: [Vec<Effect>; 2],
    now: Instant,
}

impl Wire {
    fn new(tag: &str) -> Wire {
        let dir = std::env::temp_dir().join(format!("screend-e2e-{tag}-{}", std::process::id()));
        let node = |i: u8, kind| {
            let config = Config {
                name: format!("dev{i}"),
                kind,
                capabilities: default_capabilities(kind),
                apps: Vec::new(),
                inbox: dir.join(format!("inbox{i}")),
                share: Vec::new(),
                remote_view: false,
            };
            Node::new(config, &key(i), Trust::in_memory())
        };
        let mut w = Wire {
            nodes: [node(0, DeviceKind::Phone), node(1, DeviceKind::Desktop)],
            seen: [Vec::new(), Vec::new()],
            now: Instant::now(),
        };
        let (a, b) = (id_of(&key(0)), id_of(&key(1)));
        let up0 = w.nodes[0].link_up(&b, &key(1));
        let up1 = w.nodes[1].link_up(&a, &key(0));
        w.run(0, up0);
        w.run(1, up1);
        let code = w.nodes[1].pair_begin(w.now).code;
        let (ok, effects) = w.nodes[0].pair_join(&b, &code);
        assert!(ok);
        w.run(0, effects);
        // The source's person allows control.
        let mut rights = DEFAULT_RIGHTS.to_vec();
        rights.push(Right::Control);
        assert!(w.nodes[1].set_rights(&a, &rights).0);
        w.seen = [Vec::new(), Vec::new()];
        w
    }

    fn id(i: u8) -> String {
        id_of(&key(i))
    }

    fn run(&mut self, from: usize, effects: Vec<Effect>) {
        let mut queue: VecDeque<(usize, Effect)> = effects.into_iter().map(|e| (from, e)).collect();
        while let Some((at, effect)) = queue.pop_front() {
            if let Effect::Send(to, frame) = effect {
                let target = usize::from(to == Self::id(1));
                let sender = Self::id(at as u8);
                let more = self.nodes[target].frame(&sender, frame, self.now);
                queue.extend(more.into_iter().map(|e| (target, e)));
            } else {
                self.seen[at].push(effect);
            }
        }
    }

    fn take(&mut self, i: usize) -> Vec<Effect> {
        std::mem::take(&mut self.seen[i])
    }
}

/// The mesh service's side for the engine: a rectangle goes through the node of the source.
struct ThroughMesh<'a>(&'a mut Wire);

impl Sink for ThroughMesh<'_> {
    fn picture(&mut self, r: Rect, data: Vec<u8>) -> bool {
        let effects = self.0.nodes[1].view_frame([r.x, r.y, r.w, r.h], &data);
        let taken = !effects.is_empty();
        self.0.run(1, effects);
        taken
    }
}

/// The viewer's picture: what the viewer app would assemble from the frames.
fn paint(picture: &mut [u8], effects: &[Effect]) -> usize {
    let mut frames = 0;
    for e in effects {
        if let Effect::ViewFrame {
            region: [x, y, w, h],
            data,
            ..
        } = e
        {
            for row in 0..*h {
                let dst = ((y + row) * W + x) as usize * 4;
                let src = (row * w) as usize * 4;
                picture[dst..dst + *w as usize * 4]
                    .copy_from_slice(&data[src..src + *w as usize * 4]);
            }
            frames += 1;
        }
    }
    frames
}

fn start(w: &mut Wire) -> u64 {
    let (viewer, source) = (Wire::id(0), Wire::id(1));
    let (id, effects) = w.nodes[0].view_start(&source).unwrap();
    w.run(0, effects);
    assert_eq!(
        w.take(1),
        vec![Effect::ViewIncoming {
            peer: viewer.clone(),
            id
        }]
    );
    let effects = w.nodes[1].view_accept(&viewer, id, W, H);
    w.run(1, effects);
    w.take(0);
    w.take(1);
    id
}

#[test]
fn the_screen_arrives_pixel_exact_and_the_input_arrives_in_the_injector() {
    let mut w = Wire::new("pixels");
    let id = start(&mut w);
    let screen = Shared::new(W, H, 0);
    for y in 0..H {
        for x in 0..W {
            screen.poke(x, y, ((x * 3 + y * 5) % 251) as u8);
        }
    }
    let (p, k) = (Recorder::default(), Recorder::default());
    let (pc, kc) = (p.clone(), k.clone());
    let (mut engine, size) =
        Engine::begin(Fake(screen.clone()), move |s| Injector::new(pc, kc, s)).unwrap();
    assert_eq!(size, (W, H));
    let mut picture = vec![0u8; (W * H * 4) as usize];
    let mut typed = VecDeque::new();

    // The whole screen goes through the mesh: encoded, framed, decoded, drawn.
    let step = engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w));
    assert!(matches!(step, Step::Wait(_)));
    let frames = paint(&mut picture, &w.take(0));
    assert!(frames >= 2, "the screen needs several rectangles");
    assert_eq!(picture, screen.data(), "pixel-exact");

    // Only the changed rectangle follows.
    screen.poke(150, 40, 250);
    engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w));
    assert_eq!(paint(&mut picture, &w.take(0)), 1);
    assert_eq!(picture, screen.data());

    // The viewer's events reach the source's node, the source parses and injects them.
    let (peer, words) = (
        Wire::id(0),
        [
            "m 12 34",
            "d 12 34 l",
            "u 12 34 l",
            "k 30 down",
            "k 30 up",
            "w 12 34 -2",
        ],
    );
    for word in words {
        let effects = w.nodes[0].view_input(word).unwrap();
        w.run(0, effects);
    }
    for e in w.take(1) {
        if let Effect::ViewInput {
            peer: p2, input, ..
        } = e
        {
            assert_eq!(p2, peer);
            typed.push_back(RemoteInput::parse(&input).unwrap());
        }
    }
    assert_eq!(typed.len(), 6);
    engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w));
    let pointer = p.events();
    assert!(
        pointer.contains(&(3, 0, 12)) && pointer.contains(&(3, 1, 34)),
        "placed at 12,34"
    );
    assert!(
        pointer.contains(&(1, 0x110, 1)) && pointer.contains(&(1, 0x110, 0)),
        "click down and up"
    );
    assert!(pointer.contains(&(2, 8, 2)), "wheel -2 scrolls up");
    assert_eq!(
        k.events(),
        vec![(1, 30, 1), (0, 0, 0), (1, 30, 0), (0, 0, 0)]
    );
    let _ = Button::Left;

    // The viewer stops: the source's node ends, the engine is told to release everything.
    let effects = w.nodes[0].view_stop().unwrap();
    w.run(0, effects);
    assert!(w.take(1).contains(&Effect::ViewEnded {
        peer,
        id,
        end: ViewEnd::Stopped
    }));
    assert!(!w.nodes[1].view_room(), "nothing streams after the end");
}

#[test]
fn locking_the_screen_ends_the_session_for_the_viewer_and_types_nothing() {
    let mut w = Wire::new("lock");
    let id = start(&mut w);
    let screen = Shared::new(W, H, 3);
    let (p, k) = (Recorder::default(), Recorder::default());
    let (pc, kc) = (p.clone(), k.clone());
    let (mut engine, _) =
        Engine::begin(Fake(screen.clone()), move |s| Injector::new(pc, kc, s)).unwrap();
    let mut typed = VecDeque::new();
    engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w));
    w.take(0);

    // The screen locks while the viewer is typing a password.
    screen.fail_next(ultar_screend::screen::ScreenError::Locked);
    let effects = w.nodes[0].view_input("k 30 down").unwrap();
    w.run(0, effects);
    for e in w.take(1) {
        if let Effect::ViewInput { input, .. } = e {
            typed.push_back(RemoteInput::parse(&input).unwrap());
        }
    }
    assert_eq!(
        engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w)),
        Step::Locked
    );
    assert!(k.events().is_empty(), "nothing typed into the lock screen");
    assert!(p.events().is_empty());
    // The service tells the mesh; the viewer hears why.
    let effects = w.nodes[1].view_locked();
    w.run(1, effects);
    let source = Wire::id(1);
    assert!(w.take(0).contains(&Effect::ViewEnded {
        peer: source,
        id,
        end: ViewEnd::Locked
    }));
}

#[test]
fn a_slow_viewer_sees_dropped_frames_and_a_complete_picture_later() {
    let mut w = Wire::new("slow");
    start(&mut w);
    let screen = Shared::new(W, H, 9);
    let (mut engine, _) = Engine::begin(Fake(screen.clone()), |s| {
        Injector::new(Recorder::default(), Recorder::default(), s)
    })
    .unwrap();
    let mut typed = VecDeque::new();
    // Acks from the viewer are lost: the source's window fills and the sink says "not taken".
    let mut picture = vec![0u8; (W * H * 4) as usize];
    let mut sent = 0;
    for _ in 0..8 {
        let effects_before = w.seen[0].len();
        // Cut: deliver frames to the viewer but swallow its acks by running the sink by hand.
        struct NoAck<'a>(&'a mut Wire, &'a mut usize);
        impl Sink for NoAck<'_> {
            fn picture(&mut self, r: Rect, data: Vec<u8>) -> bool {
                let effects = self.0.nodes[1].view_frame([r.x, r.y, r.w, r.h], &data);
                let taken = !effects.is_empty();
                // Hand the frame to the viewer but drop what it answers.
                for e in effects {
                    if let Effect::Send(_, frame) = e {
                        let sender = Wire::id(1);
                        let now = self.0.now;
                        let answers = self.0.nodes[0].frame(&sender, frame, now);
                        for a in answers {
                            if !matches!(a, Effect::Send(..)) {
                                self.0.seen[0].push(a);
                            }
                        }
                        *self.1 += 1;
                    }
                }
                taken
            }
        }
        engine.step(&mut || typed.pop_front(), &mut NoAck(&mut w, &mut sent));
        let _ = effects_before;
    }
    assert!(
        sent <= ultar_mesh::view::WINDOW as usize,
        "no more than the window was ever in flight: {sent}"
    );
    paint(&mut picture, &w.take(0));
    assert_ne!(picture, screen.data(), "the picture is still incomplete");
    // The viewer's ack arrives at last: the rest follows, the picture completes.
    let ack = ultar_mesh::wire::Frame::ViewAck {
        id: 1,
        seq: sent as u32,
    };
    w.nodes[1].frame(&Wire::id(0), ack, w.now);
    for _ in 0..40 {
        engine.step(&mut || typed.pop_front(), &mut ThroughMesh(&mut w));
        paint(&mut picture, &w.take(0));
    }
    assert_eq!(picture, screen.data(), "complete once the link keeps up");
}
