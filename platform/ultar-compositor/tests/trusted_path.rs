//! The trusted path and the session (fix round 1 of the security review):
//! secure attention keys, unlock rules, system dialogs, activation tokens,
//! the recording indicator, key releases and reserved client slots.

mod common;

use common::*;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use ultar_compositor::backend::Headless;
use ultar_compositor::conn::Peer as ConnPeer;
use ultar_compositor::{Clock, Compositor, Config, RightsTable};
use ultar_surface::proto::{Edge, ErrorCode, Event, OutputId, Request, Rights, Role, SurfaceId};

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

fn layer(p: &mut Peer, comp: &mut Compositor, id: u32, role: Role, color: u32) -> SurfaceId {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    let c = p.last_configure(s).unwrap();
    p.fill(comp, s, c.serial, (c.width, c.height), color);
    s
}

fn pixel(comp: &Compositor, x: u32, y: u32) -> u32 {
    let f = comp.frame(OutputId(0)).unwrap();
    f.data()[(y * f.width() + x) as usize] & 0xff_ffff
}

fn keys_to(p: &mut Peer, comp: &mut Compositor) -> Vec<(u16, u8)> {
    p.pump(comp);
    p.take()
        .into_iter()
        .filter_map(|e| match e {
            Event::Key { code, state, .. } => Some((code, state)),
            _ => None,
        })
        .collect()
}

fn locked_event(p: &mut Peer, comp: &mut Compositor) -> Option<bool> {
    p.pump(comp);
    p.events.iter().rev().find_map(|e| match e {
        Event::SessionLocked { locked } => Some(*locked),
        _ => None,
    })
}

/// Ctrl+Alt+Del and Super+L lock the session; the app sees none of the
/// keys of the combination's last key, the shell is told.
#[test]
fn secure_attention_keys_lock_and_are_never_forwarded() {
    for combo in [&[29u16, 56, 111][..], &[125, 38][..]] {
        let mut comp = compositor(config_with(Rights::SHELL), &[(800, 600)]);
        let mut shell = Peer::hello(&mut comp);
        let mut app = Peer::hello(&mut comp);
        app.window(&mut comp, 1, 0xff0000);
        keys_to(&mut app, &mut comp);
        for &k in combo {
            key(&mut comp, k, true);
        }
        for &k in combo.iter().rev() {
            key(&mut comp, k, false);
        }
        assert!(comp.state.locked(), "{combo:?} locks");
        let last = *combo.last().unwrap();
        let got = keys_to(&mut app, &mut comp);
        assert!(!got.iter().any(|&(c, _)| c == last), "{combo:?}: {got:?}");
        assert_eq!(locked_event(&mut shell, &mut comp), Some(true));
        comp.render_now();
        assert_ne!(pixel(&comp, 400, 300), 0xff0000, "nothing of the app shows");
    }
}

/// Only a lock screen client that shows a lock surface unlocks; a new
/// lock client after a crash takes the keys; unlocking shows the desktop.
#[test]
fn unlock_needs_the_lock_screen_and_a_new_lock_client_takes_over() {
    let mut comp = compositor(config_with(Rights::NONE), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    key(&mut comp, 125, true);
    key(&mut comp, 38, true);
    assert!(comp.state.locked());
    app.send(&Request::Unlock { surface: s });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert!(comp.state.locked());

    let mut comp = compositor(config_with(Rights::LOCK_SCREEN), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0xff0000);
    let mut first = Peer::hello(&mut comp);
    layer(&mut first, &mut comp, 1, Role::Lock, 0x00ff00);
    drop(first);
    comp.dispatch(Some(std::time::Duration::ZERO)).unwrap();
    let mut second = Peer::hello(&mut comp);
    let l = layer(&mut second, &mut comp, 1, Role::Lock, 0x0000ff);
    second.take();
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    assert_eq!(keys_to(&mut second, &mut comp), vec![(30, 1), (30, 0)]);
    assert!(keys_to(&mut app, &mut comp).is_empty());
    second.send(&Request::Unlock { surface: l });
    second.send(&Request::DestroySurface { surface: l });
    second.pump(&mut comp);
    assert!(!comp.state.locked() && second.error().is_none());
    comp.render_now();
    assert_eq!(pixel(&comp, 400, 300), 0xff0000);
}

/// A system dialog needs its right, sits above the shell's panels, dims
/// everything below (the panel too: no app can do that), takes the keys
/// and is modal; its title bar carries the lock mark.
#[test]
fn system_dialogs_are_above_panels_dim_the_rest_and_are_modal() {
    let mut comp = compositor(config_with(Rights::SHELL), &[(800, 600)]);
    let mut p = Peer::hello(&mut comp);
    p.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::SystemDialog,
    });
    p.pump(&mut comp);
    assert_eq!(p.error(), Some(ErrorCode::Permission));

    let mut comp = compositor(config_with(Rights::ALL), &[(800, 600)]);
    let mut shell = Peer::hello(&mut comp);
    let panel = Role::Panel {
        edge: Edge::Bottom,
        size: 48,
    };
    layer(&mut shell, &mut comp, 1, panel, 0x00ff00);
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0xff0000);
    comp.render_now();
    assert_eq!(pixel(&comp, 10, 590), 0x00ff00);
    let mut dialog = Peer::hello(&mut comp);
    let d = dialog.window_role(&mut comp, 1, Role::SystemDialog, 0xffffff);
    comp.render_now();
    let dimmed = pixel(&comp, 10, 590);
    assert!(
        dimmed != 0x00ff00 && dimmed & 0xff00 < 0xff00,
        "panel dimmed: {dimmed:06x}"
    );
    let w = comp.state.windows.iter().find(|w| w.secure).unwrap();
    assert!(w.title_cache.as_ref().is_some_and(|(k, _)| k.secure));
    assert_eq!(comp.state.text_target().map(|k| k.surface), Some(d));
    dialog.take();
    key(&mut comp, 30, true);
    assert_eq!(keys_to(&mut dialog, &mut comp), vec![(30, 1)]);
    assert!(keys_to(&mut app, &mut comp).is_empty());
    // A click on the app's window (outside the dialog) goes nowhere.
    let c = comp.state.windows[0].content();
    app.take();
    click(&mut comp, f64::from(c.x + 3), f64::from(c.y + 3));
    app.pump(&mut comp);
    assert!(!app
        .events
        .iter()
        .any(|e| matches!(e, Event::PointerButton { .. })));
}

/// Presses a key and returns the serial its receiver got.
fn key_serial(p: &mut Peer, comp: &mut Compositor) -> u32 {
    key(comp, 30, true);
    p.pump(comp);
    p.events
        .iter()
        .find_map(|e| match e {
            Event::Key { serial, .. } => Some(*serial),
            _ => None,
        })
        .unwrap()
}

/// A launcher turns a click serial into an activation token; the started
/// app's window takes the focus with it (once); bad tokens do nothing.
#[test]
fn activation_tokens_move_the_focus_once() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut shell = Peer::hello(&mut comp);
    shell.window(&mut comp, 1, 0x202020);
    let serial = key_serial(&mut shell, &mut comp);
    shell.send(&Request::RequestActivation { serial });
    shell.send(&Request::RequestActivation { serial: 999_999 });
    shell.pump(&mut comp);
    let tokens: Vec<u64> = shell
        .events
        .iter()
        .filter_map(|e| match e {
            Event::ActivationToken { token } => Some(*token),
            _ => None,
        })
        .collect();
    assert!(tokens.len() == 2 && tokens[0] != 0 && tokens[1] == 0);
    let mut intruder = Peer::hello(&mut comp);
    intruder.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Toplevel,
    });
    intruder.send(&Request::Activate {
        surface: SurfaceId(1),
        token: 12345,
    });
    intruder.pump(&mut comp);
    intruder.draw(&mut comp, SurfaceId(1), 0x0000ff);
    assert_eq!(comp.state.focus.map(|k| k.client), Some(1));
    let mut app = Peer::hello(&mut comp);
    app.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role: Role::Toplevel,
    });
    app.send(&Request::Activate {
        surface: SurfaceId(1),
        token: tokens[0],
    });
    app.pump(&mut comp);
    app.draw(&mut comp, SurfaceId(1), 0x00ff00);
    let app_client = comp.state.windows.last().unwrap().key.client;
    assert_eq!(comp.state.focus.map(|k| k.client), Some(app_client));
    intruder.send(&Request::Activate {
        surface: SurfaceId(1),
        token: tokens[0],
    });
    intruder.pump(&mut comp);
    assert_eq!(
        comp.state.focus.map(|k| k.client),
        Some(app_client),
        "single use"
    );
}

/// The release (and repeats) of a key go to the surface that got the
/// press, even when the focus moved in between.
#[test]
fn a_key_release_goes_where_the_press_went() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0xff0000);
    let mut b = Peer::hello(&mut comp);
    b.window(&mut comp, 1, 0x0000ff);
    a.take();
    b.take();
    key(&mut comp, 30, true);
    let kb = comp.state.windows[1].key;
    comp.state.focus_window(kb);
    key(&mut comp, 30, false);
    assert_eq!(keys_to(&mut a, &mut comp), vec![(30, 1), (30, 0)]);
    assert!(keys_to(&mut b, &mut comp).is_empty());
}

/// A clock the test moves by hand.
struct Manual(Arc<AtomicU64>);

impl Clock for Manual {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// While the screen is captured (and 2 s after) the compositor draws the
/// recording indicator; captures pause while the session is locked.
#[test]
fn capture_shows_the_indicator_and_pauses_while_locked() {
    let time = Arc::new(AtomicU64::new(1_000));
    let mut comp = Compositor::new(
        config_with(Rights::CAPTURE),
        Box::new(Headless::new(&[(320, 200)])),
        Box::new(Manual(time.clone())),
    );
    let mut p = Peer::hello(&mut comp);
    comp.render_now();
    let r = ultar_compositor::cursor::indicator_rect(comp.state.layout.primary().unwrap(), false);
    let (x, y) = ((r.x + r.w / 2) as u32, (r.y + r.h / 2) as u32);
    let ground = pixel(&comp, x, y);
    p.send(&Request::Capture {
        output: OutputId(0),
    });
    p.pump(&mut comp);
    assert!(p
        .events
        .iter()
        .any(|e| matches!(e, Event::CaptureData { last: true, .. })));
    let red = pixel(&comp, x, y);
    assert!(red >> 16 > (red >> 8 & 0xff) + 0x40, "indicator: {red:06x}");
    time.fetch_add(2_001, Ordering::SeqCst);
    comp.dispatch(Some(std::time::Duration::ZERO)).unwrap();
    assert_eq!(pixel(&comp, x, y), ground, "indicator gone 2 s after");
    comp.state.lock();
    p.take();
    p.send(&Request::Capture {
        output: OutputId(0),
    });
    p.pump(&mut comp);
    assert!(!p
        .events
        .iter()
        .any(|e| matches!(e, Event::CaptureData { .. })));
}

/// Apps without rights fill at most the open slots; the reserved ones stay
/// for the shell, lock screen and input method.
#[test]
fn reserved_client_slots_stay_for_system_clients() {
    let mut table = RightsTable::default();
    table.grant(4242, Rights::LOCK_SCREEN);
    let config = Config {
        rights: table,
        ..test_config()
    };
    let mut comp = compositor(config, &[(800, 600)]);
    let limits = comp.state.config.limits;
    let mut keep = Vec::new();
    let mut accepted = 0;
    for uid in 1000..1000 + limits.clients as u32 {
        let (a, b) = UnixStream::pair().unwrap();
        let peer = ConnPeer { pid: None, uid };
        accepted += usize::from(comp.state.accept(a, peer).is_some());
        keep.push(b);
    }
    assert_eq!(accepted, limits.clients - limits.reserved_clients);
    let (a, _b) = UnixStream::pair().unwrap();
    let lock = ConnPeer {
        pid: None,
        uid: 4242,
    };
    assert!(comp.state.accept(a, lock).is_some());
}
