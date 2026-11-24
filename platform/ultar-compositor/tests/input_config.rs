//! The display service passes the pointer and key-repeat settings on
//! (`Request::ConfigurePointer`, `Request::ConfigureKeyRepeat`); an app may not.

mod common;

use common::{compositor, test_config, Peer};
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_hal::InputEvent;
use ultar_surface::proto::{ErrorCode, Event, Request, Rights};

const UID_DISPLAYD: u32 = 817;
const UID_APP: u32 = 20_001;

/// One display; the display service's user holds the configuration right.
fn rig() -> Compositor {
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    let config = Config {
        rights,
        ..test_config()
    };
    compositor(config, &[(1280, 800)])
}

#[test]
fn the_display_service_sets_pointer_speed_and_scroll_direction() {
    use ultar_hal::InputEventKind;
    let mut comp = rig();
    let motion = |dx: f32| common::ev(InputEventKind::PointerMotion { dx, dy: 0.0 });
    let x0 = comp.state.pointer.x;
    comp.inject(&[motion(20.0)]);
    let plain = comp.state.pointer.x - x0;
    assert!((plain - 20.0).abs() < 0.01, "speed 0 moves as reported");

    // An app may not change it.
    let mut app = Peer::hello_as(&mut comp, UID_APP);
    let fast = Request::ConfigurePointer {
        speed: 5,
        natural_scroll: true,
    };
    app.send(&fast);
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert_eq!(comp.state.pointer.speed, 0);

    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(&fast);
    s.pump(&mut comp);
    assert!(comp.state.pointer.natural_scroll);
    let x1 = comp.state.pointer.x;
    comp.inject(&[motion(20.0)]);
    let doubled = comp.state.pointer.x - x1;
    assert!((doubled - 40.0).abs() < 0.01, "speed 5 doubles: {doubled}");

    s.send(&Request::ConfigurePointer {
        speed: -5,
        natural_scroll: false,
    });
    s.pump(&mut comp);
    let x2 = comp.state.pointer.x;
    comp.inject(&[motion(-20.0)]);
    let halved = x2 - comp.state.pointer.x;
    assert!((halved - 10.0).abs() < 0.01, "speed -5 halves: {halved}");
    assert!(!comp.state.pointer.natural_scroll);
}

#[test]
fn the_display_service_sets_how_a_held_key_repeats() {
    use ultar_hal::{InputEventKind, KeyState};
    let mut comp = rig();
    let mut w = Peer::hello(&mut comp);
    w.window(&mut comp, 1, 0x00ff00);
    w.pump(&mut comp);
    w.take();
    const KEY_A: u16 = 30;
    // The key held for a second: the driver repeats after 250 ms, then every 33 ms.
    let hold = |comp: &mut Compositor, w: &mut Peer| -> usize {
        let at = |state, ms: u64| InputEvent {
            time_us: ms * 1000,
            ..common::ev(InputEventKind::Key { code: KEY_A, state })
        };
        comp.inject(&[at(KeyState::Pressed, 0)]);
        for n in 0..23 {
            comp.inject(&[at(KeyState::Repeated, 250 + n * 33)]);
        }
        comp.inject(&[at(KeyState::Released, 1000)]);
        w.pump(comp);
        let repeats = |e: &Event| matches!(e, Event::Key { state: 2, .. });
        w.take().iter().filter(|e| repeats(e)).count()
    };
    assert_eq!(hold(&mut comp, &mut w), 23, "unset: as the driver reports");

    // An app may not change it.
    let mut app = Peer::hello_as(&mut comp, UID_APP);
    let slow = Request::ConfigureKeyRepeat {
        enabled: true,
        delay_ms: 500,
        per_second: 10,
    };
    app.send(&slow);
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    assert_eq!(hold(&mut comp, &mut w), 23);

    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(&slow);
    s.pump(&mut comp);
    // 500 ms delay, ten per second: the repeats at 514, 613, 712, 811, 910 ms.
    assert_eq!(hold(&mut comp, &mut w), 5);

    s.send(&Request::ConfigureKeyRepeat {
        enabled: false,
        delay_ms: 500,
        per_second: 10,
    });
    s.pump(&mut comp);
    assert_eq!(hold(&mut comp, &mut w), 0, "off: one press only");
}
