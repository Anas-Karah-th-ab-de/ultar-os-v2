//! The session starts locked (`Config::start_locked`, `--start-locked-in system`): found in
//! VMware, where the desktop was visible and usable right after boot because nothing locked it
//! before the first unlock. The desktop must never be visible or get input before the lock
//! screen took the password.

mod common;

use common::*;
use std::os::unix::net::UnixStream;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{Event, OutputId, Request, Rights, Role, SurfaceId};

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

fn locked_config() -> Config {
    let mut table = RightsTable::default();
    table.grant(my_uid(), Rights::LOCK_SCREEN.union(Rights::SHELL));
    Config {
        rights: table,
        start_locked: true,
        ..test_config()
    }
}

fn frame_contains(comp: &Compositor, color: u32) -> bool {
    let f = comp.frame(OutputId(0)).unwrap();
    f.data().iter().any(|p| p & 0xff_ffff == color)
}

#[test]
fn the_desktop_is_never_shown_before_the_first_unlock() {
    let mut comp = compositor(locked_config(), &[(320, 200)]);
    assert!(comp.state.locked(), "locked before any client is there");
    // the desktop (its bar and an app window) connects before the lock screen
    let mut desk = Peer::hello(&mut comp);
    desk.window(&mut comp, 1, 0xff0000);
    comp.render_now();
    assert!(!frame_contains(&comp, 0xff0000), "no window while locked");
    desk.take();
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    desk.pump(&mut comp);
    assert!(
        !desk.events.iter().any(|e| matches!(e, Event::Key { .. })),
        "no key reaches anyone while locked and no lock screen is there"
    );
    // the lock screen connects: it is told at once that the session is locked
    let mut lock = Peer::hello(&mut comp);
    lock.pump(&mut comp);
    assert!(lock
        .events
        .iter()
        .any(|e| matches!(e, Event::SessionLocked { locked: true })));
    let s = SurfaceId(1);
    lock.send(&Request::CreateSurface {
        surface: s,
        role: Role::Lock,
    });
    lock.pump(&mut comp);
    let serial = lock.last_configure(s).map_or(0, |c| c.serial);
    lock.fill(&mut comp, s, serial, (320, 200), 0x00ff00);
    comp.render_now();
    assert!(frame_contains(&comp, 0x00ff00), "the lock screen shows");
    assert!(!frame_contains(&comp, 0xff0000));
    // only Unlock (after the password check) shows the desktop
    lock.send(&Request::Unlock { surface: s });
    lock.pump(&mut comp);
    assert!(!comp.state.locked());
}

#[test]
fn without_the_option_the_session_starts_unlocked_as_before() {
    let comp = compositor(test_config(), &[(320, 200)]);
    assert!(!comp.state.locked());
}
