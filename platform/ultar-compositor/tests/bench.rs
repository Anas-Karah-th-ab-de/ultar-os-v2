//! Performance budgets (plan §3.6, docs/COMPOSITOR.md §9). Asserted only
//! in release builds (`cargo test --release --test bench -- --test-threads=1`);
//! debug builds skip them. Best of three rounds, because the build machine
//! is shared.

mod common;

use common::*;
use std::time::{Duration, Instant};
use ultar_surface::proto::{BufferId, OutputId, Rect, Request, SurfaceId};

/// Full-HD composition budget per frame.
const FULL_FRAME_BUDGET_US: u128 = 4_000;
/// A small change (a blinking caret, a hover) must be far cheaper.
const SMALL_DAMAGE_BUDGET_US: u128 = 300;

fn scene() -> (ultar_compositor::Compositor, Vec<(Peer, SurfaceId)>) {
    let mut comp = compositor(test_config(), &[(1920, 1080)]);
    let mut peers = Vec::new();
    for i in 0..4u32 {
        let mut p = Peer::hello(&mut comp);
        let s = p.window(&mut comp, 1, 0x303030 + i * 0x101010);
        peers.push((p, s));
    }
    // One maximised window below, three floating with shadows above.
    let (p, s) = &mut peers[0];
    p.send(&Request::SetState {
        surface: *s,
        state: ultar_surface::proto::WindowState::Maximized,
    });
    p.pump(&mut comp);
    p.draw(&mut comp, *s, 0x202020);
    comp.render_now();
    (comp, peers)
}

fn p99(mut v: Vec<u128>) -> u128 {
    v.sort_unstable();
    v[(v.len() * 99 / 100).min(v.len() - 1)]
}

/// CPU time of this thread: the work of a frame without the time other
/// processes on the shared build machine take the core away.
fn cpu_now() -> Duration {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::ThreadCPUTime);
    Duration::new(t.tv_sec as u64, t.tv_nsec as u32)
}

/// Best of three rounds of 100 frames: (p99 of CPU time, p99 of wall time),
/// microseconds. `frame` returns (wall, cpu).
fn measure(mut frame: impl FnMut() -> (Duration, Duration)) -> (u128, u128) {
    let mut best = (u128::MAX, u128::MAX);
    for _ in 0..3 {
        let (walls, cpus): (Vec<u128>, Vec<u128>) = (0..100)
            .map(|_| {
                let (w, c) = frame();
                (w.as_micros(), c.as_micros())
            })
            .unzip();
        let round = (p99(cpus), p99(walls));
        if round.0 < best.0 {
            best = round;
        }
    }
    best
}

fn timed(f: impl FnOnce()) -> (Duration, Duration) {
    let (t, c) = (Instant::now(), cpu_now());
    f();
    (t.elapsed(), cpu_now().saturating_sub(c))
}

#[test]
#[cfg_attr(debug_assertions, ignore = "budgets are measured in release builds")]
fn full_hd_full_damage_within_budget() {
    let (mut comp, _peers) = scene();
    let id = OutputId(0);
    let (cpu, wall) = measure(|| {
        comp.state.damage_all();
        timed(|| {
            comp.render_now();
        })
    });
    println!("full frame 1920x1080, 4 windows: p99 cpu {cpu} us, wall {wall} us (budget {FULL_FRAME_BUDGET_US})");
    assert!(cpu <= FULL_FRAME_BUDGET_US, "full frame p99 {cpu} us");
    assert!(comp.frame(id).is_some());
}

#[test]
#[cfg_attr(debug_assertions, ignore = "budgets are measured in release builds")]
fn small_damage_within_budget() {
    let (mut comp, mut peers) = scene();
    let (p, s) = &mut peers[3];
    let c = p.last_configure(*s).unwrap();
    let b = BufferId(1000 + s.0);
    let (cpu, wall) = measure(|| {
        p.send(&Request::WritePixels {
            buffer: b,
            rect: Rect::new(10, 10, 2, 20),
            data: vec![0xff; 2 * 20 * 4],
        });
        p.send(&Request::Attach {
            surface: *s,
            buffer: b,
        });
        p.send(&Request::Damage {
            surface: *s,
            rects: vec![Rect::new(10, 10, 2, 20)],
        });
        p.send(&Request::Commit {
            surface: *s,
            configure: c.serial,
        });
        p.pump(&mut comp);
        timed(|| {
            comp.render_now();
        })
    });
    println!(
        "caret-sized damage: p99 cpu {cpu} us, wall {wall} us (budget {SMALL_DAMAGE_BUDGET_US})"
    );
    assert!(cpu <= SMALL_DAMAGE_BUDGET_US, "small damage p99 {cpu} us");
}

#[test]
fn idle_means_no_work() {
    let (mut comp, _peers) = scene();
    comp.render_now();
    let frames = comp.stats.frames;
    // Nothing happens: dispatch only waits (here bounded by the timeout).
    for _ in 0..5 {
        comp.dispatch(Some(Duration::from_millis(20))).unwrap();
    }
    assert_eq!(comp.stats.frames, frames, "no frame composed while idle");
    assert!(!comp.state.has_damage());
}
