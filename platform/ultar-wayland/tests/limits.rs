//! Limits: what is too much or wrong ends that connection with a Wayland
//! error; the bridge and its other connections go on.

mod common;

use common::app::{filled, op, App};
use common::wl::Arg;
use common::Harness;
use std::os::fd::AsFd;
use std::time::Duration;

const T: Duration = Duration::from_secs(5);
const NO_MEMORY: u32 = 2;
const INVALID_STRIDE: u32 = 1;

fn error_of(h: &mut Harness, app: &mut App) -> Option<(u32, u32, String)> {
    app.until(h, T, |w| w.error().is_some());
    app.wl.error()
}

#[test]
fn a_buffer_outside_its_pool_is_an_error() {
    let mut h = Harness::start("outside", &["sleep", "30"]);
    let mut good = App::connect(&mut h);
    let mut bad = App::connect(&mut h);
    let fd = filled(16, 16, 0xFF00_00FF);
    let pool = bad.wl.new_id();
    bad.wl.send_fd(
        bad.shm,
        op::CREATE_POOL,
        &[Arg::U(pool), Arg::I(16 * 16 * 4)],
        Some(fd.as_fd()),
    );
    let buffer = bad.wl.new_id();
    // 16 rows of 64 bytes from offset 64: one row past the end.
    bad.wl.send(
        pool,
        op::CREATE_BUFFER,
        &[
            Arg::U(buffer),
            Arg::I(64),
            Arg::I(16),
            Arg::I(16),
            Arg::I(64),
            Arg::U(0),
        ],
    );
    let (obj, code, _) = error_of(&mut h, &mut bad).expect("a protocol error");
    assert_eq!((obj, code), (pool, INVALID_STRIDE));
    assert!(h.alive());
    let _win = good.window(&mut h, "still here", (80, 60), 0xFF55_6677);
    assert!(good.roundtrip(&mut h), "the other connection goes on");
    assert!(h.pump_until(T, |h| h.find(0xFF55_6677).0 >= 80 * 60 * 9 / 10));
}

#[test]
fn too_many_surfaces_end_the_connection_not_the_bridge() {
    let mut h = Harness::start("many", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    for _ in 0..=ultar_wayland::limits::MAX_SURFACES {
        let s = app.wl.new_id();
        app.wl
            .send(app.compositor, op::CREATE_SURFACE, &[Arg::U(s)]);
    }
    let (obj, code, _) = error_of(&mut h, &mut app).expect("a protocol error");
    assert_eq!((obj, code), (1, NO_MEMORY));
    assert!(h.alive());
    let mut next = App::connect(&mut h);
    assert!(next.roundtrip(&mut h));
}

#[test]
fn too_many_windows_for_the_compositor_are_refused_here() {
    let mut h = Harness::start("windows", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    let mut ended = false;
    for i in 0..ultar_wayland::native::MAX_NATIVE_SURFACES + 2 {
        let surface = app.wl.new_id();
        app.wl
            .send(app.compositor, op::CREATE_SURFACE, &[Arg::U(surface)]);
        let xdg = app.wl.new_id();
        app.wl
            .send(app.wm, op::GET_XDG_SURFACE, &[Arg::U(xdg), Arg::U(surface)]);
        let top = app.wl.new_id();
        app.wl.send(xdg, op::GET_TOPLEVEL, &[Arg::U(top)]);
        app.wl.send(surface, op::COMMIT, &[]);
        if !app.roundtrip(&mut h) {
            ended = i >= ultar_wayland::native::MAX_NATIVE_SURFACES;
            break;
        }
    }
    assert!(ended, "the window past the limit ended the connection");
    assert!(h.alive(), "the bridge itself goes on");
    let mut next = App::connect(&mut h);
    assert!(next.roundtrip(&mut h));
}
