//! Output hot-plug and layered damage checks of the independent review
//! (w4/comprev, against 0f4846e). FINDING tests fail on 0f4846e, SOUND
//! ones pass.

mod common;

use common::*;
use std::sync::{Arc, Mutex};
use ultar_compositor::backend::{Backend, BackendOutput, Hotplug};
use ultar_compositor::geom::Rect as LRect;
use ultar_compositor::pixels::Image;
use ultar_compositor::{Compositor, MonotonicClock, RightsTable};
use ultar_hal::{HalError, InputEvent, InputEventKind, PointerButton};
use ultar_surface::proto::{
    BufferId, Edge, Event, OutputId, PixelFormat, Rect, Request, Rights, Role, SurfaceId,
};

type Plugs = Arc<Mutex<(Vec<BackendOutput>, Vec<Hotplug>)>>;

/// A headless backend the test can still reach after the compositor took it.
struct Shared(Plugs);

impl Backend for Shared {
    fn outputs(&self) -> Vec<BackendOutput> {
        self.0.lock().unwrap().0.clone()
    }
    fn present(&mut self, _: u32, _: &Image, _: &[LRect]) -> Result<(), HalError> {
        Ok(())
    }
    fn input_fds(&self) -> Vec<std::os::fd::BorrowedFd<'_>> {
        Vec::new()
    }
    fn read_input(&mut self) -> Vec<InputEvent> {
        Vec::new()
    }
    fn hotplug(&mut self) -> Vec<Hotplug> {
        std::mem::take(&mut self.0.lock().unwrap().1)
    }
}

fn output(hal: u32, width: u32, height: u32) -> BackendOutput {
    BackendOutput {
        hal,
        name: format!("review-{hal}"),
        width,
        height,
        refresh_millihertz: 60_000,
        width_mm: 0,
    }
}

fn shared_compositor(config: ultar_compositor::Config) -> (Compositor, Plugs) {
    let plugs: Plugs = Arc::new(Mutex::new((
        vec![output(0, 1280, 800), output(1, 1024, 768)],
        Vec::new(),
    )));
    let comp = Compositor::new(
        config,
        Box::new(Shared(plugs.clone())),
        Box::new(MonotonicClock::default()),
    );
    (comp, plugs)
}

fn press(comp: &mut Compositor, pressed: bool) {
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed,
    })]);
}

fn check(comp: &mut Compositor, step: &str) {
    comp.render_now();
    let ids: Vec<OutputId> = comp.state.layout.enabled().map(|o| o.id).collect();
    for id in ids {
        let reference = comp.render_reference(id).unwrap();
        let frame = comp.frame(id).unwrap();
        let differ = frame
            .data()
            .iter()
            .zip(reference.data())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(differ, 0, "{step}: output {id:?} differs in {differ} px");
    }
}

/// FINDING m4: after the display under the pointer is unplugged the
/// pointer stays at its old position outside every output (invisible); a
/// click or a tap without motion goes nowhere until the pointer moves.
/// SOUND part: the window comes back to the primary, incremental == full.
#[test]
fn unplugging_the_display_under_window_and_pointer() {
    let (mut comp, plugs) = shared_compositor(test_config());
    let mut a = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0xff0000);
    let f = comp.state.windows[0].frame;
    move_to(&mut comp, f64::from(f.x + f.w / 2), f64::from(f.y + 10));
    press(&mut comp, true);
    let start = f64::from(f.x + f.w / 2);
    for i in 1..=30 {
        move_to(&mut comp, start + f64::from(i) * 50.0, 300.0);
    }
    press(&mut comp, false);
    a.pump(&mut comp);
    a.draw(&mut comp, SurfaceId(1), 0xff0000);
    let moved = comp.state.windows[0].frame;
    assert!(
        moved.x >= 1280,
        "window not moved: {moved:?} from {f:?}, pointer {:?}",
        (comp.state.pointer.x, comp.state.pointer.y)
    );
    {
        let mut p = plugs.lock().unwrap();
        p.0.retain(|o| o.hal != 1);
        p.1.push(Hotplug::Removed(1));
    }
    comp.rescan_outputs();
    a.pump(&mut comp);
    a.draw(&mut comp, SurfaceId(1), 0xff0000);
    assert!(a
        .events
        .iter()
        .any(|e| matches!(e, Event::OutputRemoved { .. })));
    let primary = comp.state.layout.primary().unwrap().logical_rect();
    assert!(comp.state.windows[0].frame.intersect(&primary).is_some());
    check(&mut comp, "after unplug");
    let (x, y) = (comp.state.pointer.x as i32, comp.state.pointer.y as i32);
    assert!(
        primary.contains(x, y),
        "m4: pointer left at ({x},{y}) outside {primary:?}"
    );
}

fn fill_argb(p: &mut Peer, comp: &mut Compositor, s: SurfaceId, size: (u32, u32), px: u32) {
    let b = BufferId(5000 + s.0);
    p.send(&Request::DestroyBuffer { buffer: b });
    p.send(&Request::CreateInlineBuffer {
        buffer: b,
        width: size.0,
        height: size.1,
        format: PixelFormat::Argb8888,
    });
    let data: Vec<u8> = px
        .to_le_bytes()
        .iter()
        .copied()
        .cycle()
        .take((size.0 * size.1 * 4) as usize)
        .collect();
    p.send(&Request::WritePixels {
        buffer: b,
        rect: Rect::new(0, 0, size.0, size.1),
        data,
    });
    p.send(&Request::Attach {
        surface: s,
        buffer: b,
    });
    let serial = p.last_configure(s).map_or(0, |c| c.serial);
    p.send(&Request::Commit {
        surface: s,
        configure: serial,
    });
    p.pump(comp);
}

fn layer(p: &mut Peer, comp: &mut Compositor, id: u32, role: Role, size: (u32, u32)) {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    fill_argb(p, comp, s, size, 0x8020_4060);
}

fn layered_scene() -> (Compositor, Peer, Peer) {
    let mut rights = RightsTable::default();
    let (x, _y) = std::os::unix::net::UnixStream::pair().unwrap();
    rights.grant(ultar_compositor::server::peer_of(&x).uid, Rights::ALL);
    let config = ultar_compositor::Config {
        rights,
        lang: ultar_ui::Lang::Ar,
        ..test_config()
    };
    let mut comp = compositor(config, &[(1024, 768), (800, 600)]);
    let second = comp.state.layout.outputs()[1].id;
    comp.state.layout.set_scale(second, 1500);
    comp.state.outputs_changed();
    let mut shell = Peer::hello(&mut comp);
    let panel = Role::Panel {
        edge: Edge::Start,
        size: 56,
    };
    layer(&mut shell, &mut comp, 1, panel, (56, 768));
    layer(&mut shell, &mut comp, 2, Role::Overlay, (256, 96));
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0x336699);
    (comp, shell, app)
}

/// SOUND: incremental == full with a translucent panel and overlay,
/// (also not premultiplied: hostile alpha) translucent popups appearing, moving and vanishing, on a 150 % second
/// output, in Arabic (panel at the right).
#[test]
fn incremental_equals_full_with_layers_and_popups() {
    let (mut comp, mut shell, mut app) = layered_scene();
    check(&mut comp, "start");
    let mut rng = Rng(0xfeed);
    for step in 0..30 {
        let id = 10 + (step % 3) as u32;
        match rng.below(4) {
            0 => {
                let (x, y) = (rng.below(900) as i32 - 100, rng.below(700) as i32);
                let popup = Role::Popup {
                    parent: SurfaceId(1),
                    x,
                    y,
                };
                app.send(&Request::DestroySurface {
                    surface: SurfaceId(id),
                });
                app.pump(&mut comp);
                let size = (rng.below(300) as u32 + 1, rng.below(200) as u32 + 1);
                layer(&mut app, &mut comp, id, popup, size);
            }
            1 => fill_argb(
                &mut shell,
                &mut comp,
                SurfaceId(2),
                (256, 96),
                rng.next() as u32,
            ),
            2 => {
                let x = rng.below(1000) as f64;
                click(&mut comp, x, rng.below(700) as f64);
            }
            _ => app.draw(&mut comp, SurfaceId(1), rng.next() as u32 & 0xff_ffff),
        }
        check(&mut comp, &format!("step {step}"));
    }
}

/// FINDING m5: the move offset is taken when the drag threshold is
/// crossed, not at the press: the first motion is lost and the window
/// stays behind the pointer by it (a fast flick or a touch drag: hundreds
/// of points).
#[test]
fn a_title_drag_keeps_the_grabbed_point_under_the_pointer() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0xff0000);
    let f = comp.state.windows[0].frame;
    let grab = (f64::from(f.x + 100), f64::from(f.y + 10));
    move_to(&mut comp, grab.0, grab.1);
    press(&mut comp, true);
    move_to(&mut comp, grab.0 + 200.0, grab.1 + 100.0);
    move_to(&mut comp, grab.0 + 210.0, grab.1 + 100.0);
    press(&mut comp, false);
    let g = comp.state.windows[0].frame;
    assert_eq!(
        (g.x - f.x, g.y - f.y),
        (210, 100),
        "m5: window moved by ({}, {}) for a drag of (210, 100)",
        g.x - f.x,
        g.y - f.y
    );
}
