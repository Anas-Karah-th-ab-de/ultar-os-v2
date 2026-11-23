//! Regression tests of the independent security review (w4/comprev,
//! against 0f4846e). Every test here names its finding; on 0f4846e the
//! ones marked FINDING fail, the ones marked SOUND pass.

mod common;

use common::*;
use std::os::unix::net::UnixStream;
use std::time::Duration;
use ultar_compositor::conn::Peer as ConnPeer;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{
    BufferId, Edge, Event, OutputId, PixelFormat, Request, Rights, Role, SurfaceId, WindowState,
};

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

/// Creates a surface with `role` and fills it with one colour.
fn surface(
    p: &mut Peer,
    comp: &mut Compositor,
    (id, role): (u32, Role),
    size: (u32, u32),
    color: u32,
) -> SurfaceId {
    let s = SurfaceId(id);
    p.send(&Request::CreateSurface { surface: s, role });
    p.pump(comp);
    let serial = p.last_configure(s).map_or(0, |c| c.serial);
    p.fill(comp, s, serial, size, color);
    s
}

fn frame_contains(comp: &Compositor, color: u32) -> bool {
    let f = comp.frame(OutputId(0)).unwrap();
    f.data().iter().any(|p| p & 0xff_ffff == color)
}

fn pixel(comp: &Compositor, x: u32, y: u32) -> u32 {
    let f = comp.frame(OutputId(0)).unwrap();
    f.data()[(y * f.width() + x) as usize] & 0xff_ffff
}

fn tap_key(comp: &mut Compositor, code: u16) {
    key(comp, code, true);
    key(comp, code, false);
}

fn got_keys(p: &Peer) -> usize {
    p.events
        .iter()
        .filter(|e| matches!(e, Event::Key { .. }))
        .count()
}

/// FINDING C1: the lock is a property of a live surface. When the lock
/// screen process dies (crash, OOM kill, kill -9 by the same uid), the
/// desktop is unlocked: windows are shown and keys go to the app.
#[test]
fn lock_screen_stays_locked_when_its_process_dies() {
    let mut comp = compositor(config_with(Rights::LOCK_SCREEN), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0xff0000);
    let mut lock = Peer::hello(&mut comp);
    surface(&mut lock, &mut comp, (1, Role::Lock), (800, 600), 0x00ff00);
    assert!(comp.state.locked());
    comp.render_now();
    assert!(!frame_contains(&comp, 0xff0000));
    drop(lock);
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    app.pump(&mut comp);
    app.take();
    tap_key(&mut comp, 30);
    app.pump(&mut comp);
    comp.render_now();
    let shown = frame_contains(&comp, 0xff0000);
    let keys = got_keys(&app);
    assert!(
        comp.state.locked() && !shown && keys == 0,
        "C1: lock client died -> locked={} app visible={shown} keys to app={keys}",
        comp.state.locked()
    );
}

/// FINDING C2: input-method commits go to the keyboard focus, not to the
/// lock screen, while locked: a PIN typed on the on-screen keyboard at the
/// lock screen lands in the app behind it (and the lock never gets it).
#[test]
fn on_screen_keyboard_types_into_the_lock_screen_while_locked() {
    let rights = Rights::LOCK_SCREEN.union(Rights::INPUT_METHOD);
    let mut comp = compositor(config_with(rights), &[(800, 600)]);
    let mut app = Peer::hello(&mut comp);
    let s = app.window(&mut comp, 1, 0xff0000);
    app.send(&Request::TextInput {
        surface: s,
        enabled: true,
        purpose: 0,
    });
    app.pump(&mut comp);
    let mut osk = Peer::hello(&mut comp);
    surface(
        &mut osk,
        &mut comp,
        (1, Role::InputPanel),
        (800, 240),
        0x333333,
    );
    let mut lock = Peer::hello(&mut comp);
    let l = surface(&mut lock, &mut comp, (1, Role::Lock), (800, 600), 0x00ff00);
    lock.send(&Request::TextInput {
        surface: l,
        enabled: true,
        purpose: 1,
    });
    lock.pump(&mut comp);
    osk.send(&Request::ImeCommit {
        text: "4711".into(),
    });
    osk.pump(&mut comp);
    app.pump(&mut comp);
    lock.pump(&mut comp);
    let text = |p: &Peer| {
        p.events
            .iter()
            .any(|e| matches!(e, Event::Text { text, .. } if text == "4711"))
    };
    assert!(
        !text(&app) && text(&lock),
        "C2: PIN went to app={} lock={}",
        text(&app),
        text(&lock)
    );
}

/// FINDING C3: a popup of an ordinary app (no right) can be screen-sized
/// and is stacked above the shell panels: it can cover the bar/dock and
/// imitate the lock screen or a system password dialog, while the keys go
/// to the app. There is no trusted path.
#[test]
fn an_app_popup_cannot_cover_the_shell_panel() {
    let mut comp = compositor(config_with(Rights::SHELL), &[(800, 600)]);
    let mut shell = Peer::hello(&mut comp);
    let panel = Role::Panel {
        edge: Edge::Bottom,
        size: 48,
    };
    surface(&mut shell, &mut comp, (1, panel), (800, 48), 0x00ff00);
    let mut app = Peer::hello(&mut comp);
    app.window(&mut comp, 1, 0xff0000);
    let popup = Role::Popup {
        parent: SurfaceId(1),
        x: -5000,
        y: -5000,
    };
    surface(&mut app, &mut comp, (2, popup), (800, 600), 0x0000ff);
    comp.render_now();
    let seen = pixel(&comp, 400, 590);
    assert_eq!(
        seen, 0x00ff00,
        "C3: the shell panel shows {seen:06x} (app popup = 0000ff)"
    );
}

/// FINDING C3 (input side): a fully transparent screen-sized popup is
/// invisible but hit-tested by its rectangle: it takes every click meant
/// for other apps (and the shell) without the user seeing anything.
#[test]
fn an_invisible_popup_cannot_take_other_apps_clicks() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut victim = Peer::hello(&mut comp);
    victim.window(&mut comp, 1, 0x00ff00);
    let content = comp.state.windows[0].content();
    let mut spy = Peer::hello(&mut comp);
    spy.window(&mut comp, 1, 0xff0000);
    let popup = Role::Popup {
        parent: SurfaceId(1),
        x: -5000,
        y: -5000,
    };
    let s = SurfaceId(2);
    spy.send(&Request::CreateSurface {
        surface: s,
        role: popup,
    });
    spy.send(&Request::CreateInlineBuffer {
        buffer: BufferId(77),
        width: 800,
        height: 600,
        format: PixelFormat::Argb8888, // zero-filled: alpha 0 everywhere
    });
    spy.send(&Request::Attach {
        surface: s,
        buffer: BufferId(77),
    });
    spy.send(&Request::Commit {
        surface: s,
        configure: 0,
    });
    spy.pump(&mut comp);
    comp.render_now();
    assert!(!frame_contains(&comp, 0x0000ff));
    let (x, y) = (content.x + 20, content.y + 20);
    victim.take();
    spy.take();
    click(&mut comp, f64::from(x), f64::from(y));
    victim.pump(&mut comp);
    spy.pump(&mut comp);
    let to = |p: &Peer| {
        p.events
            .iter()
            .filter(|e| matches!(e, Event::PointerButton { .. }))
            .count()
    };
    assert!(
        to(&victim) > 0 && to(&spy) == 0,
        "C3: click on the victim window went to victim={} popup owner={}",
        to(&victim),
        to(&spy)
    );
}

fn focus_a_then(comp: &mut Compositor) -> (Peer, Peer) {
    let mut a = Peer::hello(comp);
    a.window(comp, 1, 0xff0000);
    let mut b = Peer::hello(comp);
    b.window(comp, 1, 0x0000ff);
    let ka = comp.state.windows[0].key;
    comp.state.focus_window(ka);
    a.pump(comp);
    b.pump(comp);
    (a, b)
}

/// FINDING M1: a window in the background takes the keyboard focus
/// without any user action by unmapping and mapping again; the user's next
/// keys (a password typed into A) go to B.
#[test]
fn a_background_window_cannot_steal_focus_by_remapping() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let (mut a, mut b) = focus_a_then(&mut comp);
    let ka = comp.state.focus;
    b.send(&Request::Attach {
        surface: SurfaceId(1),
        buffer: BufferId(0),
    });
    b.send(&Request::Commit {
        surface: SurfaceId(1),
        configure: 0,
    });
    b.pump(&mut comp);
    b.draw(&mut comp, SurfaceId(1), 0x0000ff);
    b.take();
    tap_key(&mut comp, 30);
    a.pump(&mut comp);
    b.pump(&mut comp);
    assert!(
        comp.state.focus == ka && got_keys(&b) == 0,
        "M1: focus stolen by re-map, {} key events went to B",
        got_keys(&b)
    );
}

/// FINDING M1 (second path): `SetState(Minimized)` then `SetState(Normal)`
/// from the client raises and focuses its window.
#[test]
fn a_background_window_cannot_steal_focus_by_restoring_itself() {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let (_a, mut b) = focus_a_then(&mut comp);
    let ka = comp.state.focus;
    for state in [WindowState::Minimized, WindowState::Normal] {
        b.send(&Request::SetState {
            surface: SurfaceId(1),
            state,
        });
    }
    b.pump(&mut comp);
    assert_eq!(comp.state.focus, ka, "M1: focus stolen by SetState");
}

/// FINDING M2: the client limit is global. One uid (one app, 64
/// connections) takes every slot; the shell, lock screen and every other
/// app are refused afterwards.
#[test]
fn one_uid_cannot_take_every_client_slot() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut keep = Vec::new();
    for _ in 0..comp.state.config.limits.clients {
        let (a, b) = UnixStream::pair().unwrap();
        let peer = ConnPeer {
            pid: None,
            uid: 1000,
        };
        comp.state.accept(a, peer);
        keep.push(b);
    }
    let (a, _b) = UnixStream::pair().unwrap();
    let shell = comp.state.accept(a, ConnPeer { pid: None, uid: 0 });
    assert!(shell.is_some(), "M2: uid 1000 holds every client slot");
}

/// FINDING M3: the pixel memory limit is per connection only; nothing
/// bounds the sum. 8 connections of one app commit 2 GiB of compositor
/// memory (64 connections: 16 GiB) – the compositor is OOM-killed and the
/// whole desktop with it.
#[test]
fn total_client_pixel_memory_is_bounded() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut peers = Vec::new();
    let mut accepted: u64 = 0;
    for _ in 0..8 {
        let mut p = Peer::hello(&mut comp);
        for b in 1..=4 {
            p.send(&Request::CreateInlineBuffer {
                buffer: BufferId(b),
                width: 8192,
                height: 2048,
                format: PixelFormat::Xrgb8888,
            });
        }
        p.pump(&mut comp);
        if p.error().is_none() {
            accepted += 4 * 8192 * 2048 * 4;
        }
        peers.push(p);
    }
    assert!(
        accepted <= 1 << 30,
        "M3: compositor accepted {} MiB of pixel memory",
        accepted >> 20
    );
}

/// FINDING m1: U+061C ARABIC LETTER MARK (a bidi format character) and
/// other invisible format characters survive `clean_title`.
#[test]
fn titles_lose_every_bidi_format_character() {
    let mut comp = compositor(test_config(), &[(800, 600)]);
    let mut a = Peer::hello(&mut comp);
    let s = a.window(&mut comp, 1, 0xff0000);
    a.send(&Request::SetTitle {
        surface: s,
        title: "a\u{061C}b\u{2060}c\u{FEFF}d".into(),
    });
    a.pump(&mut comp);
    let key = comp.state.windows[0].key;
    let title = comp.state.surface(key).unwrap().title.clone();
    assert_eq!(title, "abcd", "m1: format characters kept in {title:?}");
}

/// SOUND: capture needs the right; an app without it is refused and the
/// connection ends.
#[test]
fn capture_without_right_is_refused() {
    let mut comp = compositor(test_config(), &[(320, 200)]);
    let mut a = Peer::hello(&mut comp);
    a.send(&Request::Capture {
        output: OutputId(0),
    });
    a.pump(&mut comp);
    assert!(a.error().is_some());
    assert!(!a
        .events
        .iter()
        .any(|e| matches!(e, Event::CaptureData { .. })));
}
