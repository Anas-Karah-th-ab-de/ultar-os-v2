//! Phone mode (`SetPhoneMode`, `EdgeSwipe`, `WindowAction::Back`): only the shell may switch
//! it; app windows lose their title bar and fill the work area; a touch or mouse drag from
//! the bottom or a side edge is kept from the apps and reaches the shell clients only, never
//! while the session is locked.

mod common;

use common::*;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_hal::InputEventKind;
use ultar_surface::proto::{
    Edge, ErrorCode, Event, Request, Rights, Role, SwipeEdge, WindowAction, WindowState,
};

const SHELL_UID: u32 = 5001;
const LOCK_UID: u32 = 5002;
const APP_UID: u32 = 5003;
const W: f64 = 360.0;
const H: f64 = 800.0;

fn setup() -> (Compositor, Peer, Peer) {
    let mut rights = RightsTable::default();
    rights.grant(SHELL_UID, Rights::SHELL);
    rights.grant(LOCK_UID, Rights::LOCK_SCREEN);
    let config = Config {
        rights,
        ..test_config()
    };
    let mut comp = compositor(config, &[(360, 800)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    for (id, edge, size) in [(1, Edge::Top, 32), (2, Edge::Bottom, 44)] {
        shell.window_role(&mut comp, id, Role::Panel { edge, size }, 0x101010);
    }
    let app = Peer::hello_as(&mut comp, APP_UID);
    (comp, shell, app)
}

fn touch(comp: &mut Compositor, kind: &str, x: f64, y: f64) {
    let (nx, ny) = (
        ((x + 0.5) * 65536.0 / W) as u16,
        ((y + 0.5) * 65536.0 / H) as u16,
    );
    comp.inject(&[ev(match kind {
        "down" => InputEventKind::TouchDown {
            slot: 0,
            x: nx,
            y: ny,
        },
        "move" => InputEventKind::TouchMove {
            slot: 0,
            x: nx,
            y: ny,
        },
        _ => InputEventKind::TouchUp { slot: 0 },
    })]);
}

fn swipes(p: &mut Peer, comp: &mut Compositor) -> Vec<(SwipeEdge, bool)> {
    p.pump(comp);
    p.take()
        .into_iter()
        .filter_map(|e| match e {
            Event::EdgeSwipe { edge, held } => Some((edge, held)),
            _ => None,
        })
        .collect()
}

fn touched(p: &mut Peer, comp: &mut Compositor) -> bool {
    p.pump(comp);
    p.take()
        .iter()
        .any(|e| matches!(e, Event::TouchDown { .. } | Event::TouchUp { .. }))
}

#[test]
fn only_the_shell_switches_it_and_windows_fill_the_work_area_without_a_title_bar() {
    let (mut comp, mut shell, mut app) = setup();
    let s = app.window(&mut comp, 1, 0x3366ff);
    let before = comp.state.windows[0].content();
    let mut other = Peer::hello_as(&mut comp, APP_UID + 1);
    other.send(&Request::SetPhoneMode { on: true });
    other.pump(&mut comp);
    assert_eq!(other.error(), Some(ErrorCode::Permission));
    assert!(!comp.state.session.shell.phone.on);
    shell.send(&Request::SetPhoneMode { on: true });
    shell.pump(&mut comp);
    assert!(comp.state.session.shell.phone.on && shell.error().is_none());
    app.pump(&mut comp);
    let c = app.last_configure(s).unwrap();
    assert_eq!(c.state, WindowState::Maximized);
    assert_eq!(
        (c.width, c.height),
        (360, 800 - 32 - 44),
        "the whole work area"
    );
    assert!(!comp.state.windows[0].decorated());
    assert_ne!(before.h as u32, c.height);
    // "Maximise" does nothing: it stays.
    let id = comp.state.windows[0].id;
    shell.send(&Request::WindowAction {
        window: id,
        action: WindowAction::ToggleMaximize,
    });
    shell.pump(&mut comp);
    assert_eq!(comp.state.windows[0].state, WindowState::Maximized);
    // A new window comes the same way.
    let t = app.window(&mut comp, 2, 0x00ff00);
    app.pump(&mut comp);
    assert_eq!(app.last_configure(t).unwrap().state, WindowState::Maximized);
    // Off: title bars again, back to normal.
    shell.send(&Request::SetPhoneMode { on: false });
    shell.pump(&mut comp);
    assert!(comp.state.windows.iter().all(|w| w.decorated()));
    assert!(comp
        .state
        .windows
        .iter()
        .all(|w| w.state == WindowState::Normal));
}

#[test]
fn edge_swipes_reach_the_shell_only_and_the_apps_see_nothing() {
    let (mut comp, mut shell, mut app) = setup();
    app.window(&mut comp, 1, 0x3366ff);
    // Not in phone mode: an edge touch is an ordinary touch.
    touch(&mut comp, "down", 180.0, 795.0);
    touch(&mut comp, "up", 180.0, 795.0);
    assert!(swipes(&mut shell, &mut comp).is_empty());
    shell.send(&Request::SetPhoneMode { on: true });
    shell.pump(&mut comp);
    app.pump(&mut comp);
    app.take();
    // Up from the bottom edge: home.
    touch(&mut comp, "down", 180.0, 795.0);
    touch(&mut comp, "move", 180.0, 700.0);
    touch(&mut comp, "up", 180.0, 700.0);
    assert_eq!(
        swipes(&mut shell, &mut comp),
        vec![(SwipeEdge::Bottom, false)]
    );
    assert!(!touched(&mut app, &mut comp), "the app saw none of it");
    // Up and held: recent apps.
    touch(&mut comp, "down", 180.0, 795.0);
    touch(&mut comp, "move", 180.0, 650.0);
    comp.state.now += 500;
    touch(&mut comp, "up", 180.0, 650.0);
    assert_eq!(
        swipes(&mut shell, &mut comp),
        vec![(SwipeEdge::Bottom, true)]
    );
    // Too short: nothing.
    touch(&mut comp, "down", 180.0, 795.0);
    touch(&mut comp, "move", 180.0, 780.0);
    touch(&mut comp, "up", 180.0, 780.0);
    assert!(swipes(&mut shell, &mut comp).is_empty());
    // Inwards from the start or end edge: back; outwards: nothing.
    for (from, to, expected) in [
        (2.0, 120.0, true),
        (357.0, 200.0, true),
        (357.0, 359.0, false),
    ] {
        touch(&mut comp, "down", from, 400.0);
        touch(&mut comp, "move", to, 400.0);
        touch(&mut comp, "up", to, 400.0);
        let got = swipes(&mut shell, &mut comp);
        assert_eq!(
            got == vec![(SwipeEdge::Side, false)],
            expected,
            "{from} → {to}"
        );
    }
    assert!(!touched(&mut app, &mut comp));
    // The middle of the screen is the app's.
    let content = comp.state.windows[0].content();
    touch(&mut comp, "down", 180.0, f64::from(content.y + 100));
    touch(&mut comp, "up", 180.0, f64::from(content.y + 100));
    assert!(touched(&mut app, &mut comp));
}

#[test]
fn back_gives_the_window_escape() {
    let (mut comp, mut shell, mut app) = setup();
    let s = app.window(&mut comp, 1, 0x3366ff);
    let id = comp.state.windows[0].id;
    app.pump(&mut comp);
    app.take();
    shell.send(&Request::WindowAction {
        window: id,
        action: WindowAction::Back,
    });
    shell.pump(&mut comp);
    app.pump(&mut comp);
    let keys: Vec<(u16, u8)> = app
        .take()
        .into_iter()
        .filter_map(|e| match e {
            Event::Key {
                surface,
                code,
                state,
                ..
            } if surface == s => Some((code, state)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![(1, 1), (1, 0)], "Escape pressed and released");
    // Not for others.
    app.send(&Request::WindowAction {
        window: id,
        action: WindowAction::Back,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

#[test]
fn a_mouse_drag_works_the_same_and_nothing_while_locked() {
    let (mut comp, mut shell, mut app) = setup();
    app.window(&mut comp, 1, 0x3366ff);
    shell.send(&Request::SetPhoneMode { on: true });
    shell.pump(&mut comp);
    // A mouse drag from the bottom edge works the same.
    move_to(&mut comp, 180.0, 798.0);
    let press = |pressed| InputEventKind::PointerButton {
        button: ultar_hal::PointerButton::Primary,
        pressed,
    };
    comp.inject(&[ev(press(true))]);
    move_to(&mut comp, 180.0, 600.0);
    comp.inject(&[ev(press(false))]);
    assert_eq!(
        swipes(&mut shell, &mut comp),
        vec![(SwipeEdge::Bottom, false)]
    );
    // Locked: the lock screen keeps every contact; no gesture.
    let _lock = Peer::hello_as(&mut comp, LOCK_UID);
    key(&mut comp, 125, true);
    key(&mut comp, 38, true);
    key(&mut comp, 38, false);
    key(&mut comp, 125, false);
    assert!(comp.state.locked());
    touch(&mut comp, "down", 180.0, 795.0);
    touch(&mut comp, "move", 180.0, 600.0);
    touch(&mut comp, "up", 180.0, 600.0);
    assert!(swipes(&mut shell, &mut comp).is_empty());
}
