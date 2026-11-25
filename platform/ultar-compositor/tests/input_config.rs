//! The display service passes the pointer, key-repeat and keyboard settings on
//! (`Request::ConfigurePointer`, `Request::ConfigureKeyRepeat`, `Request::ConfigureKeyboard`);
//! an app may not.

mod common;

use common::{compositor, test_config, Peer};
use ultar_compositor::{Compositor, Config, RightsTable};
use ultar_hal::InputEvent;
use ultar_surface::proto::{ErrorCode, Event, PointerSize, Request, Rights, SwitchShortcut};

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

/// The pointer configuration with the schema's defaults, as the display service sends
/// them before a setting changes.
#[derive(Clone, Copy)]
struct Cfg {
    speed: i8,
    natural_scroll: bool,
    swap_buttons: bool,
    acceleration: bool,
    size: PointerSize,
    double_click_ms: u16,
}

impl Default for Cfg {
    fn default() -> Self {
        Cfg {
            speed: 0,
            natural_scroll: false,
            swap_buttons: false,
            acceleration: true,
            size: PointerSize::Normal,
            double_click_ms: 400,
        }
    }
}

impl Cfg {
    fn request(self) -> Request {
        Request::ConfigurePointer {
            speed: self.speed,
            natural_scroll: self.natural_scroll,
            swap_buttons: self.swap_buttons,
            acceleration: self.acceleration,
            size: self.size,
            double_click_ms: self.double_click_ms,
        }
    }
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
    let fast = Cfg {
        speed: 5,
        natural_scroll: true,
        ..Cfg::default()
    }
    .request();
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

    s.send(
        &Cfg {
            speed: -5,
            natural_scroll: false,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    let x2 = comp.state.pointer.x;
    comp.inject(&[motion(-20.0)]);
    let halved = x2 - comp.state.pointer.x;
    assert!((halved - 10.0).abs() < 0.01, "speed -5 halves: {halved}");
    assert!(!comp.state.pointer.natural_scroll);
}

/// A window and the display service's connection, the right button primary.
fn swapped() -> (Compositor, Peer, Peer) {
    let mut comp = rig();
    let mut w = Peer::hello(&mut comp);
    w.window(&mut comp, 1, 0x00ff00);
    w.pump(&mut comp);
    w.take();
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    s.send(
        &Cfg {
            swap_buttons: true,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    (comp, w, s)
}

#[test]
fn the_primary_button_swap_reaches_the_clients() {
    use ultar_hal::{InputEventKind, PointerButton};
    let (mut comp, mut w, _s) = swapped();
    // The physical primary button on content is a secondary click for the client.
    let f = comp.state.windows[0].frame;
    common::move_to(&mut comp, f64::from(f.x + f.w / 2), f64::from(f.y + 40));
    let button = |b: PointerButton, pressed| {
        common::ev(InputEventKind::PointerButton { button: b, pressed })
    };
    comp.inject(&[button(PointerButton::Primary, true)]);
    w.pump(&mut comp);
    assert!(
        w.take().iter().any(|e| matches!(
            e,
            Event::PointerButton {
                button: 0x111,
                pressed: true,
                ..
            }
        )),
        "the client sees the swapped button"
    );
    comp.inject(&[button(PointerButton::Primary, false)]);
}

#[test]
fn the_swapped_buttons_drive_the_title_bar() {
    use ultar_hal::{InputEventKind, PointerButton};
    let (mut comp, _w, mut s) = swapped();
    let button = |b: PointerButton, pressed| {
        common::ev(InputEventKind::PointerButton { button: b, pressed })
    };
    let press_twice = |comp: &mut Compositor, b: PointerButton, at: u64| {
        let f = comp.state.windows[0].frame;
        common::move_to(comp, f64::from(f.x + f.w / 2), f64::from(f.y + 5));
        comp.state.now = at;
        comp.inject(&[button(b, true), button(b, false)]);
        comp.state.now = at + 300;
        comp.inject(&[button(b, true), button(b, false)]);
    };
    // The physical secondary button now drives the title bar: a double press maximizes.
    press_twice(&mut comp, PointerButton::Secondary, 1_000);
    assert_eq!(
        comp.state.windows[0].state,
        ultar_surface::proto::WindowState::Maximized
    );
    // Back to the left button primary: the physical primary button restores again.
    s.send(&Cfg::default().request());
    s.pump(&mut comp);
    press_twice(&mut comp, PointerButton::Primary, 2_000);
    assert_eq!(
        comp.state.windows[0].state,
        ultar_surface::proto::WindowState::Normal
    );
}

#[test]
fn acceleration_measures_velocity_and_can_be_switched_off() {
    use ultar_hal::InputEventKind;
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    let config = Config {
        rights,
        ..test_config()
    };
    // A wide output so the accelerated motions stay on it.
    let mut comp = compositor(config, &[(3000, 800)]);
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    let motion = |dx: f32, ms: u64| InputEvent {
        time_us: ms * 1000,
        ..common::ev(InputEventKind::PointerMotion { dx, dy: 0.0 })
    };
    // On (the default): the first event has no previous time, the next is fast
    // (300 units in 10 ms = 30/ms): three times as far.
    let x0 = comp.state.pointer.x;
    comp.inject(&[motion(300.0, 10)]);
    assert!((comp.state.pointer.x - x0 - 300.0).abs() < 0.01);
    comp.inject(&[motion(300.0, 20)]);
    assert!((comp.state.pointer.x - x0 - 1200.0).abs() < 0.01);
    // Off: as reported, whatever the velocity.
    s.send(
        &Cfg {
            acceleration: false,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    let x1 = comp.state.pointer.x;
    comp.inject(&[motion(-300.0, 30)]);
    assert!((comp.state.pointer.x - x1 + 300.0).abs() < 0.01);
    // On again, but events without usable times pass as reported.
    s.send(&Cfg::default().request());
    s.pump(&mut comp);
    let x2 = comp.state.pointer.x;
    comp.inject(&[motion(-300.0, 0), motion(-300.0, 0)]);
    assert!((comp.state.pointer.x - x2 + 600.0).abs() < 0.01);
}

/// Non-ground pixels around the resting pointer: the arrow's ink.
fn cursor_ink(comp: &mut Compositor, id: ultar_surface::proto::OutputId) -> usize {
    comp.render_now();
    let img = comp.frame(id).expect("frame");
    let ground = img.pixel(0, 0).expect("corner");
    let (px, py) = (comp.state.pointer.x as i32, comp.state.pointer.y as i32);
    let mut n = 0;
    for y in py - 2..py + 62 {
        for x in px - 2..px + 62 {
            if img.pixel(x, y).is_some_and(|p| p != ground) {
                n += 1;
            }
        }
    }
    n
}

#[test]
fn the_pointer_size_changes_the_drawn_cursor() {
    use ultar_compositor::backend::Headless;
    use ultar_compositor::MonotonicClock;
    let mut rights = RightsTable::default();
    rights.grant(UID_DISPLAYD, Rights::OUTPUT_CONFIG);
    let config = Config {
        rights,
        cursor: true,
        ..test_config()
    };
    let mut comp = Compositor::new(
        config,
        Box::new(Headless::new(&[(1280, 800)])),
        Box::new(MonotonicClock::default()),
    );
    let id = comp.state.layout.enabled().next().expect("output").id;
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    let normal = cursor_ink(&mut comp, id);
    assert!(normal > 0, "the cursor is drawn");
    s.send(
        &Cfg {
            size: PointerSize::Large,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    assert_eq!(comp.state.pointer.size, PointerSize::Large);
    let large = cursor_ink(&mut comp, id);
    assert!(large > normal, "large: {large} > {normal}");
    s.send(
        &Cfg {
            size: PointerSize::Larger,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    let larger = cursor_ink(&mut comp, id);
    assert!(larger > large, "larger: {larger} > {large}");
    assert!(larger > normal * 3, "larger: {larger} > 3 x {normal}");
    // Shrinking again leaves no trace of the bigger image.
    s.send(&Cfg::default().request());
    s.pump(&mut comp);
    assert_eq!(cursor_ink(&mut comp, id), normal);
}

#[test]
fn the_double_click_interval_follows_the_setting() {
    use ultar_hal::{InputEventKind, PointerButton};
    let mut comp = rig();
    let mut w = Peer::hello(&mut comp);
    w.window(&mut comp, 1, 0x00ff00);
    w.pump(&mut comp);
    w.take();
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    let click_title = |comp: &mut Compositor, now: u64| {
        let f = comp.state.windows[0].frame;
        common::move_to(comp, f64::from(f.x + f.w / 2), f64::from(f.y + 5));
        comp.state.now = now;
        let button = |pressed| {
            common::ev(InputEventKind::PointerButton {
                button: PointerButton::Primary,
                pressed,
            })
        };
        comp.inject(&[button(true), button(false)]);
    };
    // The default 400 ms: two presses 300 ms apart are a double click (maximize).
    click_title(&mut comp, 1_000);
    click_title(&mut comp, 1_300);
    assert_eq!(
        comp.state.windows[0].state,
        ultar_surface::proto::WindowState::Maximized
    );
    // 200 ms: 250 ms apart is too slow, 150 ms apart is a double click (restore).
    s.send(
        &Cfg {
            double_click_ms: 200,
            ..Cfg::default()
        }
        .request(),
    );
    s.pump(&mut comp);
    click_title(&mut comp, 2_000);
    click_title(&mut comp, 2_250);
    assert_eq!(
        comp.state.windows[0].state,
        ultar_surface::proto::WindowState::Maximized,
        "250 ms is outside the 200 ms interval"
    );
    click_title(&mut comp, 2_400);
    assert_eq!(
        comp.state.windows[0].state,
        ultar_surface::proto::WindowState::Normal
    );
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

#[test]
fn the_switch_shortcut_setting_leaves_only_the_chosen_combination() {
    let mut comp = rig();
    let mut w = Peer::hello(&mut comp);
    w.window(&mut comp, 1, 0x00ff00);
    w.pump(&mut comp);
    let ctrl_shift = |c: &mut Compositor| {
        common::key(c, 29, true);
        common::key(c, 42, true);
        common::key(c, 42, false);
        common::key(c, 29, false);
    };
    let super_space = |c: &mut Compositor| {
        common::key(c, 125, true);
        common::key(c, 57, true);
        common::key(c, 57, false);
        common::key(c, 125, false);
    };
    // Unconfigured: every combination switches, as before.
    ctrl_shift(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 1);
    // An app may not choose the combination.
    let mut app = Peer::hello_as(&mut comp, UID_APP);
    app.send(&Request::ConfigureKeyboard {
        switch_shortcut: SwitchShortcut::SuperSpace,
        per_window: false,
    });
    app.pump(&mut comp);
    assert_eq!(app.error(), Some(ErrorCode::Permission));
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    // Super+Space only: the chord does nothing, the space switches.
    s.send(&Request::ConfigureKeyboard {
        switch_shortcut: SwitchShortcut::SuperSpace,
        per_window: false,
    });
    s.pump(&mut comp);
    ctrl_shift(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 1);
    super_space(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 0);
    // Ctrl+Shift only: the space passes through, the chord switches.
    s.send(&Request::ConfigureKeyboard {
        switch_shortcut: SwitchShortcut::CtrlShift,
        per_window: false,
    });
    s.pump(&mut comp);
    super_space(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 0);
    ctrl_shift(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 1);
}

#[test]
fn per_window_languages_are_remembered_and_forgotten() {
    let mut comp = rig();
    let mut w1 = Peer::hello(&mut comp);
    w1.window(&mut comp, 1, 0x00ff00);
    let mut w2 = Peer::hello(&mut comp);
    w2.window(&mut comp, 2, 0x0000ff);
    w1.pump(&mut comp);
    w2.pump(&mut comp);
    let (k1, k2) = (comp.state.windows[0].key, comp.state.windows[1].key);
    let mut s = Peer::hello_as(&mut comp, UID_DISPLAYD);
    let alt_shift = |c: &mut Compositor| {
        common::key(c, 56, true);
        common::key(c, 42, true);
        common::key(c, 42, false);
        common::key(c, 56, false);
    };
    s.send(&Request::ConfigureKeyboard {
        switch_shortcut: SwitchShortcut::AltShift,
        per_window: true,
    });
    s.pump(&mut comp);
    comp.state.focus_window(k1);
    alt_shift(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 1, "w1 types English");
    comp.state.focus_window(k2);
    alt_shift(&mut comp);
    assert_eq!(comp.state.keyboard.layout_index, 0, "w2 types Arabic");
    comp.state.focus_window(k1);
    assert_eq!(comp.state.keyboard.layout_index, 1, "w1 gets English back");
    comp.state.focus_window(k2);
    assert_eq!(comp.state.keyboard.layout_index, 0, "w2 gets Arabic back");
    // Off: nothing is remembered any more, the layout stays as it is.
    s.send(&Request::ConfigureKeyboard {
        switch_shortcut: SwitchShortcut::AltShift,
        per_window: false,
    });
    s.pump(&mut comp);
    comp.state.focus_window(k1);
    assert_eq!(comp.state.keyboard.layout_index, 0, "no restore when off");
    alt_shift(&mut comp);
    comp.state.focus_window(k2);
    comp.state.focus_window(k1);
    assert_eq!(comp.state.keyboard.layout_index, 1, "nothing remembered");
}
