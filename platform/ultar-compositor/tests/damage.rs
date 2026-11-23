//! Incremental composition equals full composition (differential test):
//! random window operations and partial commits, after every frame the
//! damage-based result must be pixel-identical to composing from scratch.

mod common;

use common::*;
use ultar_compositor::Config;
use ultar_hal::{InputEventKind, PointerButton};
use ultar_surface::proto::{
    BufferId, OutputId, PixelFormat, Rect, Request, SurfaceId, WindowState,
};

fn check(comp: &mut ultar_compositor::Compositor, step: usize) {
    comp.render_now();
    let outputs: Vec<OutputId> = comp.state.layout.enabled().map(|o| o.id).collect();
    for id in outputs {
        let reference = comp.render_reference(id).unwrap();
        let frame = comp.frame(id).unwrap();
        let differ = frame
            .data()
            .iter()
            .zip(reference.data())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differ, 0,
            "step {step}: output {id:?} differs in {differ} pixels"
        );
    }
}

fn partial(peer: &mut Peer, comp: &mut ultar_compositor::Compositor, s: SurfaceId, rng: &mut Rng) {
    let c = peer.last_configure(s).unwrap();
    let (w, h) = (c.width, c.height);
    let b = BufferId(1000 + s.0);
    let rw = (rng.below(u64::from(w / 2)) + 1) as u32;
    let rh = (rng.below(u64::from(h / 2)) + 1) as u32;
    let x = rng.below(u64::from(w - rw)) as i32;
    let y = rng.below(u64::from(h - rh)) as i32;
    let color = (rng.next() as u32) | 0xff00_0000;
    let data: Vec<u8> = color
        .to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take((rw * rh * 4) as usize)
        .collect();
    peer.send(&Request::WritePixels {
        buffer: b,
        rect: Rect::new(x, y, rw, rh),
        data,
    });
    peer.send(&Request::Attach {
        surface: s,
        buffer: b,
    });
    peer.send(&Request::Damage {
        surface: s,
        rects: vec![Rect::new(x, y, rw, rh)],
    });
    peer.send(&Request::Commit {
        surface: s,
        configure: c.serial,
    });
    peer.pump(comp);
}

/// Drags the title bar of window `which` by a random amount.
fn drag_title(comp: &mut ultar_compositor::Compositor, which: usize, rng: &mut Rng) {
    let f = comp.state.windows[which].frame;
    let (x, y) = (f64::from(f.x + f.w / 2), f64::from(f.y + 10));
    move_to(comp, x, y);
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    })]);
    move_to(
        comp,
        x + rng.below(200) as f64 - 100.0,
        y + rng.below(120) as f64,
    );
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    })]);
}

fn run(config: Config, sizes: &[(u32, u32)], seed: u64) {
    let mut comp = compositor(config, sizes);
    let mut rng = Rng(seed);
    let mut peers: Vec<(Peer, SurfaceId)> = Vec::new();
    for i in 0..3u32 {
        let mut p = Peer::hello(&mut comp);
        let s = p.window(&mut comp, 1, 0x203040 + i * 0x202020);
        peers.push((p, s));
    }
    check(&mut comp, 0);
    for step in 1..60 {
        let which = rng.below(3) as usize;
        match rng.below(6) {
            0 | 1 => {
                let (p, s) = &mut peers[which];
                partial(p, &mut comp, *s, &mut rng);
            }
            2 => {
                // Click somewhere: focus and stacking change.
                let x = rng.below(1000) as f64;
                let y = rng.below(700) as f64;
                click(&mut comp, x, y);
            }
            3 => drag_title(&mut comp, which, &mut rng),
            4 => {
                let (p, s) = &mut peers[which];
                let state = [
                    WindowState::Maximized,
                    WindowState::Normal,
                    WindowState::Minimized,
                ][rng.below(3) as usize];
                p.send(&Request::SetState { surface: *s, state });
                p.pump(&mut comp);
                p.draw(&mut comp, *s, 0x506070);
            }
            _ => {
                let (p, s) = &mut peers[which];
                p.draw(&mut comp, *s, (rng.next() as u32) & 0xffffff);
            }
        }
        check(&mut comp, step);
    }
    let _ = PixelFormat::Xrgb8888;
}

#[test]
fn incremental_equals_full_single_output() {
    run(test_config(), &[(1024, 768)], 0x1234_5678);
}

#[test]
fn incremental_equals_full_two_outputs_with_scale() {
    let mut comp_config = test_config();
    comp_config.lang = ultar_ui::Lang::Ar;
    run(comp_config, &[(1024, 768), (800, 600)], 0x9e37_79b9);
}

#[test]
fn hidpi_output_and_killed_client() {
    let mut comp = compositor(test_config(), &[(1600, 1000)]);
    let id = comp.state.layout.primary().unwrap().id;
    comp.state.layout.set_scale(id, 1250);
    comp.state.outputs_changed();
    let mut a = Peer::hello(&mut comp);
    let s = a.window(&mut comp, 1, 0xaa0000);
    let mut b = Peer::hello(&mut comp);
    b.window(&mut comp, 1, 0x00aa00);
    check(&mut comp, 0);
    let mut rng = Rng(7);
    partial(&mut a, &mut comp, s, &mut rng);
    check(&mut comp, 1);
    drop(b);
    comp.dispatch(Some(std::time::Duration::ZERO)).unwrap();
    assert_eq!(comp.state.clients.len(), 1);
    check(&mut comp, 2);
}

#[test]
fn arabic_desktop_snapshot() {
    let config = Config {
        lang: ultar_ui::Lang::Ar,
        ..test_config()
    };
    let mut comp = compositor(config, &[(1024, 640)]);
    let mut files = Peer::hello(&mut comp);
    files.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: ultar_surface::proto::Role::Toplevel,
    });
    files.send(&Request::SetTitle {
        surface: SurfaceId(1),
        title: "الملفات".into(),
    });
    files.pump(&mut comp);
    files.draw(&mut comp, SurfaceId(1), 0x2a6f4e);
    let mut web = Peer::hello(&mut comp);
    web.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: ultar_surface::proto::Role::Toplevel,
    });
    web.send(&Request::SetTitle {
        surface: SurfaceId(1),
        title: "متصفح Ultar – 2 نوافذ".into(),
    });
    web.pump(&mut comp);
    web.draw(&mut comp, SurfaceId(1), 0x6f4e2a);
    // The user clicks the new window's title bar (it opened inactive).
    let f = comp.state.windows[1].frame;
    click(&mut comp, f64::from(f.x + f.w / 2), f64::from(f.y + 10));
    web.pump(&mut comp);
    check(&mut comp, 0);
    assert_snapshot("two_windows_ar", comp.frame(OutputId(0)).unwrap());
}
