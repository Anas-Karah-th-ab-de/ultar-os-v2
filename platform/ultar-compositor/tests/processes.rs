//! App processes against an in-process compositor: two separate client
//! processes, an `ultar-ui` app, pixel snapshots, focus switching, hostile
//! clients, crashes (kill -9, death mid-frame) and a compositor restart.

mod common;

use common::*;
use std::time::Duration;
use ultar_compositor::{Config, Limits};
use ultar_hal::InputEventKind;
use ultar_surface::proto::{OutputId, Rights};

const WAIT: Duration = Duration::from_secs(20);

fn mapped(comp: &ultar_compositor::Compositor) -> usize {
    comp.state.windows.iter().filter(|w| w.mapped).count()
}

#[test]
fn two_processes_snapshot_focus_and_keys() {
    let mut comp = compositor(test_config(), &[(1024, 640)]);
    let dir = temp_dir("two");
    let socket = listen(&mut comp, &dir);
    let s = socket.to_str().unwrap();
    let mut red = spawn(testclient(), &[s, "solid", "c03030", "Red"]);
    assert!(
        red.wait_line(&mut comp, "drawn", WAIT),
        "red drawn: {:?}",
        red.seen
    );
    let mut blue = spawn(testclient(), &[s, "solid", "3050c0", "Blue"]);
    assert!(blue.wait_line(&mut comp, "drawn", WAIT));
    assert!(pump_until(&mut comp, WAIT, |c| mapped(c) == 2));
    // Blue opened inactive (no user action on it); the user clicks its
    // title bar.
    assert_eq!(comp.state.focus, Some(comp.state.windows[0].key));
    let fb = comp.state.windows[1].frame;
    click(&mut comp, f64::from(fb.x + fb.w / 2), f64::from(fb.y + 10));
    assert!(blue.wait_line(&mut comp, "focus 1", WAIT));
    comp.render_now();
    let frame = comp.frame(OutputId(0)).unwrap().clone();
    assert_snapshot("two_windows_en", &frame);
    // Blue has focus; click on red's visible part.
    let red_key = comp.state.windows[0].key;
    let wr = comp.state.windows[0].content();
    click(&mut comp, f64::from(wr.x + 4), f64::from(wr.y + 4));
    assert!(red.wait_line(&mut comp, "focus 1", WAIT));
    assert!(blue.wait_line(&mut comp, "focus 0", WAIT));
    assert_eq!(comp.state.focus, Some(red_key));
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    assert!(
        red.wait_line(&mut comp, "key 30 1 1588", WAIT),
        "arabic shin to the focused process: {:?}",
        red.seen
    );
    assert!(!blue.seen.iter().any(|l| l.starts_with("key")));
    comp.render_now();
    let reference = comp.render_reference(OutputId(0)).unwrap();
    assert_eq!(
        comp.frame(OutputId(0)).unwrap(),
        &reference,
        "incremental = full after focus switch"
    );
}

#[test]
fn ultar_ui_app_in_its_own_process() {
    let mut comp = compositor(test_config(), &[(1024, 640)]);
    let dir = temp_dir("ui");
    let socket = listen(&mut comp, &dir);
    let mut app = spawn(testclient(), &[socket.to_str().unwrap(), "ui", "Toolkit"]);
    assert!(app.wait_line(&mut comp, "drawn", WAIT), "{:?}", app.seen);
    assert!(pump_until(&mut comp, WAIT, |c| mapped(c) == 1));
    // Let the app finish its first frame, then compose.
    for _ in 0..20 {
        comp.dispatch(Some(Duration::from_millis(5))).unwrap();
    }
    comp.render_now();
    assert_snapshot("ui_app_en", comp.frame(OutputId(0)).unwrap());
    // Click the button (first child of the card, top of the content).
    let c = comp.state.windows[0].content();
    let hit = (16..120).step_by(4).find(|&dy| {
        click(&mut comp, f64::from(c.x + 30), f64::from(c.y + dy));
        app.wait_line(&mut comp, "activated", Duration::from_millis(300))
    });
    assert!(hit.is_some(), "button reachable by pointer: {:?}", app.seen);
}

#[test]
fn hostile_clients_cannot_touch_others() {
    let limits = Limits {
        out_bytes: 64 << 10,
        ..Limits::default()
    };
    let mut comp = compositor(
        Config {
            limits,
            ..test_config()
        },
        &[(800, 600)],
    );
    let dir = temp_dir("hostile");
    let socket = listen(&mut comp, &dir);
    let s = socket.to_str().unwrap();
    let mut victim = spawn(testclient(), &[s, "solid", "20a020", "Victim"]);
    assert!(victim.wait_line(&mut comp, "drawn", WAIT));
    let before = comp.state.windows[0].frame;
    let victim_key = comp.state.windows[0].key;
    // Using the victim's surface id from another process: its own namespace.
    let mut thief = spawn(testclient(), &[s, "steal", "1"]);
    assert!(
        thief.wait_line(&mut comp, "refused UnknownObject", WAIT),
        "{:?}",
        thief.seen
    );
    // Screenshots without the right.
    let mut spy = spawn(testclient(), &[s, "capture"]);
    assert!(
        spy.wait_line(&mut comp, "refused Permission", WAIT),
        "{:?}",
        spy.seen
    );
    // Flooding: the compositor stays responsive; the victim still works.
    let mut flood = spawn(testclient(), &[s, "flood"]);
    assert!(flood.wait_line(&mut comp, "flooding", WAIT));
    let wv = comp.state.windows[0].content();
    click(&mut comp, f64::from(wv.x + 3), f64::from(wv.y + 3));
    assert!(
        victim.wait_line(&mut comp, "button 272 1", WAIT),
        "victim served during flood"
    );
    let victim = comp.state.windows.iter().find(|w| w.key == victim_key);
    assert_eq!(
        victim.map(|w| w.frame),
        Some(before),
        "the victim's window is untouched"
    );
}

#[test]
fn a_client_that_never_reads_is_dropped() {
    let limits = Limits {
        out_bytes: 64 << 10,
        ..Limits::default()
    };
    let mut comp = compositor(
        Config {
            limits,
            ..test_config()
        },
        &[(800, 600)],
    );
    let dir = temp_dir("deaf");
    let socket = listen(&mut comp, &dir);
    let s = socket.to_str().unwrap();
    let mut other = spawn(testclient(), &[s, "solid", "20a020", "Other"]);
    assert!(other.wait_line(&mut comp, "drawn", WAIT));
    // A client that never reads is dropped once its queue is full.
    let mut deaf = spawn(testclient(), &[s, "noread"]);
    assert!(deaf.wait_line(&mut comp, "drawn", WAIT));
    let deaf_client = comp.state.windows.last().unwrap().key.client;
    let target = comp.state.windows.last().unwrap().content();
    let mut dropped = false;
    for i in 0..20_000 {
        move_to(
            &mut comp,
            f64::from(target.x + 1 + (i % 50)),
            f64::from(target.y + 1 + (i % 30)),
        );
        comp.dispatch(Some(Duration::ZERO)).unwrap();
        if !comp.state.clients.contains_key(&deaf_client) {
            dropped = true;
            break;
        }
    }
    assert!(dropped, "slow client dropped");
    assert_eq!(comp.state.clients.len(), 1, "the other client stays");
}

#[test]
fn capture_with_the_capture_permission() {
    let mut rights = ultar_compositor::RightsTable::default();
    // Tests run as one user: grant capture to it.
    rights.grant(current_uid(), Rights::CAPTURE);
    let mut comp = compositor(
        Config {
            rights,
            ..test_config()
        },
        &[(320, 200)],
    );
    let dir = temp_dir("capture");
    let socket = listen(&mut comp, &dir);
    let mut tool = spawn(testclient(), &[socket.to_str().unwrap(), "capture"]);
    assert!(
        tool.wait_line(&mut comp, "captured 320 200", WAIT),
        "{:?}",
        tool.seen
    );
}

fn current_uid() -> u32 {
    let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

#[test]
fn crashes_never_take_the_compositor_down() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let dir = temp_dir("crash");
    let socket = listen(&mut comp, &dir);
    let s = socket.to_str().unwrap();
    let mut keep = spawn(testclient(), &[s, "solid", "a0a020", "Keep"]);
    assert!(keep.wait_line(&mut comp, "drawn", WAIT));
    let mut doomed = spawn(testclient(), &[s, "solid", "a020a0", "Doomed"]);
    assert!(doomed.wait_line(&mut comp, "drawn", WAIT));
    assert!(pump_until(&mut comp, WAIT, |c| mapped(c) == 2));
    // kill -9 while it owns the focus and a grab.
    let wd = comp.state.windows[1].content();
    move_to(&mut comp, f64::from(wd.x + 5), f64::from(wd.y + 5));
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: ultar_hal::PointerButton::Primary,
        pressed: true,
    })]);
    doomed.kill();
    assert!(
        pump_until(&mut comp, WAIT, |c| mapped(c) == 1),
        "window of the killed process removed"
    );
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: ultar_hal::PointerButton::Primary,
        pressed: false,
    })]);
    assert_eq!(
        comp.state.focus.map(|k| k.client),
        Some(comp.state.windows[0].key.client)
    );
    // Death in the middle of a frame (half a message written).
    let mut half = spawn(testclient(), &[s, "midframe"]);
    assert!(half.wait_line(&mut comp, "half", WAIT));
    assert!(
        pump_until(&mut comp, WAIT, |c| c.state.clients.len() == 1),
        "half-frame client gone"
    );
    comp.render_now();
    let reference = comp.render_reference(OutputId(0)).unwrap();
    assert_eq!(comp.frame(OutputId(0)).unwrap(), &reference);
    let wk = comp.state.windows[0].content();
    click(&mut comp, f64::from(wk.x + 5), f64::from(wk.y + 5));
    assert!(
        keep.wait_line(&mut comp, "button 272 1", WAIT),
        "survivor still served"
    );
}

#[test]
fn clients_reconnect_after_a_compositor_restart() {
    let dir = temp_dir("restart");
    let socket = dir.join("socket");
    let s = socket.to_str().unwrap();
    let bin = env!("CARGO_BIN_EXE_ultar-compositor");
    let args = [
        "--socket",
        s,
        "--headless",
        "640x480",
        "--inline",
        "--allow-root",
        "--lang",
        "en",
    ];
    let mut first = spawn(bin, &args);
    let mut comp = compositor(test_config(), &[(64, 64)]);
    assert!(first.wait_line(&mut comp, "", WAIT), "compositor up");
    let mut app = spawn(testclient(), &[s, "solid", "306090", "Survivor"]);
    assert!(app.wait_line(&mut comp, "drawn", WAIT), "{:?}", app.seen);
    first.kill();
    assert!(app.wait_line(&mut comp, "reconnecting", WAIT));
    let mut second = spawn(bin, &args);
    assert!(second.wait_line(&mut comp, "", WAIT));
    assert!(
        app.wait_line(&mut comp, "drawn", WAIT),
        "window shown again: {:?}",
        app.seen
    );
    second.kill();
}
