//! Re-review (w4/comprev2, against b8849e3): the overdraw budget. FINDING
//! tests fail on b8849e3 (the budget test in release builds only).

mod common;

use common::*;
use std::time::Duration;
use ultar_compositor::Compositor;
use ultar_surface::proto::{BufferId, Event, PixelFormat, Rect, Request, Role, SurfaceId};

fn cpu_now() -> Duration {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// A translucent (premultiplied grey, alpha 0x40) surface of `size`.
fn translucent(p: &mut Peer, comp: &mut Compositor, (id, role): (u32, Role), size: (u32, u32)) {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    let b = BufferId(id + 500);
    p.send(&Request::CreateInlineBuffer {
        buffer: b,
        width: size.0,
        height: size.1,
        format: PixelFormat::Argb8888,
    });
    let rows = ((1 << 20) / (size.0 * 4)).max(1);
    let mut y = 0;
    while y < size.1 {
        let n = rows.min(size.1 - y);
        p.send(&Request::WritePixels {
            buffer: b,
            rect: Rect::new(0, y as i32, size.0, n),
            data: vec![0x40; (size.0 * n * 4) as usize],
        });
        p.pump(comp);
        y += n;
    }
    p.send(&Request::Attach {
        surface: s,
        buffer: b,
    });
    let serial = p.last_configure(s).map_or(0, |c| c.serial);
    p.send(&Request::Commit {
        surface: s,
        configure: serial,
    });
    p.send(&Request::DestroyBuffer { buffer: b });
    p.pump(comp);
}

/// FINDING N11: the budget is spent bottom to top by whoever is lowest.
/// Two hostile ids put three screens of translucent windows below;
/// the user's translucent app on top is culled – invisible – and the click
/// the user aims at it lands in the hostile window below.
#[test]
fn the_overdraw_budget_cannot_hide_the_window_on_top() {
    let mut comp = compositor(test_config(), &[(1024, 768)]);
    let mut hostile = Vec::new();
    for (uid, n) in [(5000u32, 4u32), (5001, 4)] {
        let mut p = Peer::hello_as(&mut comp, uid);
        for id in 1..=n {
            translucent(&mut p, &mut comp, (id, Role::Toplevel), (1024, 728));
        }
        hostile.push(p);
    }
    let mut user = Peer::hello_as(&mut comp, 6000);
    translucent(&mut user, &mut comp, (1, Role::Toplevel), (600, 400));
    let top = comp.state.windows.last().unwrap().key;
    comp.state.focus_window(top);
    let c = comp.state.windows.last().unwrap().content();
    let (_, culled) = comp.state.scene_parts();
    user.take();
    click(&mut comp, f64::from(c.x + 10), f64::from(c.y + 10));
    user.pump(&mut comp);
    let got = user
        .events
        .iter()
        .filter(|e| matches!(e, Event::PointerButton { .. }))
        .count();
    assert!(
        !culled.contains(&top) && got > 0,
        "N11: top window culled={} , clicks it got={got}",
        culled.contains(&top)
    );
}

/// FINDING N12: opaque windows are outside the overdraw budget, but each
/// floating window still costs its shadow band, its corners and its title
/// bar. One uid with 8 connections × 63 thin full-width windows (memory:
/// a few KiB) makes a full frame cost far beyond one refresh, for the
/// shell too. Release only.
#[test]
#[cfg_attr(debug_assertions, ignore = "measured in release builds")]
fn thin_opaque_windows_stay_within_the_frame_budget() {
    let mut comp = compositor(test_config(), &[(1920, 1080)]);
    let mut peers = Vec::new();
    for _ in 0..8 {
        let mut p = Peer::hello_as(&mut comp, 5000);
        for id in 1..=63 {
            let s = SurfaceId(id);
            p.send(&Request::CreateSurface {
                surface: s,
                role: Role::Toplevel,
            });
            p.pump(&mut comp);
            let serial = p.last_configure(s).unwrap().serial;
            p.fill(&mut comp, s, serial, (1800, 2), 0x202020);
        }
        assert!(p.error().is_none());
        peers.push(p);
    }
    let windows = comp.state.windows.iter().filter(|w| w.visible()).count();
    let mut cpus: Vec<u128> = (0..10)
        .map(|_| {
            comp.state.damage_all();
            let c = cpu_now();
            comp.render_now();
            cpu_now().saturating_sub(c).as_micros()
        })
        .collect();
    cpus.sort_unstable();
    println!(
        "{windows} thin windows: full frame cpu median {} us",
        cpus[5]
    );
    assert!(
        cpus[5] <= 16_000,
        "N12: {windows} thin opaque windows cost {} us per frame",
        cpus[5]
    );
}
