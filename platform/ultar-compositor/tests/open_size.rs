//! A new window opens no smaller than the least size its app stated before its first frame
//! (the phone's dial pad was cut off in the usual share of the work area), never larger than
//! the work area, and limits stated later leave a shown window as it is.

mod common;

use common::*;
use std::os::unix::net::UnixStream;
use ultar_compositor::{Config, RightsTable};
use ultar_surface::proto::{Request, Rights, Role, SurfaceId};

fn open(screen: (u32, u32), min: (u32, u32)) -> (u32, u32) {
    let mut comp = compositor(test_config(), &[screen]);
    let mut a = Peer::hello(&mut comp);
    let s = SurfaceId(1);
    a.send(&Request::CreateSurface {
        surface: s,
        role: Role::Toplevel,
    });
    a.send(&Request::SetSizeLimits {
        surface: s,
        min,
        max: (0, 0),
    });
    a.pump(&mut comp);
    a.draw(&mut comp, s, 0x336699);
    a.pump(&mut comp);
    assert!(comp.state.windows[0].mapped);
    let c = comp.state.windows[0].content();
    let work = comp.state.windows[0].frame;
    assert!(work.x >= 0 && work.y >= 0, "inside the screen: {work:?}");
    assert!(
        work.max_x() <= screen.0 as i32 && work.max_y() <= screen.1 as i32,
        "inside the screen: {work:?}"
    );
    (c.w as u32, c.h as u32)
}

#[test]
fn a_window_opens_at_least_as_large_as_its_app_needs() {
    for screen in [(1280, 800), (1024, 768)] {
        // No wish: the usual share of the work area.
        let (w, h) = open(screen, (0, 0));
        assert_eq!(w, screen.0 * 3 / 5, "{screen:?}");
        assert!(h < 700, "{screen:?}: {h}");
        // Taller than the share: the window opens that tall, as wide as the share.
        assert_eq!(open(screen, (360, 700)), (w, 700), "{screen:?}");
        // Wider and taller.
        assert_eq!(open(screen, (800, 700)), (800, 700), "{screen:?}");
        // Smaller than the share changes nothing.
        assert_eq!(open(screen, (360, 320)), (w, h), "{screen:?}");
    }
}

#[test]
fn a_window_never_opens_larger_than_the_work_area() {
    let (w, h) = open((1024, 768), (4000, 4000));
    assert_eq!(w, 1024);
    assert!(h <= 768, "{h}");
    let (w, h) = open((640, 480), (800, 700));
    assert_eq!(w, 640);
    assert!(h <= 480, "{h}");
}

#[test]
fn limits_stated_after_the_first_frame_do_not_resize_the_window() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    let s = a.window(&mut comp, 1, 0x336699);
    a.pump(&mut comp);
    let before = comp.state.windows[0].frame;
    a.take();
    a.send(&Request::SetSizeLimits {
        surface: s,
        min: (900, 700),
        max: (0, 0),
    });
    a.pump(&mut comp);
    assert_eq!(comp.state.windows[0].frame, before);
    assert!(a.last_configure(s).is_none(), "no new configure");
}

/// A client that may draw its own header (the phone).
fn own_header_config() -> Config {
    let (a, _b) = UnixStream::pair().unwrap();
    let mut table = RightsTable::default();
    table.grant(
        ultar_compositor::server::peer_of(&a).uid,
        Rights::CLIENT_HEADER,
    );
    Config {
        rights: table,
        ..test_config()
    }
}

#[test]
fn a_window_with_its_own_header_opens_with_its_least_size_as_content() {
    for (min, header_first) in [((800, 700), false), ((800, 700), true), ((0, 0), false)] {
        let mut comp = compositor(own_header_config(), &[(1280, 800)]);
        let mut a = Peer::hello(&mut comp);
        let s = SurfaceId(1);
        a.send(&Request::CreateSurface {
            surface: s,
            role: Role::Toplevel,
        });
        let limits = Request::SetSizeLimits {
            surface: s,
            min,
            max: (0, 0),
        };
        let header = Request::ClientHeader {
            surface: s,
            on: true,
        };
        if header_first {
            a.send(&header);
            a.send(&limits);
        } else {
            a.send(&limits);
            a.send(&header);
        }
        a.pump(&mut comp);
        assert!(a.error().is_none());
        a.draw(&mut comp, s, 0x336699);
        a.pump(&mut comp);
        let w = &comp.state.windows[0];
        assert!(w.mapped && w.content() == w.frame, "no title bar");
        let (cw, ch) = (w.content().w, w.content().h);
        if min == (0, 0) {
            // The usual share, with the title bar's room in the content.
            assert_eq!(cw, 1280 * 3 / 5);
            assert!(ch > 400 && ch < 520, "{ch}");
        } else {
            // Exactly what the app needs: not a title bar higher.
            assert_eq!((cw, ch), (800, 700), "header first: {header_first}");
        }
        // Centred in the work area.
        assert_eq!(w.frame.x, (1280 - cw) / 2);
        assert_eq!(w.frame.y, (800 - ch) / 2);
    }
}
