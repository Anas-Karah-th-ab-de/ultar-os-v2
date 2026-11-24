//! The desktop shell's protocol (right `ui.shell`): popups of panels,
//! the shell's keyboard, its shortcuts, the window list, window actions,
//! the display mode and keyboard layouts – and that none of it is open to
//! a client without the right.

mod common;

use common::*;
use ultar_compositor::output::DisplayMode;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{
    DisplayModeRequest, Edge, ErrorCode, Event, Modifiers, OutputId, Request, Rights, Role,
    ShellWindow, SurfaceId, WindowAction, WindowState,
};

const SHELL_UID: u32 = 5001;
const LOCK_UID: u32 = 5002;
const APP_UID: u32 = 5003;

fn config() -> Config {
    let mut rights = RightsTable::default();
    rights.grant(SHELL_UID, Rights::SHELL);
    rights.grant(LOCK_UID, Rights::LOCK_SCREEN);
    Config {
        rights,
        ..test_config()
    }
}

fn panel(shell: &mut Peer, comp: &mut Compositor) -> SurfaceId {
    let s = SurfaceId(1);
    let role = Role::Panel {
        edge: Edge::Bottom,
        size: 56,
    };
    shell.send(&Request::CreateSurface { surface: s, role });
    shell.pump(comp);
    let c = shell.last_configure(s).unwrap();
    shell.fill(comp, s, c.serial, (c.width, c.height), 0x101010);
    s
}

fn popup(
    shell: &mut Peer,
    comp: &mut Compositor,
    id: u32,
    at: (i32, i32),
    size: (u32, u32),
) -> SurfaceId {
    let s = SurfaceId(id);
    let role = Role::Popup {
        parent: SurfaceId(1),
        x: at.0,
        y: at.1,
    };
    shell.send(&Request::CreateSurface { surface: s, role });
    shell.pump(comp);
    let serial = shell.last_configure(s).map_or(0, |c| c.serial);
    shell.fill(comp, s, serial, size, 0x00aa00);
    s
}

fn pixel(comp: &mut Compositor, x: u32, y: u32) -> u32 {
    comp.render_now();
    let f = comp.frame(OutputId(0)).unwrap();
    f.data()[(y * f.width() + x) as usize] & 0xff_ffff
}

fn windows(p: &mut Peer, comp: &mut Compositor) -> Option<Vec<ShellWindow>> {
    p.pump(comp);
    p.events.iter().rev().find_map(|e| match e {
        Event::WindowsChanged { windows } => Some(windows.clone()),
        _ => None,
    })
}

fn shell_keys(p: &mut Peer, comp: &mut Compositor) -> Vec<(u16, u8)> {
    p.pump(comp);
    p.take()
        .into_iter()
        .filter_map(|e| match e {
            Event::ShellKey {
                code, modifiers, ..
            } => Some((code, modifiers.0)),
            _ => None,
        })
        .collect()
}

fn app_keys(p: &mut Peer, comp: &mut Compositor) -> Vec<(u16, u8)> {
    p.pump(comp);
    p.take()
        .into_iter()
        .filter_map(|e| match e {
            Event::Key { code, state, .. } => Some((code, state)),
            _ => None,
        })
        .collect()
}

/// A sheet of the bar is drawn above the windows, at its offset from the
/// panel, and never leaves the screen.
#[test]
fn panel_popups_float_above_windows_and_stay_on_screen() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    panel(&mut shell, &mut comp);
    // The bar is at y 544..600; the sheet 300 × 200 ends 8 above it.
    popup(&mut shell, &mut comp, 2, (500, -208), (300, 200));
    assert_eq!(pixel(&mut comp, 510, 340), 0x00aa00);
    assert_ne!(pixel(&mut comp, 510, 330), 0x00aa00);
    // Far away: clamped to the screen.
    popup(&mut shell, &mut comp, 3, (5000, -5000), (100, 100));
    assert_eq!(pixel(&mut comp, 750, 50), 0x00aa00);
    assert!(shell.error().is_none());
}

/// Only a shell client may attach popups to a panel, and only to its own.
#[test]
fn panel_popups_need_the_shell_right_and_an_own_panel() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    let role = Role::Panel {
        edge: Edge::Bottom,
        size: 56,
    };
    app.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    panel(&mut shell, &mut comp);
    let mut other = Peer::hello_as(&mut comp, APP_UID);
    let role = Role::Popup {
        parent: SurfaceId(1),
        x: 0,
        y: 0,
    };
    other.send(&Request::CreateSurface {
        surface: SurfaceId(2),
        role,
    });
    other.pump(&mut comp);
    assert_eq!(other.error(), Some(ErrorCode::UnknownObject));
}

/// An open sheet gets the keys; a click on a window or a lock takes the
/// keyboard back and tells the shell.
#[test]
fn shell_focus_takes_keys_until_the_user_clicks_elsewhere() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    app_keys(&mut app, &mut comp);
    panel(&mut shell, &mut comp);
    let sheet = popup(&mut shell, &mut comp, 2, (500, -208), (300, 200));
    shell.send(&Request::ShellFocus { surface: sheet });
    shell.pump(&mut comp);
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    assert!(
        app_keys(&mut app, &mut comp).is_empty(),
        "the app saw a key"
    );
    shell.pump(&mut comp);
    let got: Vec<_> = shell
        .take()
        .into_iter()
        .filter(|e| matches!(e, Event::Key { surface, .. } if *surface == sheet))
        .collect();
    assert_eq!(got.len(), 2);
    // A click inside the sheet keeps it; a click on the window ends it.
    click(&mut comp, 600.0, 400.0);
    shell.pump(&mut comp);
    assert!(!shell
        .events
        .iter()
        .any(|e| matches!(e, Event::ShellFocusLost { .. })));
    let window = comp.state.windows[0].content();
    click(&mut comp, f64::from(window.x + 5), f64::from(window.y + 5));
    shell.pump(&mut comp);
    assert!(shell
        .events
        .contains(&Event::ShellFocusLost { surface: sheet }));
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    assert_eq!(app_keys(&mut app, &mut comp), vec![(30, 1), (30, 0)]);
    // Giving the keyboard back by itself, and refusals.
    shell.send(&Request::ShellFocus { surface: sheet });
    shell.send(&Request::ShellFocus {
        surface: SurfaceId(0),
    });
    shell.pump(&mut comp);
    assert!(comp.state.session.shell.focus.is_none());
    app.send(&Request::ShellFocus {
        surface: SurfaceId(1),
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

/// A shell surface that is not a panel or panel popup cannot take keys.
#[test]
fn shell_focus_refuses_other_surfaces() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    shell.window(&mut comp, 7, 0x0000ff);
    shell.send(&Request::ShellFocus {
        surface: SurfaceId(7),
    });
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::UnknownObject));
}

/// Super alone and Super chords the compositor does not use go to the
/// shell – never to the app; Super+arrows stay the compositor's.
#[test]
fn super_shortcuts_go_to_the_shell_only() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    panel(&mut shell, &mut comp);
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    shell_keys(&mut shell, &mut comp);
    app_keys(&mut app, &mut comp);
    // Super tap.
    key(&mut comp, 125, true);
    key(&mut comp, 125, false);
    assert_eq!(shell_keys(&mut shell, &mut comp), vec![(125, 0)]);
    // Super+P: P and its release never reach the app.
    key(&mut comp, 125, true);
    key(&mut comp, 25, true);
    key(&mut comp, 25, false);
    key(&mut comp, 125, false);
    assert_eq!(
        shell_keys(&mut shell, &mut comp),
        vec![(25, Modifiers::SUPER)]
    );
    let seen = app_keys(&mut app, &mut comp);
    assert!(!seen.iter().any(|(c, _)| *c == 25), "{seen:?}");
    // Super+Left snaps (compositor), no shell key, no tap.
    key(&mut comp, 125, true);
    key(&mut comp, 105, true);
    key(&mut comp, 105, false);
    key(&mut comp, 125, false);
    assert!(shell_keys(&mut shell, &mut comp).is_empty());
    assert!(comp.state.windows[0].snap.is_some());
}

/// The bar learns about every window and can act on any of them; an app
/// cannot.
#[test]
fn window_list_and_actions_for_the_shell() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    panel(&mut shell, &mut comp);
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    app.window(&mut comp, 2, 0x00ff00);
    let list = windows(&mut shell, &mut comp).unwrap();
    assert_eq!(list.len(), 2);
    assert_ne!(list[0].id, list[1].id);
    assert_eq!(list[0].app_id, "test");
    let (first, second) = (list[0].id, list[1].id);
    shell.send(&Request::WindowAction {
        window: first,
        action: WindowAction::Minimize,
    });
    let list = windows(&mut shell, &mut comp).unwrap();
    assert_eq!(list[0].state, WindowState::Minimized);
    shell.send(&Request::WindowAction {
        window: first,
        action: WindowAction::Activate,
    });
    let list = windows(&mut shell, &mut comp).unwrap();
    let w = list.iter().find(|w| w.id == first).unwrap();
    assert!(w.focused && w.state == WindowState::Normal);
    shell.send(&Request::WindowAction {
        window: second,
        action: WindowAction::ToggleMaximize,
    });
    shell.send(&Request::WindowAction {
        window: second,
        action: WindowAction::Close,
    });
    shell.send(&Request::WindowAction {
        window: 9999,
        action: WindowAction::Close,
    });
    shell.pump(&mut comp);
    app.pump(&mut comp);
    assert!(app.events.contains(&Event::CloseRequested {
        surface: SurfaceId(2)
    }));
    assert!(shell.error().is_none());
    app.send(&Request::WindowAction {
        window: first,
        action: WindowAction::Close,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

/// Super+P of the shell switches the compositor's display mode.
#[test]
fn display_mode_from_the_shell() {
    let mut comp = compositor(config(), &[(800, 600), (1024, 768)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    shell.send(&Request::SetDisplayMode {
        mode: DisplayModeRequest::Mirror,
    });
    shell.pump(&mut comp);
    assert_eq!(comp.state.layout.mode(), DisplayMode::Mirror);
    shell.send(&Request::SetDisplayMode {
        mode: DisplayModeRequest::Single(OutputId(1)),
    });
    shell.send(&Request::SetDisplayMode {
        mode: DisplayModeRequest::Single(OutputId(77)),
    });
    shell.pump(&mut comp);
    assert_eq!(comp.state.layout.mode(), DisplayMode::Single(OutputId(1)));
    assert!(shell.error().is_none());
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.send(&Request::SetDisplayMode {
        mode: DisplayModeRequest::Extend,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

/// Shell and lock screen learn the layouts and may switch them; the
/// active one is announced again after Super+Space; Caps Lock shows in key
/// events.
#[test]
fn keyboard_layouts_and_caps_lock() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    let layouts = |p: &mut Peer, comp: &mut Compositor| {
        p.pump(comp);
        p.events.iter().rev().find_map(|e| match e {
            Event::KeyboardLayouts { names, active } => Some((names.clone(), *active)),
            _ => None,
        })
    };
    assert_eq!(
        layouts(&mut shell, &mut comp),
        Some((vec!["ar".to_owned(), "en".to_owned()], 0))
    );
    shell.send(&Request::SetKeyboardLayout { index: 1 });
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 1);
    key(&mut comp, 125, true);
    key(&mut comp, 57, true);
    key(&mut comp, 57, false);
    key(&mut comp, 125, false);
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 0);
    let mut lock = Peer::hello_as(&mut comp, LOCK_UID);
    lock.send(&Request::SetKeyboardLayout { index: 1 });
    lock.send(&Request::SetKeyboardLayout { index: 9 });
    assert_eq!(layouts(&mut lock, &mut comp).unwrap().1, 1);
    assert!(lock.error().is_none());
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    key(&mut comp, 58, true);
    key(&mut comp, 58, false);
    key(&mut comp, 30, true);
    app.pump(&mut comp);
    let caps = app.events.iter().any(|e| {
        matches!(e, Event::Key { code: 30, modifiers, .. } if modifiers.has(Modifiers::CAPS_LOCK))
    });
    assert!(caps);
    app.send(&Request::SetKeyboardLayout { index: 0 });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

/// Ctrl+Shift and Alt+Shift switch the layout when released without another key (also while
/// locked); Ctrl+Shift+N stays a shortcut; the shell sets the list (settings) and the text of
/// the keys follows the active layout.
#[test]
fn layout_switch_chords_and_list() {
    let mut comp = compositor(config(), &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    let layouts = |p: &mut Peer, comp: &mut Compositor| {
        p.pump(comp);
        p.events.iter().rev().find_map(|e| match e {
            Event::KeyboardLayouts { names, active } => Some((names.clone(), *active)),
            _ => None,
        })
    };
    let mut app = Peer::hello_as(&mut comp, APP_UID);
    app.window(&mut comp, 1, 0xff0000);
    let typed = |app: &mut Peer, comp: &mut Compositor| {
        app.take();
        key(comp, 30, true);
        key(comp, 30, false);
        app.pump(comp);
        app.events.iter().find_map(|e| match e {
            Event::Key {
                code: 30,
                state: 1,
                character,
                ..
            } => char::from_u32(*character),
            _ => None,
        })
    };
    assert_eq!(typed(&mut app, &mut comp), Some('ش'));
    // Ctrl+Shift: switches on the release.
    key(&mut comp, 29, true);
    key(&mut comp, 42, true);
    key(&mut comp, 42, false);
    key(&mut comp, 29, false);
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 1);
    assert_eq!(typed(&mut app, &mut comp), Some('a'));
    // Alt+Shift (the other order) switches back.
    key(&mut comp, 42, true);
    key(&mut comp, 56, true);
    key(&mut comp, 56, false);
    key(&mut comp, 42, false);
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 0);
    // Ctrl+Shift+N is a shortcut, no switch.
    key(&mut comp, 29, true);
    key(&mut comp, 42, true);
    key(&mut comp, 49, true);
    key(&mut comp, 49, false);
    key(&mut comp, 42, false);
    key(&mut comp, 29, false);
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 0);
    // The shell sets Arabic + German; unknown names are left out.
    shell.send(&Request::SetKeyboardLayouts {
        names: vec!["ar".into(), "xx".into(), "de".into()],
    });
    assert_eq!(
        layouts(&mut shell, &mut comp),
        Some((vec!["ar".to_owned(), "de".to_owned()], 0))
    );
    key(&mut comp, 29, true);
    key(&mut comp, 42, true);
    key(&mut comp, 29, false);
    key(&mut comp, 42, false);
    assert_eq!(layouts(&mut shell, &mut comp).unwrap().1, 1);
    app.take();
    key(&mut comp, 21, true);
    key(&mut comp, 21, false);
    app.pump(&mut comp);
    assert!(app.events.iter().any(
        |e| matches!(e, Event::Key { code: 21, state: 1, character, .. } if *character == u32::from('z'))
    ));
    // Not for apps.
    app.send(&Request::SetKeyboardLayouts {
        names: vec!["en".into()],
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    // The lock screen hears of a switch by the chord.
    let mut lock = Peer::hello_as(&mut comp, LOCK_UID);
    assert_eq!(layouts(&mut lock, &mut comp).unwrap().1, 1);
    key(&mut comp, 56, true);
    key(&mut comp, 42, true);
    key(&mut comp, 42, false);
    key(&mut comp, 56, false);
    assert_eq!(layouts(&mut lock, &mut comp).unwrap().1, 0);
}

/// While locked nothing of the shell's reaches the keyboard: no shortcut,
/// no sheet focus.
#[test]
fn locked_session_has_no_shell_keys() {
    let mut rights = RightsTable::default();
    rights.grant(SHELL_UID, Rights::SHELL);
    rights.grant(LOCK_UID, Rights::LOCK_SCREEN);
    let mut comp = compositor(
        Config {
            rights,
            ..test_config()
        },
        &[(800, 600)],
    );
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    panel(&mut shell, &mut comp);
    let sheet = popup(&mut shell, &mut comp, 2, (0, -100), (100, 50));
    shell.send(&Request::ShellFocus { surface: sheet });
    let _lock = Peer::hello_as(&mut comp, LOCK_UID);
    shell.pump(&mut comp);
    // Super+L locks (a lock screen client is connected).
    key(&mut comp, 125, true);
    key(&mut comp, 38, true);
    key(&mut comp, 38, false);
    key(&mut comp, 125, false);
    assert!(comp.state.locked());
    shell.pump(&mut comp);
    assert!(shell
        .events
        .contains(&Event::ShellFocusLost { surface: sheet }));
    shell.take();
    key(&mut comp, 125, true);
    key(&mut comp, 25, true);
    key(&mut comp, 25, false);
    key(&mut comp, 125, false);
    assert!(shell_keys(&mut shell, &mut comp).is_empty());
    shell.send(&Request::ShellFocus { surface: sheet });
    shell.pump(&mut comp);
    assert!(comp.state.session.shell.focus.is_none());
}
