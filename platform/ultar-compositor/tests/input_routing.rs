//! Input goes only to the surface it belongs to, with coordinates relative
//! to that surface; decorations, snapping and shortcuts work.

mod common;

use common::*;
use ultar_compositor::Config;
use ultar_hal::{InputEventKind, PointerButton};
use ultar_surface::proto::{Event, Request, WindowState};

/// A and B map one after the other; nothing was clicked yet.
fn two_new_windows() -> (ultar_compositor::Compositor, Peer, Peer) {
    let mut comp = compositor(test_config(), &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    let mut b = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0xff0000);
    b.window(&mut comp, 1, 0x0000ff);
    a.pump(&mut comp);
    b.pump(&mut comp);
    (comp, a, b)
}

/// Two windows; the user clicked into B (B has the focus).
fn two_windows() -> (ultar_compositor::Compositor, Peer, Peer) {
    let (mut comp, mut a, mut b) = two_new_windows();
    let c = comp.state.windows[1].content();
    click(
        &mut comp,
        f64::from(c.x + c.w / 2),
        f64::from(c.y + c.h / 2),
    );
    a.pump(&mut comp);
    b.pump(&mut comp);
    a.take();
    b.take();
    (comp, a, b)
}

#[test]
fn windows_map_cascade_and_only_the_first_takes_the_focus() {
    let (mut comp, mut a, mut b) = two_new_windows();
    let wins: Vec<_> = comp.state.windows.iter().filter(|w| w.mapped).collect();
    assert_eq!(wins.len(), 2);
    assert_ne!(wins[0].frame, wins[1].frame);
    // B opened without a user action on B: it is shown, inactive.
    assert_eq!(comp.state.focus.map(|k| k.client), Some(wins[0].key.client));
    assert!(!b
        .take()
        .iter()
        .any(|e| matches!(e, Event::KeyboardFocus { focused: true, .. })));
    a.take();
    // A click moves the focus.
    let f = comp.state.windows[1].frame;
    click(&mut comp, f64::from(f.x + f.w / 2), f64::from(f.y + 10));
    a.pump(&mut comp);
    b.pump(&mut comp);
    let focus_a: Vec<_> = a
        .take()
        .into_iter()
        .filter(|e| matches!(e, Event::KeyboardFocus { .. }))
        .collect();
    assert_eq!(
        focus_a.last(),
        Some(&Event::KeyboardFocus {
            surface: comp.state.windows[0].key.surface,
            focused: false
        })
    );
    assert!(b
        .take()
        .iter()
        .any(|e| matches!(e, Event::KeyboardFocus { focused: true, .. })));
}

#[test]
fn clicks_focus_and_route_with_local_coordinates() {
    let (mut comp, mut a, mut b) = two_windows();
    let wa = comp.state.windows[0].content();
    // A point of window A not covered by B.
    let (x, y) = (f64::from(wa.x + 5), f64::from(wa.y + 7));
    assert_eq!(
        comp.state.hit_test(x, y).map(|h| h.0.client),
        Some(comp.state.windows[0].key.client)
    );
    a.take();
    b.take();
    click(&mut comp, x + 0.5, y + 0.25);
    a.read();
    b.read();
    let ea = a.take();
    assert!(ea.iter().any(|e| matches!(
        e,
        Event::PointerButton {
            pressed: true,
            button: 0x110,
            ..
        }
    )));
    assert!(ea.iter().any(|e| matches!(e, Event::PointerEnter { x, y, .. } if *x == 5 * 256 + 128 && *y == 7 * 256 + 64)));
    assert!(ea
        .iter()
        .any(|e| matches!(e, Event::KeyboardFocus { focused: true, .. })));
    let eb = b.take();
    assert!(
        !eb.iter().any(|e| matches!(e, Event::PointerButton { .. })),
        "B saw nothing of the click"
    );
    assert!(eb
        .iter()
        .any(|e| matches!(e, Event::KeyboardFocus { focused: false, .. })));
    // A is on top now.
    assert_eq!(
        comp.state.windows.last().map(|w| w.key.client),
        Some(comp.state.focus.unwrap().client)
    );
}

#[test]
fn keys_go_to_the_focused_client_only() {
    let (mut comp, mut a, mut b) = two_windows();
    let wa = comp.state.windows[0].content();
    click(&mut comp, f64::from(wa.x + 5), f64::from(wa.y + 7));
    a.read();
    b.read();
    a.take();
    b.take();
    // Keys go to A only, with Arabic characters by default.
    key(&mut comp, 30, true);
    key(&mut comp, 30, false);
    a.read();
    b.read();
    let keys: Vec<_> = a
        .take()
        .into_iter()
        .filter_map(|e| match e {
            Event::Key {
                code,
                state,
                character,
                ..
            } => Some((code, state, character)),
            _ => None,
        })
        .collect();
    assert_eq!(keys, vec![(30, 1, 'ش' as u32), (30, 0, 0)]);
    assert!(b.take().iter().all(|e| !matches!(e, Event::Key { .. })));
}

#[test]
fn implicit_grab_keeps_the_pointer_until_release() {
    let (mut comp, mut a, mut b) = two_windows();
    let wb = comp.state.windows[1].content();
    move_to(&mut comp, f64::from(wb.x + 10), f64::from(wb.y + 10));
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    })]);
    b.read();
    b.take();
    // Drag far outside B: motion still goes to B with negative coordinates.
    move_to(&mut comp, 1.0, 1.0);
    b.read();
    assert!(b
        .take()
        .iter()
        .any(|e| matches!(e, Event::PointerMotion { x, .. } if *x < 0)));
    a.read();
    assert!(!a
        .take()
        .iter()
        .any(|e| matches!(e, Event::PointerMotion { .. } | Event::PointerEnter { .. })));
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    })]);
    b.read();
    assert!(b
        .take()
        .iter()
        .any(|e| matches!(e, Event::PointerButton { pressed: false, .. })));
}

#[test]
fn title_bar_drag_snap_and_shortcuts() {
    let (mut comp, _a, mut b) = two_windows();
    let top = comp.state.windows.len() - 1;
    let frame = comp.state.windows[top].frame;
    let wkey = comp.state.windows[top].key;
    let index = |comp: &ultar_compositor::Compositor| comp.state.window_index(wkey).unwrap();
    // Drag the title to the left edge: snaps to the left half.
    move_to(&mut comp, f64::from(frame.x + 200), f64::from(frame.y + 20));
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    })]);
    move_to(&mut comp, 100.0, 300.0);
    move_to(&mut comp, 0.0, 300.0);
    assert!(comp.state.pointer.snap_preview.is_some());
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    })]);
    assert_eq!(
        comp.state.windows[index(&comp)].snap,
        Some(ultar_compositor::wm::Snap::Left)
    );
    // The app draws the new size; the frame follows only then.
    b.pump(&mut comp);
    b.draw(&mut comp, wkey.surface, 0x0000ff);
    let work = comp
        .state
        .work_area(comp.state.layout.primary().unwrap().id);
    assert_eq!(
        comp.state.windows[index(&comp)].frame,
        ultar_compositor::wm::snap_rect(ultar_compositor::wm::Snap::Left, work)
    );
    // Super+Up: snapped left -> top left quarter.
    key(&mut comp, 125, true);
    key(&mut comp, 103, true);
    key(&mut comp, 103, false);
    key(&mut comp, 125, false);
    assert_eq!(
        comp.state.windows[index(&comp)].snap,
        Some(ultar_compositor::wm::Snap::TopLeft)
    );
    b.pump(&mut comp);
    b.draw(&mut comp, wkey.surface, 0x0000ff);
    // Double click on the title maximises.
    let f = comp.state.windows[index(&comp)].frame;
    click(&mut comp, f64::from(f.x + 100), f64::from(f.y + 20));
    click(&mut comp, f64::from(f.x + 100), f64::from(f.y + 20));
    assert_eq!(
        comp.state.windows[index(&comp)].state,
        WindowState::Maximized
    );
    b.pump(&mut comp);
    b.draw(&mut comp, wkey.surface, 0x0000ff);
    assert_eq!(comp.state.windows[index(&comp)].frame, work);
    assert!(b
        .last_configure(wkey.surface)
        .is_some_and(|c| c.state == WindowState::Maximized));
}

#[test]
fn close_button_asks_the_app_and_alt_f4_too() {
    let (mut comp, _a, mut b) = two_windows();
    let top = comp.state.windows.len() - 1;
    let frame = comp.state.windows[top].frame;
    click(
        &mut comp,
        f64::from(frame.max_x() - 20),
        f64::from(frame.y + 20),
    );
    b.read();
    assert!(b
        .take()
        .iter()
        .any(|e| matches!(e, Event::CloseRequested { .. })));
    key(&mut comp, 56, true);
    key(&mut comp, 62, true);
    key(&mut comp, 62, false);
    key(&mut comp, 56, false);
    b.read();
    let got = b.take();
    assert!(got
        .iter()
        .any(|e| matches!(e, Event::CloseRequested { .. })));
    assert!(
        !got.iter().any(|e| matches!(
            e,
            Event::Key {
                code: 62,
                state: 1,
                ..
            }
        )),
        "shortcut is not passed on"
    );
}

#[test]
fn arabic_puts_the_close_button_on_the_left() {
    let config = Config {
        lang: ultar_ui::Lang::Ar,
        ..test_config()
    };
    let mut comp = compositor(config, &[(1280, 800)]);
    let mut a = Peer::hello(&mut comp);
    a.window(&mut comp, 1, 0x00ff00);
    let frame = comp.state.windows[0].frame;
    assert!(
        a.last_configure(ultar_surface::proto::SurfaceId(1))
            .unwrap()
            .rtl
    );
    click(&mut comp, f64::from(frame.x + 20), f64::from(frame.y + 20));
    a.read();
    assert!(a
        .take()
        .iter()
        .any(|e| matches!(e, Event::CloseRequested { .. })));
}

#[test]
fn touch_goes_to_the_touched_surface_and_alt_tab_cycles() {
    let (mut comp, mut a, mut b) = two_windows();
    let wa = comp.state.windows[0].content();
    let o = comp.state.layout.primary().unwrap().logical_rect();
    let nx = ((f64::from(wa.x + 3) + 0.5) * 65536.0 / f64::from(o.w)) as u16;
    let ny = ((f64::from(wa.y + 3) + 0.5) * 65536.0 / f64::from(o.h)) as u16;
    comp.inject(&[ev(InputEventKind::TouchDown {
        slot: 0,
        x: nx,
        y: ny,
    })]);
    comp.inject(&[ev(InputEventKind::TouchUp { slot: 0 })]);
    a.read();
    b.read();
    let ea = a.take();
    assert!(ea
        .iter()
        .any(|e| matches!(e, Event::TouchDown { x, .. } if *x / 256 == 3)));
    assert!(ea.iter().any(|e| matches!(e, Event::TouchUp { .. })));
    assert!(!b
        .take()
        .iter()
        .any(|e| matches!(e, Event::TouchDown { .. })));
    let before = comp.state.focus;
    key(&mut comp, 56, true);
    key(&mut comp, 15, true);
    key(&mut comp, 15, false);
    key(&mut comp, 56, false);
    assert_ne!(comp.state.focus, before);
}

#[test]
fn minimise_hides_and_alt_tab_restores() {
    let (mut comp, _a, mut b) = two_windows();
    let key_b = comp.state.windows[1].key;
    b.send(&Request::SetState {
        surface: key_b.surface,
        state: WindowState::Minimized,
    });
    b.pump(&mut comp);
    assert!(!comp.state.windows[comp.state.window_index(key_b).unwrap()].visible());
    assert_ne!(comp.state.focus, Some(key_b));
    key(&mut comp, 56, true);
    key(&mut comp, 15, true);
    key(&mut comp, 56, false);
    assert_eq!(comp.state.focus, Some(key_b));
}
