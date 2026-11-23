//! Worst-case composition cost of the independent review (w4/comprev),
//! now an asserted budget (M7): an app without rights stacks as many
//! translucent screen-sized surfaces as the limits let it (popups up to
//! the per-window cap, further translucent toplevels, two connections),
//! three such apps (three user ids) at once, and a full frame must still
//! fit the budget. Release only
//! (`cargo test --release --test review_bench`).

mod common;

use common::*;
use std::time::Duration;
use ultar_surface::proto::{BufferId, PixelFormat, Rect, Request, Role, SurfaceId};

/// CPU budget of one full 1920×1080 frame of the hostile scene (p99 of
/// the best of three rounds): within one refresh at 60 Hz.
const HOSTILE_FRAME_BUDGET_US: u128 = 16_000;

fn cpu_now() -> Duration {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// One translucent 1920×1080 surface; its buffer is dropped after the
/// commit so only the committed copy is charged (8 MiB).
fn translucent(p: &mut Peer, comp: &mut ultar_compositor::Compositor, id: u32, role: Role) {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    let b = BufferId(id);
    p.send(&Request::CreateInlineBuffer {
        buffer: b,
        width: 1920,
        height: 1080,
        format: PixelFormat::Argb8888,
    });
    for y in (0..1080).step_by(120) {
        p.send(&Request::WritePixels {
            buffer: b,
            rect: Rect::new(0, y, 1920, 120),
            data: vec![0x08; 1920 * 120 * 4],
        });
        p.pump(comp);
    }
    p.send(&Request::Attach {
        surface: s,
        buffer: b,
    });
    p.send(&Request::Commit {
        surface: s,
        configure: 0,
    });
    p.send(&Request::DestroyBuffer { buffer: b });
    p.pump(comp);
}

fn hostile_scene() -> (ultar_compositor::Compositor, Vec<Peer>) {
    let mut comp = compositor(test_config(), &[(1920, 1080)]);
    let cap = comp.state.config.limits.popups as u32;
    let mut peers = Vec::new();
    for c in 0..6u32 {
        let mut p = Peer::hello_as(&mut comp, 5000 + c / 2);
        p.window(&mut comp, 1, 0x303030);
        let popup = Role::Popup {
            parent: SurfaceId(1),
            x: -4000,
            y: -4000,
        };
        for id in 2..2 + cap {
            translucent(&mut p, &mut comp, id, popup);
        }
        for id in 100..106 {
            translucent(&mut p, &mut comp, id, Role::Toplevel);
        }
        assert!(p.error().is_none(), "client {c} refused within the limits");
        peers.push(p);
    }
    (comp, peers)
}

/// M7: the popup cap holds (the next popup ends the client) and the frame
/// cost of everything the limits allow stays within the budget.
#[test]
#[cfg_attr(debug_assertions, ignore = "measured in release builds")]
fn translucent_stack_frame_cost_is_bounded() {
    let (mut comp, mut peers) = hostile_scene();
    let (layers, culled) = {
        let (scene, culled) = comp.state.scene_parts();
        (scene.len(), culled.len())
    };
    assert!(culled > 0, "the overdraw budget left nothing out");
    let mut best = u128::MAX;
    for _ in 0..3 {
        let mut cpus: Vec<u128> = (0..20)
            .map(|_| {
                comp.state.damage_all();
                let c = cpu_now();
                comp.render_now();
                cpu_now().saturating_sub(c).as_micros()
            })
            .collect();
        cpus.sort_unstable();
        best = best.min(cpus[cpus.len() * 99 / 100]);
    }
    println!("{layers} layers shown, {culled} left out: full frame cpu p99 {best} us");
    assert!(
        best <= HOSTILE_FRAME_BUDGET_US,
        "M7: hostile frame {best} us > {HOSTILE_FRAME_BUDGET_US} us"
    );
    // One popup beyond the cap is refused.
    let p = &mut peers[0];
    p.send(&Request::CreateSurface {
        surface: SurfaceId(99),
        role: Role::Popup {
            parent: SurfaceId(1),
            x: 0,
            y: 0,
        },
    });
    p.pump(&mut comp);
    assert!(p.error().is_some(), "popup beyond the cap accepted");
}
