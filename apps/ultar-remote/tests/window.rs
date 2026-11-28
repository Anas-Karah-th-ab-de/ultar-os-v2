//! The remote view window headless: every state in Arabic and English, dark and light, the
//! clicks of every button, the pointer and keys that go to the other device, the end states
//! and the retry. The mesh is played by a scripted port.

mod common;

use common::*;
use std::time::{Duration, Instant};
use ultar_bus::messages::mesh_view::{Button, EndReason, RemoteInput, ViewEvent};
use ultar_i18n::Lang;
use ultar_remote::app::{Phase, Wish, WAIT_LIMIT};
use ultar_remote::port::PortEvent;
use ultar_ui::event::{Event, Key, Modifiers, PointerButton};
use ultar_ui::Point;

#[test]
fn without_devices_it_says_so_and_a_device_appears_by_itself() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    assert!(
        !has(&mut app, "remote.connect"),
        "nothing to connect to yet"
    );
    assert!(app.candidates().is_empty());
    port.tell(PortEvent::Peers(vec![
        peer("a1", "Laptop", true),
        peer("b2", "Phone", false),
    ]));
    app.poll(now).unwrap();
    let names: Vec<&str> = app.candidates().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["Laptop"], "only paired and reachable devices");
    assert!(has(&mut app, "remote.device"));
    assert!(has(&mut app, "remote.connect"));
}

#[test]
fn connecting_asks_the_chosen_device_and_waits() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    port.tell(PortEvent::Peers(vec![
        peer("a1", "Laptop", true),
        peer("c3", "Tablet", true),
    ]));
    app.poll(now).unwrap();
    choose(&mut app, "remote.device", 1, now);
    click(&mut app, "remote.connect", now);
    assert_eq!(port.0.borrow().started, ["c3"]);
    assert_eq!(app.phase(), Phase::Waiting);
    assert!(has(&mut app, "remote.cancel"));
    click(&mut app, "remote.cancel", now);
    assert_eq!(port.0.borrow().stops, 1, "cancel tells the mesh");
    assert_eq!(app.phase(), Phase::Choose);
}

#[test]
fn the_screen_arrives_in_pieces_and_is_shown() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    let piece: Vec<u8> = (0..4 * 3 * 4)
        .map(|i| if i % 4 == 3 { 0 } else { 77 })
        .collect();
    port.tell(PortEvent::View(ViewEvent::Picture {
        x: 10,
        y: 20,
        w: 4,
        h: 3,
        data: piece,
    }));
    // a piece outside the screen is ignored
    port.tell(PortEvent::View(ViewEvent::Picture {
        x: W - 1,
        y: 0,
        w: 4,
        h: 1,
        data: vec![9; 16],
    }));
    app.poll(now).unwrap();
    let remote = app.remote().unwrap();
    assert_eq!((remote.width(), remote.height()), (W, H));
    let at = |x: u32, y: u32| remote.data()[((y * W + x) * 4) as usize];
    assert_eq!(
        (at(10, 20), at(13, 22), at(14, 20), at(W - 1, 0)),
        (77, 77, 0, 0)
    );
    let canvas = render(&mut app.ui).unwrap();
    let img = Image::from_canvas(&canvas);
    let first = &img.rgba[..4];
    assert!(img.rgba.chunks(4).any(|p| p != first));
}

#[test]
fn moves_between_two_looks_are_one_move_to_the_right_place() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    for (x, y) in [(10, 10), (20, 30), (100, 50)] {
        let pos = at(&mut app, x, y);
        app.pointer(Event::PointerMove { pos }).unwrap();
    }
    assert!(port.0.borrow().inputs.is_empty(), "held back until the next look");
    app.poll(now).unwrap();
    assert_eq!(port.0.borrow().inputs, [RemoteInput::Move { x: 100, y: 50 }]);
}

fn mouse(app: &mut RemoteApp, pos: Point, button: PointerButton, down: bool) {
    let event = if down {
        Event::PointerDown { pos, button }
    } else {
        Event::PointerUp { pos, button }
    };
    app.pointer(event).unwrap();
}

#[test]
fn a_drag_that_leaves_the_picture_is_released_on_its_edge() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    let down = at(&mut app, 5, 6);
    mouse(&mut app, down, PointerButton::Primary, true);
    let out = Point { x: 5000.0, y: -50.0 };
    app.pointer(Event::PointerMove { pos: out }).unwrap();
    mouse(&mut app, out, PointerButton::Primary, false);
    let btn = |x, y, down| RemoteInput::Button { x, y, button: Button::Left, down };
    assert_eq!(
        port.0.borrow().inputs,
        [btn(5, 6, true), RemoteInput::Move { x: W - 1, y: 0 }, btn(W - 1, 0, false)]
    );
}

#[test]
fn a_right_click_and_the_wheel_arrive_and_every_word_is_valid() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    let p = at(&mut app, 50, 60);
    mouse(&mut app, p, PointerButton::Secondary, true);
    mouse(&mut app, p, PointerButton::Secondary, false);
    app.pointer(Event::Scroll { pos: p, lines: 3.0 }).unwrap();
    app.pointer(Event::Scroll { pos: p, lines: 0.0 }).unwrap();
    let btn = |down| RemoteInput::Button { x: 50, y: 60, button: Button::Right, down };
    let inputs = port.0.borrow().inputs.clone();
    assert_eq!(inputs, [btn(true), btn(false), RemoteInput::Wheel { x: 50, y: 60, lines: 3 }]);
    for i in inputs {
        assert_eq!(RemoteInput::parse(&i.word()), Some(i));
    }
}

#[test]
fn a_press_on_the_bars_goes_nowhere() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    // 320x200 in a wide window: bars at the left
    let id = find(&mut app, "remote.picture");
    let r = app.ui.rect(id).unwrap();
    let pos = Point {
        x: r.x + 2.0,
        y: r.y + r.h / 2.0,
    };
    app.pointer(Event::PointerDown {
        pos,
        button: PointerButton::Primary,
    })
    .unwrap();
    assert!(port.0.borrow().inputs.is_empty());
}

#[test]
fn keys_become_key_codes_and_arabic_letters_are_dropped() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    let key = |key, modifiers| Event::Key { key, modifiers };
    app.key(key(Key::Char('a'), Modifiers::default())).unwrap();
    app.key(key(Key::Char('ب'), Modifiers::default())).unwrap();
    app.key(key(
        Key::Enter,
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        },
    ))
    .unwrap();
    let k = |code, down| RemoteInput::Key { code, down };
    assert_eq!(
        port.0.borrow().inputs,
        [
            k(30, true),
            k(30, false),
            k(29, true),
            k(28, true),
            k(28, false),
            k(29, false)
        ]
    );
}

#[test]
fn the_toolbar_sends_key_combinations_and_quality_changes_the_picture() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    choose(&mut app, "remote.keys", 1, now);
    let k = |code, down| RemoteInput::Key { code, down };
    assert_eq!(
        port.0.borrow().inputs,
        [
            k(29, true),
            k(56, true),
            k(111, true),
            k(111, false),
            k(56, false),
            k(29, false)
        ]
    );
    assert!(
        has(&mut app, "remote.keys"),
        "the menu is back at its heading"
    );
    choose(&mut app, "remote.quality", 1, now);
    assert_eq!(app.phase(), Phase::Running);
    assert!(
        app.remote().is_some(),
        "the screen stays when the quality changes"
    );
}

#[test]
fn stop_ends_the_session_and_full_screen_is_a_wish_of_the_window() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    click(&mut app, "remote.fullscreen", now);
    assert_eq!(app.take_wish(), Some(Wish::Fullscreen(true)));
    assert!(app.fullscreen());
    assert!(has(&mut app, "remote.leave_fullscreen"));
    // F11 is the window's own key and is not typed on the other device
    app.key(Event::Key {
        key: Key::F(11),
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert_eq!(app.take_wish(), Some(Wish::Fullscreen(false)));
    assert!(port.0.borrow().inputs.is_empty());
    click(&mut app, "remote.stop", now);
    assert_eq!(port.0.borrow().stops, 1);
    assert_eq!(app.phase(), Phase::Choose);
    assert!(app.remote().is_none());
    // stopping while in full screen leaves it
    running(&mut app, &port, now);
    app.toggle_fullscreen().unwrap();
    app.take_wish();
    click(&mut app, "remote.stop", now);
    assert_eq!(app.take_wish(), Some(Wish::Fullscreen(false)));
}

#[test]
fn every_end_has_its_own_screen_and_a_retry_that_asks_again() {
    for (event, phase) in [
        (EndReason::Refused, Phase::Refused),
        (EndReason::Stopped, Phase::Ended(EndReason::Stopped)),
        (EndReason::Locked, Phase::Ended(EndReason::Locked)),
        (EndReason::Lost, Phase::Ended(EndReason::Lost)),
    ] {
        let (mut app, port) = fresh();
        let now = Instant::now();
        running(&mut app, &port, now);
        port.tell(PortEvent::View(ViewEvent::Ended { reason: event }));
        app.poll(now).unwrap();
        assert_eq!(app.phase(), phase);
        assert!(app.remote().is_none(), "nothing of the old screen stays");
        assert!(has(&mut app, "remote.retry") && has(&mut app, "remote.back"));
        click(&mut app, "remote.retry", now);
        assert_eq!(port.0.borrow().started, ["a1", "a1"], "{event:?}");
        assert_eq!(app.phase(), Phase::Waiting);
        port.tell(PortEvent::View(ViewEvent::Ended {
            reason: EndReason::Refused,
        }));
        app.poll(now).unwrap();
        click(&mut app, "remote.back", now);
        assert_eq!(app.phase(), Phase::Choose);
    }
}

#[test]
fn a_lost_bus_and_a_refused_start_are_connection_lost_and_an_unanswered_ask_gives_up() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    port.tell(PortEvent::Lost);
    app.poll(now).unwrap();
    assert_eq!(app.phase(), Phase::Ended(EndReason::Lost));

    let (mut app, port) = fresh();
    port.0.borrow_mut().refuse_start = true;
    port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
    app.poll(now).unwrap();
    click(&mut app, "remote.connect", now);
    assert_eq!(app.phase(), Phase::Ended(EndReason::Lost));

    let (mut app, port) = fresh();
    port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
    app.poll(now).unwrap();
    click(&mut app, "remote.connect", now);
    app.poll(now + WAIT_LIMIT - Duration::from_secs(1)).unwrap();
    assert_eq!(app.phase(), Phase::Waiting);
    app.poll(now + WAIT_LIMIT).unwrap();
    assert_eq!(app.phase(), Phase::Refused);
    assert_eq!(port.0.borrow().stops, 1);
}

#[test]
fn a_slow_link_drops_input_and_counts_it_without_queueing() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    running(&mut app, &port, now);
    port.0.borrow_mut().refuse_input = true;
    let p = at(&mut app, 3, 3);
    app.pointer(Event::PointerDown {
        pos: p,
        button: PointerButton::Primary,
    })
    .unwrap();
    app.pointer(Event::PointerUp {
        pos: p,
        button: PointerButton::Primary,
    })
    .unwrap();
    assert_eq!(app.dropped, 2);
    port.0.borrow_mut().refuse_input = false;
    app.poll(now).unwrap();
    assert!(
        port.0.borrow().inputs.is_empty(),
        "dropped events are not sent later"
    );
}

#[test]
fn input_outside_a_session_is_not_sent() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    app.key(Event::Key {
        key: Key::Char('a'),
        modifiers: Modifiers::default(),
    })
    .unwrap();
    app.pointer(Event::PointerDown {
        pos: Point { x: 5.0, y: 5.0 },
        button: PointerButton::Primary,
    })
    .unwrap();
    running(&mut app, &port, now);
    port.tell(PortEvent::View(ViewEvent::Ended {
        reason: EndReason::Locked,
    }));
    app.poll(now).unwrap();
    app.key(Event::Key {
        key: Key::Char('a'),
        modifiers: Modifiers::default(),
    })
    .unwrap();
    assert!(port.0.borrow().inputs.is_empty());
}

#[test]
fn a_screen_of_an_absurd_size_ends_the_session_instead_of_eating_memory() {
    let (mut app, port) = fresh();
    let now = Instant::now();
    port.tell(PortEvent::Peers(vec![peer("a1", "Laptop", true)]));
    app.poll(now).unwrap();
    click(&mut app, "remote.connect", now);
    port.tell(PortEvent::View(ViewEvent::Running {
        width: 8192,
        height: 8192,
    }));
    app.poll(now).unwrap();
    assert_eq!(app.phase(), Phase::Ended(EndReason::Lost));
    assert_eq!(port.0.borrow().stops, 1);
}

