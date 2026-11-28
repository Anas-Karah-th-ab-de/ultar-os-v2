//! The incoming-call screen (`Role::CallScreen`, right `ui.call-screen`): shown above the lock
//! screen as a band at the top, answerable there by pointer or touch while locked, but never
//! given keys, never the lock's entry area below the band and never able to unlock. Without the
//! right the role is refused.

mod common;

use common::*;
use std::os::unix::net::UnixStream;
use ultar_compositor::{Config, RightsTable};
use ultar_surface::proto::{ErrorCode, Event, OutputId, Request, Rights, Role, SurfaceId};

fn my_uid() -> u32 {
    let (a, _b) = UnixStream::pair().unwrap();
    ultar_compositor::server::peer_of(&a).uid
}

fn config_with(rights: Rights) -> Config {
    let mut table = RightsTable::default();
    table.grant(my_uid(), rights);
    Config {
        rights: table,
        ..test_config()
    }
}

fn layer(
    p: &mut Peer,
    comp: &mut ultar_compositor::Compositor,
    id: u32,
    role: Role,
    color: u32,
) -> SurfaceId {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    let c = p.last_configure(s).expect("layer configured");
    p.fill(comp, s, c.serial, (c.width, c.height), color);
    s
}

fn pixel(comp: &ultar_compositor::Compositor, x: u32, y: u32) -> u32 {
    let f = comp.frame(OutputId(0)).unwrap();
    f.data()[(y * f.width() + x) as usize] & 0xff_ffff
}

#[test]
fn call_screen_needs_its_right() {
    let mut comp = compositor(config_with(Rights::LOCK_SCREEN), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    app.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::CallScreen,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert!(ultar_compositor::config::PERMISSION_NAMES
        .iter()
        .any(|(name, r)| *name == "ui.call-screen" && *r == Rights::CALL_SCREEN));
}

/// A locked desktop (app window, lock screen) with a call screen on top.
struct Locked {
    comp: ultar_compositor::Compositor,
    lock: Peer,
    phone: Peer,
    call: SurfaceId,
    _app: Peer,
}

fn locked_with_call() -> Locked {
    let rights = Rights::LOCK_SCREEN.union(Rights::CALL_SCREEN);
    let mut comp = compositor(config_with(rights), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0xff0000);
    let mut lock = Peer::hello(&mut comp);
    layer(&mut lock, &mut comp, 1, Role::Lock, 0x00ff00);
    assert!(comp.state.locked());
    comp.render_now();
    assert_eq!(
        pixel(&comp, 400, 300),
        0x00ff00,
        "lock screen covers the app"
    );
    let mut phone = Peer::hello(&mut comp);
    let call = layer(&mut phone, &mut comp, 1, Role::CallScreen, 0x0000ff);
    Locked {
        comp,
        lock,
        phone,
        call,
        _app: app,
    }
}

fn pointer_buttons(p: &mut Peer) -> usize {
    p.take()
        .iter()
        .filter(|e| matches!(e, Event::PointerButton { .. }))
        .count()
}

#[test]
fn call_screen_is_a_band_above_the_lock() {
    let Locked {
        mut comp,
        phone,
        call,
        ..
    } = locked_with_call();
    let band = phone.last_configure(call).unwrap();
    assert_eq!(
        (band.width, band.height),
        (
            800,
            600 * ultar_compositor::state::CALL_BAND_PERCENT as u32 / 100
        ),
        "while locked only a band at the top"
    );
    comp.render_now();
    assert_eq!(
        pixel(&comp, 400, 60),
        0x0000ff,
        "call screen above the lock screen"
    );
    assert_eq!(
        pixel(&comp, 400, 300),
        0x00ff00,
        "the lock's entry area stays visible"
    );
}

#[test]
fn locked_call_screen_takes_pointer_in_the_band_only_and_no_keys() {
    let Locked {
        mut comp,
        mut lock,
        mut phone,
        ..
    } = locked_with_call();
    phone.take();
    lock.take();
    click(&mut comp, 400.0, 60.0);
    phone.pump(&mut comp);
    assert_eq!(
        pointer_buttons(&mut phone),
        2,
        "press and release in the band reach the call screen"
    );

    // below the band the lock screen gets the taps, the call screen nothing (S-5: no PIN harvest)
    click(&mut comp, 400.0, 400.0);
    phone.pump(&mut comp);
    lock.pump(&mut comp);
    assert_eq!(
        pointer_buttons(&mut phone),
        0,
        "no tap of the entry area reaches the call screen"
    );
    assert!(pointer_buttons(&mut lock) > 0, "the lock screen gets them");

    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    phone.pump(&mut comp);
    lock.pump(&mut comp);
    assert!(
        !phone.take().iter().any(|e| matches!(e, Event::Key { .. })),
        "no keys to the call screen"
    );
    assert!(
        lock.take().iter().any(|e| matches!(e, Event::Key { .. })),
        "keys stay with the lock"
    );
}

#[test]
fn call_screen_cannot_unlock_and_the_lock_returns_when_it_goes() {
    let Locked {
        mut comp,
        mut lock,
        mut phone,
        call,
        ..
    } = locked_with_call();
    lock.pump(&mut comp);
    phone.send(&Request::Unlock { surface: call });
    phone.pump(&mut comp);
    assert_eq!(phone.error(), Some(ErrorCode::Permission));
    assert!(comp.state.locked(), "a call screen never unlocks");

    // When the call screen goes, the lock screen shows again.
    phone.send(&Request::DestroySurface { surface: call });
    phone.pump(&mut comp);
    lock.pump(&mut comp);
    comp.render_now();
    assert_eq!(pixel(&comp, 400, 300), 0x00ff00);
}

#[test]
fn client_header_needs_its_right_and_drops_the_title_bar() {
    let mut comp = compositor(config_with(Rights::NONE), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    app.send(&Request::ClientHeader {
        surface: s,
        on: true,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert!(comp.state.windows.iter().all(|w| w.decorated()));

    let mut comp = compositor(config_with(Rights::CLIENT_HEADER), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    let before = app.last_configure(s).unwrap();
    app.send(&Request::ClientHeader {
        surface: s,
        on: true,
    });
    app.pump(&mut comp);
    assert!(app.error().is_none());
    let w = &comp.state.windows[0];
    assert!(
        !w.decorated() && w.content() == w.frame,
        "no compositor title bar"
    );
    let after = app.last_configure(s).unwrap();
    assert!(
        after.serial > before.serial && after.height > before.height,
        "content grew by the bar"
    );
    // The compositor still closes it with its own keys (Alt+F4).
    app.take();
    key(&mut comp, 56, true);
    key(&mut comp, 62, true);
    key(&mut comp, 62, false);
    key(&mut comp, 56, false);
    app.pump(&mut comp);
    assert!(app
        .take()
        .iter()
        .any(|e| matches!(e, Event::CloseRequested { .. })));
}

#[test]
fn client_header_round_trips_and_refuses_bad_values() {
    use ultar_surface::proto::Request as R;
    let r = R::ClientHeader {
        surface: SurfaceId(3),
        on: true,
    };
    assert!(R::decode(42, &[3, 0, 0, 0, 1]).is_ok_and(|x| x == r));
    assert!(R::decode(42, &[3, 0, 0, 0, 2]).is_err());
    assert_eq!(Rights::ALL.0, 1023);
}

/// Security review S-5: unlocked, a call screen never covers a system dialog (a permission
/// or password prompt stays visible above it).
#[test]
fn an_unlocked_call_screen_stays_below_system_dialogs() {
    let mut comp = compositor(
        config_with(Rights(Rights::CALL_SCREEN.0 | Rights::SYSTEM_DIALOG.0)),
        &[(800, 600)],
    );
    let mut phone = Peer::hello(&mut comp);
    layer(&mut phone, &mut comp, 1, Role::CallScreen, 0xff0000);
    let mut dialog = Peer::hello(&mut comp);
    dialog.window_role(&mut comp, 1, Role::SystemDialog, 0xffffff);
    comp.render_now();
    let w = comp.state.windows.iter().find(|w| w.secure).unwrap();
    let c = w.content();
    let at = pixel(&comp, (c.x + c.w / 2) as u32, (c.y + c.h / 2) as u32);
    assert_eq!(at, 0xffffff, "the dialog is above the call screen");
}
