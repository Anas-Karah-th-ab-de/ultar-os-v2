//! The keyboard helpers and the compose key end to end: the display service passes them on
//! (`Request::ConfigureKeyHelpers`), an app may not, and the compositor's key input path
//! follows them – including a change at runtime.

mod common;

use common::{compositor, test_config, Peer};
use std::time::Duration;
use ultar_compositor::keymap::Layout;
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_hal::{InputEvent, InputEventKind, KeyState};
use ultar_surface::proto::{ComposeKey, ErrorCode, Event, Modifiers, Request, Rights};

const UID_DISPLAYD: u32 = 817;
const UID_APP: u32 = 20_001;

/// One display, the display service holds the configuration right, one keyboard layout.
fn rig(layout: Layout) -> Compositor {
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    let config = Config {
        rights,
        layouts: vec![layout],
        ..test_config()
    };
    compositor(config, &[(1280, 800)])
}

fn helpers(sticky: bool, slow: bool, bounce: bool, mouse: bool, compose: ComposeKey) -> Request {
    Request::ConfigureKeyHelpers {
        sticky_keys: sticky,
        slow_keys: slow,
        bounce_keys: bounce,
        mouse_keys: mouse,
        compose_key: compose,
    }
}

fn key_ev(code: u16, state: KeyState, ms: u64) -> InputEvent {
    InputEvent {
        time_us: ms * 1000,
        ..common::ev(InputEventKind::Key { code, state })
    }
}

/// A window with the keyboard focus and the display service's connection.
fn focused(layout: Layout) -> (Compositor, Peer, Peer) {
    let mut comp = rig(layout);
    let mut w = Peer::hello(&mut comp);
    w.window(&mut comp, 1, 0x00ff00);
    w.pump(&mut comp);
    w.take();
    let s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    (comp, w, s)
}

/// The character and modifiers of the press of `code` among the events.
fn press_of(events: &[Event], code: u16) -> Option<(u32, Modifiers)> {
    events.iter().find_map(|e| match e {
        Event::Key {
            code: c,
            state: 1,
            character,
            modifiers,
            ..
        } if *c == code => Some((*character, *modifiers)),
        _ => None,
    })
}

fn texts(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Text { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn an_app_may_not_configure_the_helpers() {
    let mut comp = rig(Layout::English);
    let mut app = Peer::hello_as(&mut comp, UID_APP);
    app.send(&helpers(true, true, true, true, ComposeKey::Menu));
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert!(!comp.state.keyboard.helpers.sticky.on);
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(&helpers(true, false, false, false, ComposeKey::None));
    s.pump(&mut comp);
    assert!(comp.state.keyboard.helpers.sticky.on);
}

#[test]
fn sticky_keys_latch_a_modifier_for_the_next_key_only() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(true, false, false, false, ComposeKey::None));
    s.pump(&mut comp);
    // Shift pressed and released alone, then 'a' (30): the client sees 'A' with Shift.
    comp.inject(&[
        key_ev(42, KeyState::Pressed, 0),
        key_ev(42, KeyState::Released, 0),
    ]);
    comp.inject(&[
        key_ev(30, KeyState::Pressed, 0),
        key_ev(30, KeyState::Released, 0),
    ]);
    w.pump(&mut comp);
    assert_eq!(
        press_of(&w.take(), 30),
        Some((u32::from('A'), Modifiers(Modifiers::SHIFT)))
    );
    // The next 'a' is normal again (the latch was consumed).
    comp.inject(&[
        key_ev(30, KeyState::Pressed, 0),
        key_ev(30, KeyState::Released, 0),
    ]);
    w.pump(&mut comp);
    assert_eq!(
        press_of(&w.take(), 30),
        Some((u32::from('a'), Modifiers(0)))
    );
}

#[test]
fn sticky_keys_pressed_twice_lock_the_modifier() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(true, false, false, false, ComposeKey::None));
    s.pump(&mut comp);
    for _ in 0..2 {
        comp.inject(&[
            key_ev(42, KeyState::Pressed, 0),
            key_ev(42, KeyState::Released, 0),
        ]);
    }
    for _ in 0..2 {
        comp.inject(&[
            key_ev(30, KeyState::Pressed, 0),
            key_ev(30, KeyState::Released, 0),
        ]);
        w.pump(&mut comp);
        assert_eq!(
            press_of(&w.take(), 30),
            Some((u32::from('A'), Modifiers(Modifiers::SHIFT))),
            "the lock stays"
        );
    }
}

#[test]
fn slow_keys_count_a_key_only_after_it_is_held() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(false, true, false, false, ComposeKey::None));
    s.pump(&mut comp);
    // A short press never counts.
    comp.state.now = 1000;
    comp.inject(&[key_ev(30, KeyState::Pressed, 0)]);
    comp.state.now = 1100;
    comp.inject(&[key_ev(30, KeyState::Released, 0)]);
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    w.pump(&mut comp);
    assert!(press_of(&w.take(), 30).is_none(), "released too soon");
    // A held press counts once it has been held 300 ms.
    comp.state.now = 2000;
    comp.inject(&[key_ev(30, KeyState::Pressed, 0)]);
    w.pump(&mut comp);
    assert!(press_of(&w.take(), 30).is_none(), "not yet");
    comp.state.now = 2300;
    comp.dispatch(Some(Duration::ZERO)).unwrap();
    w.pump(&mut comp);
    assert_eq!(
        press_of(&w.take(), 30),
        Some((u32::from('a'), Modifiers(0)))
    );
    comp.inject(&[key_ev(30, KeyState::Released, 0)]);
}

#[test]
fn bounce_keys_drop_a_quick_repress() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(false, false, true, false, ComposeKey::None));
    s.pump(&mut comp);
    let tap = |comp: &mut Compositor, at: u64| {
        comp.inject(&[
            key_ev(30, KeyState::Pressed, at),
            key_ev(30, KeyState::Released, at + 50),
        ]);
    };
    tap(&mut comp, 1000);
    tap(&mut comp, 1100); // a bounce 100 ms later is dropped
    w.pump(&mut comp);
    let presses = w
        .take()
        .iter()
        .filter(|e| {
            matches!(
                e,
                Event::Key {
                    code: 30,
                    state: 1,
                    ..
                }
            )
        })
        .count();
    assert_eq!(presses, 1, "the bounce did not count");
    tap(&mut comp, 2000); // after the interval it counts again
    w.pump(&mut comp);
    assert!(press_of(&w.take(), 30).is_some());
}

#[test]
fn mouse_keys_move_the_pointer_and_click() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(false, false, false, true, ComposeKey::None));
    s.pump(&mut comp);
    let x0 = comp.state.pointer.x;
    comp.inject(&[key_ev(77, KeyState::Pressed, 0)]); // keypad 6: a step to the right
    assert!(
        (comp.state.pointer.x - x0 - 20.0).abs() < 0.01,
        "the pointer moved a step"
    );
    // A click on the window content reaches the client; the keypad key is not delivered.
    let f = comp.state.windows[0].frame;
    common::move_to(&mut comp, f64::from(f.x + f.w / 2), f64::from(f.y + 40));
    comp.inject(&[key_ev(76, KeyState::Pressed, 0)]); // keypad 5: click
    w.pump(&mut comp);
    let events = w.take();
    assert!(events.iter().any(|e| matches!(
        e,
        Event::PointerButton {
            pressed: true,
            button: 0x110,
            ..
        }
    )));
    assert!(!events
        .iter()
        .any(|e| matches!(e, Event::Key { code: 76, .. })));
}

#[test]
fn the_compose_key_produces_a_latin_accent() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(false, false, false, false, ComposeKey::Menu));
    s.pump(&mut comp);
    // Menu, apostrophe (40), e (18) -> é.
    for code in [139, 40, 18] {
        comp.inject(&[
            key_ev(code, KeyState::Pressed, 0),
            key_ev(code, KeyState::Released, 0),
        ]);
    }
    w.pump(&mut comp);
    let events = w.take();
    assert_eq!(texts(&events), vec!["é".to_string()]);
    assert!(!events.iter().any(|e| matches!(e, Event::Key { .. })));
}

#[test]
fn the_compose_key_produces_an_arabic_hamza() {
    let (mut comp, mut w, mut s) = focused(Layout::Arabic);
    s.send(&helpers(false, false, false, false, ComposeKey::AltGr));
    s.pump(&mut comp);
    // Alt Gr (100), hamza (45), alef (35) -> alef with hamza above.
    for code in [100, 45, 35] {
        comp.inject(&[
            key_ev(code, KeyState::Pressed, 0),
            key_ev(code, KeyState::Released, 0),
        ]);
    }
    w.pump(&mut comp);
    assert_eq!(texts(&w.take()), vec!["أ".to_string()]);
}

#[test]
fn caps_lock_as_the_compose_key_does_not_toggle_caps() {
    let (mut comp, _w, mut s) = focused(Layout::English);
    s.send(&helpers(false, false, false, false, ComposeKey::CapsLock));
    s.pump(&mut comp);
    comp.inject(&[
        key_ev(58, KeyState::Pressed, 0),
        key_ev(58, KeyState::Released, 0),
    ]);
    assert!(
        !comp.state.keyboard.caps_lock,
        "it starts a sequence instead"
    );
}

#[test]
fn the_helpers_follow_a_runtime_change() {
    let (mut comp, mut w, mut s) = focused(Layout::English);
    s.send(&helpers(true, false, false, false, ComposeKey::None));
    s.pump(&mut comp);
    comp.inject(&[
        key_ev(42, KeyState::Pressed, 0),
        key_ev(42, KeyState::Released, 0),
    ]);
    comp.inject(&[
        key_ev(30, KeyState::Pressed, 0),
        key_ev(30, KeyState::Released, 0),
    ]);
    w.pump(&mut comp);
    assert_eq!(
        press_of(&w.take(), 30).map(|(c, _)| c),
        Some(u32::from('A'))
    );
    // Turned off at runtime: a lone Shift no longer latches.
    s.send(&helpers(false, false, false, false, ComposeKey::None));
    s.pump(&mut comp);
    comp.inject(&[
        key_ev(42, KeyState::Pressed, 0),
        key_ev(42, KeyState::Released, 0),
    ]);
    comp.inject(&[
        key_ev(30, KeyState::Pressed, 0),
        key_ev(30, KeyState::Released, 0),
    ]);
    w.pump(&mut comp);
    assert_eq!(
        press_of(&w.take(), 30).map(|(c, _)| c),
        Some(u32::from('a'))
    );
}
