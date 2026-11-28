//! The text clipboard both ways between a Wayland app and a native app,
//! with the compositor's rules: focus and a fresh user action.

mod common;

use common::app::{op, App};
use common::native::Peer;
use common::wl::{Arg, Args};
use common::{alt_tab, mapped, Harness};
use std::os::fd::{AsFd, OwnedFd};
use std::time::Duration;
use ultar_surface::proto::{Event, Request, CLIP_TEXT_MIME};

const BLUE: u32 = 0xFF33_66CC;
const GREEN: u32 = 0xFF22_AA44;
const T: Duration = Duration::from_secs(5);
const MIME: &str = "text/plain;charset=utf-8";
const FROM_APP: &str = "مرحبا from wayland";
const FROM_PEER: &str = "سلام from ultar";

/// The app's window (big) and the peer's (small, on top).
fn setup() -> (Harness, App, Peer) {
    let mut h = Harness::start("clip", &["sleep", "30"]);
    let mut app = App::connect(&mut h);
    app.input();
    let _win = app.window(&mut h, "Clip", (500, 360), BLUE);
    mapped(&mut h, &mut app, BLUE, 500 * 360 * 9 / 10);
    let mut peer = Peer::connect(&mut h.comp);
    peer.pump(&mut h.comp);
    peer.window(&mut h.comp, (120, 80), GREEN);
    mapped(&mut h, &mut app, GREEN, 120 * 80 / 2);
    (h, app, peer)
}

/// A click on the visible part of the app; the app copies (serial of the press).
fn app_copies(h: &mut Harness, app: &mut App) -> u32 {
    let spot = h.spot(BLUE).expect("part of the app window is visible");
    h.click(spot.0, spot.1);
    let ptr = app.pointer;
    assert!(app.until(h, T, |w| w.has(ptr, 3)));
    let press = Args::of(app.wl.take(ptr, 3).first().unwrap()).u();
    let source = app.wl.new_id();
    app.wl.fd_events.push((source, 1));
    app.wl.send(app.ddm, op::CREATE_SOURCE, &[Arg::U(source)]);
    app.wl.send(source, op::SOURCE_OFFER, &[Arg::S(MIME)]);
    app.wl.send(
        app.device,
        op::SET_SELECTION,
        &[Arg::U(source), Arg::U(press)],
    );
    app.roundtrip(h);
    h.pump(50);
    source
}

/// Collects the data of transfers until something came.
fn peer_reads(h: &mut Harness, peer: &mut Peer) -> String {
    let mut got = Vec::new();
    for _ in 0..200 {
        peer.pump(&mut h.comp);
        h.pump(5);
        for e in peer.events.drain(..) {
            if let Event::Data { data, .. } = e {
                got.extend_from_slice(&data);
            }
        }
        if !got.is_empty() {
            break;
        }
    }
    String::from_utf8_lossy(&got).into_owned()
}

/// Answers the peer's send requests and reads the app's pipe to its end.
fn app_reads(h: &mut Harness, peer: &mut Peer, r: &OwnedFd) -> String {
    let mut sent = false;
    let mut text = Vec::new();
    rustix::fs::fcntl_setfl(r, rustix::fs::OFlags::NONBLOCK).unwrap();
    for _ in 0..400 {
        peer.pump(&mut h.comp);
        h.pump(5);
        let events: Vec<Event> = peer.events.drain(..).collect();
        for e in events {
            if let (Event::SendRequest { transfer, .. }, false) = (e, sent) {
                let data = FROM_PEER.as_bytes().to_vec();
                peer.send(&Request::SendData {
                    transfer,
                    data,
                    last: true,
                });
                sent = true;
            }
        }
        let mut buf = [0u8; 256];
        match rustix::io::read(r, &mut buf) {
            Ok(0) if !text.is_empty() => break,
            Ok(n) => text.extend_from_slice(&buf[..n]),
            Err(_) => {}
        }
    }
    String::from_utf8_lossy(&text).into_owned()
}

#[test]
fn clipboard_both_ways() {
    let (mut h, mut app, mut peer) = setup();

    // App → compositor → peer: Alt+Tab and a key press give the peer the
    // focus and a fresh user action.
    let source = app_copies(&mut h, &mut app);
    alt_tab(&mut h);
    h.tap(30);
    peer.pump(&mut h.comp);
    let press = peer.last_serial();
    peer.send(&Request::ReadSelection {
        serial: press,
        mime: CLIP_TEXT_MIME.into(),
    });
    peer.pump(&mut h.comp);
    assert!(
        app.until(&mut h, T, |w| w.has(source, 1)),
        "the app is asked for its text"
    );
    let send = app.wl.take(source, 1);
    rustix::io::write(send[0].fd.as_ref().unwrap(), FROM_APP.as_bytes()).unwrap();
    drop(send);
    assert_eq!(peer_reads(&mut h, &mut peer), FROM_APP);

    // Peer → compositor → app (the peer's press is still fresh).
    peer.send(&Request::SetSelection {
        serial: press,
        mimes: vec![CLIP_TEXT_MIME.into()],
    });
    peer.pump(&mut h.comp);
    alt_tab(&mut h);
    h.tap(30);
    let device = app.device;
    assert!(
        app.until(&mut h, T, |w| w.has(device, 5)),
        "selection offered to the app"
    );
    let offer = Args::of(app.wl.take(device, 5).last().unwrap()).u();
    assert_ne!(offer, 0);
    let (r, w) = rustix::pipe::pipe().unwrap();
    app.wl
        .send_fd(offer, op::OFFER_RECEIVE, &[Arg::S(MIME)], Some(w.as_fd()));
    drop(w);
    app.roundtrip(&mut h);
    assert_eq!(app_reads(&mut h, &mut peer, &r), FROM_PEER);
    assert!(app.wl.error().is_none());
}
