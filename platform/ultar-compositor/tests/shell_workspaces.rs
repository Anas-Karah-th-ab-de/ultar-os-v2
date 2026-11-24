//! Workspaces and window thumbnails of the desktop shell's overview (board Overview):
//! only the shell may use them, never while locked, never for system dialogs, bounded.

mod common;

use common::*;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_surface::proto::{
    Edge, ErrorCode, Event, OutputId, Request, Rights, Role, ShellWindow, SurfaceId,
    ALL_WORKSPACES, MAX_THUMBNAILS_PER_SECOND, MAX_WORKSPACES,
};

const SHELL_UID: u32 = 5001;
const LOCK_UID: u32 = 5002;
const APP_UID: u32 = 5003;

fn setup() -> (Compositor, Peer, Peer) {
    let mut rights = RightsTable::default();
    rights.grant(SHELL_UID, Rights::SHELL);
    rights.grant(LOCK_UID, Rights::LOCK_SCREEN);
    let config = Config {
        rights,
        ..test_config()
    };
    let mut comp = compositor(config, &[(800, 600)]);
    let mut shell = Peer::hello_as(&mut comp, SHELL_UID);
    let role = Role::Panel {
        edge: Edge::Bottom,
        size: 56,
    };
    shell.send(&Request::CreateSurface {
        surface: SurfaceId(1),
        role,
    });
    shell.pump(&mut comp);
    let app = Peer::hello_as(&mut comp, APP_UID);
    (comp, shell, app)
}

fn last<T>(p: &mut Peer, comp: &mut Compositor, f: impl Fn(&Event) -> Option<T>) -> Option<T> {
    p.pump(comp);
    p.events.iter().rev().find_map(f)
}

fn windows(p: &mut Peer, comp: &mut Compositor) -> Vec<ShellWindow> {
    last(p, comp, |e| match e {
        Event::WindowsChanged { windows } => Some(windows.clone()),
        _ => None,
    })
    .unwrap_or_default()
}

fn workspaces(p: &mut Peer, comp: &mut Compositor) -> Option<(u8, u8)> {
    last(p, comp, |e| match e {
        Event::Workspaces { current, count } => Some((*current, *count)),
        _ => None,
    })
}

fn pixel(comp: &mut Compositor, x: u32, y: u32) -> u32 {
    comp.render_now();
    let f = comp.frame(OutputId(0)).unwrap();
    f.data()[(y * f.width() + x) as usize] & 0xff_ffff
}

#[test]
fn windows_live_on_workspaces_and_only_the_shown_one_is_drawn() {
    let (mut comp, mut shell, mut app) = setup();
    assert_eq!(workspaces(&mut shell, &mut comp), Some((0, 1)));
    app.window(&mut comp, 1, 0xff0000);
    let list = windows(&mut shell, &mut comp);
    let red = list[0].id;
    assert_eq!(list[0].workspace, 0);
    let (cx, cy) = (400, 300);
    assert_eq!(
        pixel(&mut comp, cx, cy),
        0xff0000,
        "red window in the middle"
    );
    // A new workspace: shown, the red window hidden, the focus gone with it.
    shell.send(&Request::SwitchWorkspace { index: 1 });
    assert_eq!(workspaces(&mut shell, &mut comp), Some((1, 2)));
    assert_ne!(pixel(&mut comp, cx, cy), 0xff0000, "hidden on workspace 2");
    assert!(comp.state.focus.is_none());
    // A window opened now lives on workspace 2.
    app.window(&mut comp, 2, 0x00ff00);
    let green = windows(&mut shell, &mut comp)
        .into_iter()
        .find(|w| w.id != red)
        .unwrap();
    assert_eq!(green.workspace, 1);
    // Activating the red window shows its workspace again.
    shell.send(&Request::WindowAction {
        window: red,
        action: ultar_surface::proto::WindowAction::Activate,
    });
    assert_eq!(workspaces(&mut shell, &mut comp).map(|w| w.0), Some(0));
    assert_eq!(pixel(&mut comp, cx, cy), 0xff0000);
    // On all workspaces: drawn on both.
    shell.send(&Request::MoveToWorkspace {
        window: green.id,
        index: ALL_WORKSPACES,
    });
    shell.send(&Request::SwitchWorkspace { index: 1 });
    shell.pump(&mut comp);
    let list = windows(&mut shell, &mut comp);
    assert!(list
        .iter()
        .any(|w| w.id == green.id && w.workspace == ALL_WORKSPACES));
    assert_eq!(pixel(&mut comp, cx, cy), 0x00ff00);
    assert!(shell.error().is_none());
}

#[test]
fn workspaces_are_bounded_and_empty_ones_at_the_end_go() {
    let (mut comp, mut shell, _app) = setup();
    for i in 1..MAX_WORKSPACES as u8 {
        shell.send(&Request::SwitchWorkspace { index: i });
        shell.pump(&mut comp);
    }
    // Empty workspaces behind the shown one go: only up to the shown one stays.
    assert_eq!(
        workspaces(&mut shell, &mut comp),
        Some((MAX_WORKSPACES as u8 - 1, MAX_WORKSPACES as u8))
    );
    shell.send(&Request::SwitchWorkspace {
        index: MAX_WORKSPACES as u8,
    });
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::Limit));
    let (mut comp, mut shell, _app) = setup();
    shell.send(&Request::SwitchWorkspace { index: 1 });
    shell.send(&Request::SwitchWorkspace { index: 0 });
    assert_eq!(
        workspaces(&mut shell, &mut comp),
        Some((0, 1)),
        "empty 2 went"
    );
    shell.send(&Request::SwitchWorkspace { index: 5 });
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::Limit));
}

#[test]
fn thumbnails_are_scaled_copies_for_the_shell_only() {
    let (mut comp, mut shell, mut app) = setup();
    app.window(&mut comp, 1, 0x3366cc);
    let id = windows(&mut shell, &mut comp)[0].id;
    shell.send(&Request::WindowThumbnail {
        window: id,
        max_width: 64,
        max_height: 64,
    });
    let thumb = last(&mut shell, &mut comp, |e| match e {
        Event::WindowThumbnail {
            window,
            width,
            height,
            data,
        } => Some((*window, *width, *height, data.clone())),
        _ => None,
    })
    .unwrap();
    assert_eq!(thumb.0, id);
    assert!(thumb.1 <= 64 && thumb.2 <= 64 && (thumb.1 == 64 || thumb.2 == 64));
    assert_eq!(
        thumb.3.len(),
        usize::from(thumb.1) * usize::from(thumb.2) * 4
    );
    let px = u32::from_le_bytes([thumb.3[0], thumb.3[1], thumb.3[2], thumb.3[3]]);
    assert_eq!(px & 0xff_ffff, 0x3366cc);
    // An app may not read other windows, nor switch workspaces.
    app.send(&Request::WindowThumbnail {
        window: id,
        max_width: 8,
        max_height: 8,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    let (mut comp, _shell, mut app) = setup();
    app.send(&Request::SwitchWorkspace { index: 1 });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
}

#[test]
fn thumbnails_are_rate_limited_and_never_while_locked() {
    let (mut comp, mut shell, mut app) = setup();
    app.window(&mut comp, 1, 0x112233);
    let id = windows(&mut shell, &mut comp)[0].id;
    shell.take();
    for _ in 0..MAX_THUMBNAILS_PER_SECOND {
        shell.send(&Request::WindowThumbnail {
            window: id,
            max_width: 4,
            max_height: 4,
        });
    }
    shell.pump(&mut comp);
    let n = shell
        .take()
        .iter()
        .filter(|e| matches!(e, Event::WindowThumbnail { .. }))
        .count();
    assert_eq!(n, MAX_THUMBNAILS_PER_SECOND as usize);
    assert!(shell.error().is_none());
    shell.send(&Request::WindowThumbnail {
        window: id,
        max_width: 4,
        max_height: 4,
    });
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::Limit));
    // Locked: refused (a new shell connection, the old one failed).
    let (mut comp, mut shell, mut app) = setup();
    app.window(&mut comp, 1, 0x112233);
    let id = windows(&mut shell, &mut comp)[0].id;
    let _lock = Peer::hello_as(&mut comp, LOCK_UID);
    key(&mut comp, 125, true);
    key(&mut comp, 38, true);
    key(&mut comp, 38, false);
    key(&mut comp, 125, false);
    assert!(comp.state.locked());
    shell.send(&Request::WindowThumbnail {
        window: id,
        max_width: 4,
        max_height: 4,
    });
    shell.pump(&mut comp);
    assert_eq!(shell.error(), Some(ErrorCode::Permission));
    assert!(!shell
        .events
        .iter()
        .any(|e| matches!(e, Event::WindowThumbnail { .. })));
}
