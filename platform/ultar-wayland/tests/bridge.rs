//! End to end on Linux: the compositor (in-process, headless), the bridge
//! (a real process) and a raw Wayland client. `ULTAR_WAYLAND_SHOTS=dir`
//! keeps pictures of the screen.

mod common;

use common::app::{op, App};
use common::wl::{Arg, Args, Wl};
use common::{mapped, Harness};
use std::io::Read;
use std::os::fd::AsFd;
use std::time::Duration;

const BLUE: u32 = 0xFF33_66CC;
const T: Duration = Duration::from_secs(5);

#[test]
fn window_appears_with_its_pixels() {
    let mut h = Harness::start("window", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    let _win = app.window(&mut h, "Ultar", (240, 160), BLUE);
    let (x0, y0, x1, y1) = mapped(&mut h, &mut app, BLUE, 240 * 160 * 9 / 10);
    assert_eq!((x1 - x0 + 1, y1 - y0 + 1), (240, 160), "drawn 1:1");
    h.shot("window");
    assert!(app.wl.error().is_none());
}

#[test]
fn keyboard_and_pointer_reach_the_app() {
    let mut h = Harness::start("input", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    app.input();
    let _win = app.window(&mut h, "Input", (200, 120), BLUE);
    let (x0, y0, x1, y1) = mapped(&mut h, &mut app, BLUE, 200 * 120 * 9 / 10);
    h.click(f64::from(x0 + x1) / 2.0, f64::from(y0 + y1) / 2.0);
    let (ptr, kb) = (app.pointer, app.keyboard);
    assert!(app.until(&mut h, T, |w| w.has(ptr, 3)), "button event");
    let buttons = app.wl.take(ptr, 3);
    let mut a = Args::of(&buttons[0]);
    let (_serial, _time, button, state) = (a.u(), a.u(), a.u(), a.u());
    assert_eq!((button, state), (0x110, 1));
    assert!(app.until(&mut h, T, |w| w.has(kb, 1)), "keyboard enter");
    let keymap = app.wl.take(kb, 0);
    let mut text = String::new();
    std::fs::File::from(
        keymap[0]
            .fd
            .as_ref()
            .unwrap()
            .as_fd()
            .try_clone_to_owned()
            .unwrap(),
    )
    .read_to_string(&mut text)
    .unwrap();
    assert!(text.starts_with("xkb_keymap {") && text.contains("Group3"));
    h.tap(30);
    assert!(app.until(&mut h, T, |w| w.has(kb, 3)), "key event");
    let keys = app.wl.take(kb, 3);
    let mut a = Args::of(&keys[0]);
    let (_s, _t, code, pressed) = (a.u(), a.u(), a.u(), a.u());
    assert_eq!((code, pressed), (30, 1));
    let group_before = last_group(&mut app, kb);
    // Super+Space: the next layout; KEY_A then types in another group.
    h.key(125, true);
    h.tap(57);
    h.key(125, false);
    h.tap(30);
    assert!(app.until(&mut h, T, |w| w.has(kb, 3)));
    let group_after = last_group(&mut app, kb);
    assert_ne!(
        group_before, group_after,
        "the layout switch reached the app"
    );
    assert!(app.wl.error().is_none());
}

/// A toplevel configure of `top` that lists `state`.
fn has_state(w: &Wl, top: u32, state: u32) -> bool {
    w.events
        .iter()
        .filter(|e| e.obj == top && e.op == 0)
        .any(|e| {
            let mut a = Args::of(e);
            let _ = (a.i(), a.i());
            a.a()
                .chunks_exact(4)
                .any(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) == state)
        })
}

fn last_group(app: &mut App, kb: u32) -> u32 {
    let mods = app.wl.take(kb, 4);
    mods.last().map_or(0, |e| {
        let mut a = Args::of(e);
        let _ = (a.u(), a.u(), a.u(), a.u());
        a.u()
    })
}

#[test]
fn maximize_resizes_the_window() {
    let mut h = Harness::start("resize", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    let mut win = app.window(&mut h, "Resize", (300, 200), BLUE);
    mapped(&mut h, &mut app, BLUE, 300 * 200 * 9 / 10);
    app.roundtrip(&mut h);
    app.ack_latest(&mut win);
    app.wl.send(win.top, op::SET_MAXIMIZED, &[]);
    let top = win.top;
    assert!(
        app.until(&mut h, T, |w| has_state(w, top, 1)),
        "configure with the maximized state"
    );
    let states = app.ack_latest(&mut win);
    assert!(states.contains(&1), "maximized state");
    assert!(
        win.size.0 > 300 && win.size.1 > 200,
        "bigger: {:?}",
        win.size
    );
    app.paint(&win, win.size, BLUE);
    let n = (win.size.0 * win.size.1) as usize;
    mapped(&mut h, &mut app, BLUE, n * 9 / 10);
    h.shot("maximized");
}

#[test]
fn popup_shows_and_closes_on_outside_click() {
    let mut h = Harness::start("popup", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    app.input();
    let win = app.window(&mut h, "Menu", (300, 220), BLUE);
    let (x0, y0, _, _) = mapped(&mut h, &mut app, BLUE, 300 * 220 * 9 / 10);
    let pos = app.wl.new_id();
    app.wl.send(app.wm, op::CREATE_POSITIONER, &[Arg::U(pos)]);
    app.wl
        .send(pos, op::POS_SET_SIZE, &[Arg::I(100), Arg::I(60)]);
    app.wl.send(
        pos,
        op::POS_ANCHOR_RECT,
        &[Arg::I(20), Arg::I(20), Arg::I(1), Arg::I(1)],
    );
    app.wl.send(pos, op::POS_ANCHOR, &[Arg::U(5)]);
    app.wl.send(pos, op::POS_GRAVITY, &[Arg::U(8)]);
    let surface = app.wl.new_id();
    app.wl
        .send(app.compositor, op::CREATE_SURFACE, &[Arg::U(surface)]);
    let xdg = app.wl.new_id();
    app.wl
        .send(app.wm, op::GET_XDG_SURFACE, &[Arg::U(xdg), Arg::U(surface)]);
    let popup = app.wl.new_id();
    app.wl.send(
        xdg,
        op::GET_POPUP,
        &[Arg::U(popup), Arg::U(win.xdg), Arg::U(pos)],
    );
    app.wl.send(surface, op::COMMIT, &[]);
    assert!(app.until(&mut h, T, |w| w.has(xdg, 0)), "popup configure");
    let cfg = app.wl.take(popup, 0);
    let mut a = Args::of(&cfg[0]);
    assert_eq!((a.i(), a.i(), a.i(), a.i()), (20, 20, 100, 60));
    let serial = Args::of(&app.wl.take(xdg, 0)[0]).u();
    app.wl.send(xdg, op::ACK, &[Arg::U(serial)]);
    app.paint_surface(surface, (100, 60), 0xFFEE_8800);
    let (mx0, my0, _, _) = mapped(&mut h, &mut app, 0xFFEE_8800, 100 * 60 * 8 / 10);
    assert_eq!((mx0 - x0, my0 - y0), (20, 20), "placed at the anchor");
    h.shot("popup");
    let ptr = app.pointer;
    app.wl
        .send(popup, op::POPUP_GRAB, &[Arg::U(app.seat), Arg::U(1)]);
    app.roundtrip(&mut h);
    h.click(f64::from(x0 + 250), f64::from(y0 + 180));
    assert!(
        app.until(&mut h, T, |w| w.has(popup, 1)),
        "popup_done on an outside click"
    );
    let _ = app.wl.take(ptr, 3);
}

#[test]
fn close_request_reaches_the_app_and_exit_code_is_the_programs() {
    let mut h = Harness::start("close", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    app.input();
    let win = app.window(&mut h, "Close", (200, 100), BLUE);
    let (x0, y0, x1, y1) = mapped(&mut h, &mut app, BLUE, 200 * 100 * 9 / 10);
    h.click(f64::from(x0 + x1) / 2.0, f64::from(y0 + y1) / 2.0);
    h.key(56, true);
    h.tap(62);
    h.key(56, false);
    let top = win.top;
    assert!(app.until(&mut h, T, |w| w.has(top, 1)), "close event");
    app.wl.send(win.top, op::DESTROY, &[]);
    app.roundtrip(&mut h);
    let gone = h.pump_until(T, |h| h.find(BLUE).0 == 0);
    assert!(gone, "window gone from the screen");
    drop(app);
    let mut h2 = Harness::start("exit", &["sh", "-c", "exit 7"]);
    assert_eq!(h2.exit_code(T), Some(7));
}
