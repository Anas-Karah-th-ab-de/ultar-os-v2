//! The real `ultar-displayd` between the real settings service and the real
//! compositor: displays reported with their identity, Super+P and "Keep?",
//! and every way a waiting change goes back – the asking app ends, the
//! display service ends (at once, and again after its restart), the
//! settings service hangs (the compositor's own deadline). Runs as root
//! and as another user (umask 002 too).

mod common;

use common::*;
use std::time::Duration;
use ultar_bus::messages::display_service::{
    code, Apply, Change, Detect, Identify, Keep, Mode, Outputs, Projection, Revert,
};
use ultar_bus::messages::settings::{Get, Set, SettingValue};
use ultar_bus::{BusError, Client, ErrorCode};

fn apply(app: &Client, p: Projection) -> Vec<String> {
    let reply = app
        .call(
            &Apply {
                change: Change::Projection(p),
            },
            WAIT,
        )
        .unwrap();
    assert_eq!(reply.seconds, 15);
    reply.pending
}

fn setting(app: &Client, key: &str) -> SettingValue {
    app.call(&Get { key: key.into() }, WAIT)
        .unwrap()
        .value
        .unwrap()
}

fn text(t: &str) -> SettingValue {
    SettingValue::Text(t.into())
}

/// Waits until displayd reported both displays and arranged them.
fn ready(rig: &Rig, app: &Client) -> Outputs {
    let outputs = app.subscribe::<Outputs>().unwrap();
    let start = std::time::Instant::now();
    loop {
        let o = outputs.recv_timeout(WAIT).unwrap();
        if o.outputs.len() == rig.bench.lock().unwrap().outputs.len() {
            rig.wait_for("arranged", |s| {
                s.outputs.iter().filter(|o| o.2).count() == o.outputs.len() && !s.provisional
            });
            return o;
        }
        assert!(start.elapsed() < WAIT);
    }
}

#[test]
fn displays_are_reported_and_super_p_waits_for_keep() {
    let rig = Rig::new("keep");
    let _d = rig.start_displayd();
    let app = rig.client("app.settings");
    let outputs = ready(&rig, &app);
    let ids: Vec<&str> = outputs.outputs.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(
        ids,
        vec!["boe-0010-100", "del-0011-101"],
        "keyed by the displays' own data"
    );
    assert_eq!(outputs.outputs[0].modes.len(), 2);
    assert!(outputs.outputs[0].internal && outputs.outputs[0].diagonal_tenths > 0);
    let pending = apply(&app, Projection::SecondOnly);
    assert_eq!(pending, vec!["display.mode", "display.single_display"]);
    let s = rig.wait_for("only the second display", |s| {
        s.enabled() == vec![1] && s.provisional
    });
    assert!(s.outputs[1].3, "the remaining display is the main one");
    assert_eq!(app.call(&Keep { more: false }, WAIT).unwrap().resolved, 2);
    rig.wait_for("confirmed", |s| !s.provisional && s.enabled() == vec![1]);
    rig.advance_settings(20);
    rig.advance_compositor(60_000);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(rig.snap().enabled(), vec![1], "kept");
    assert_eq!(setting(&app, "display.mode"), text("single"));
    let nothing = app.call(&Keep { more: false }, WAIT);
    assert!(
        matches!(nothing, Err(BusError::Remote(ErrorCode::Service(c))) if c == code::NOT_PENDING)
    );
}

#[test]
fn keep_more_restarts_and_revert_goes_back_now() {
    let rig = Rig::new("more");
    let _d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    let pending = apply(&app, Projection::ThisDeviceOnly).len() as u32;
    rig.wait_for("only this device", |s| {
        s.enabled() == vec![0] && s.provisional
    });
    rig.advance_settings(10);
    assert_eq!(
        app.call(&Keep { more: true }, WAIT).unwrap().resolved,
        pending
    );
    rig.advance_settings(10);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        rig.snap().enabled(),
        vec![0],
        "20 s after the change, still waiting"
    );
    assert_eq!(app.call(&Revert {}, WAIT).unwrap().resolved, pending);
    rig.wait_for("back to both", |s| {
        s.enabled() == vec![0, 1] && !s.provisional
    });
    assert_eq!(setting(&app, "display.mode"), text("extend"));
}

#[test]
fn the_asking_app_ending_takes_the_change_back() {
    let rig = Rig::new("app-gone");
    let _d = rig.start_displayd();
    let watcher = rig.client("app.settings");
    ready(&rig, &watcher);
    {
        let app = rig.client("app.settings");
        apply(&app, Projection::Mirror);
        rig.wait_for("mirrored", |s| {
            s.provisional && s.outputs.iter().all(|o| o.2 && o.4 == 0)
        });
    } // the app ends without answering
    rig.advance_settings(15);
    rig.wait_for("extended again", |s| !s.provisional && s.outputs[1].4 > 0);
    assert_eq!(setting(&watcher, "display.mode"), text("extend"));
}

#[test]
fn the_display_service_ending_takes_the_change_back_now_and_after_its_restart() {
    let rig = Rig::new("displayd-gone");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    apply(&app, Projection::SecondOnly);
    rig.wait_for("second only", |s| s.enabled() == vec![1] && s.provisional);
    drop(d); // kill -9
    rig.wait_for("compositor went back at once", |s| {
        s.enabled() == vec![0, 1] && !s.provisional
    });
    assert_eq!(
        setting(&app, "display.mode"),
        text("single"),
        "settingsd still waits"
    );
    let _d = rig.start_displayd();
    eventually("the waiting change was taken back at start", || {
        setting(&app, "display.mode") == text("extend")
    });
    rig.wait_for("still both", |s| {
        s.enabled() == vec![0, 1] && !s.provisional
    });
}

#[test]
fn a_hung_settings_service_cannot_keep_a_change() {
    let mut rig = Rig::new("settings-gone");
    let _d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    apply(&app, Projection::ThisDeviceOnly);
    rig.wait_for("this device only", |s| {
        s.enabled() == vec![0] && s.provisional
    });
    rig.stop_settings(); // nobody will ever take it back or keep it
    rig.advance_compositor(15_000);
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        rig.snap().enabled(),
        vec![0],
        "the grace time is not over yet"
    );
    rig.advance_compositor(3_001);
    rig.wait_for("the compositor went back by itself", |s| {
        s.enabled() == vec![0, 1] && !s.provisional
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        rig.snap().enabled(),
        vec![0, 1],
        "and the display service does not undo it"
    );
    // The settings service comes back: it takes the change back at start.
    rig.start_settings();
    eventually("settings back to extend", || {
        setting(&app, "display.mode") == text("extend")
    });
    rig.wait_for("both still on", |s| {
        s.enabled() == vec![0, 1] && !s.provisional
    });
}

#[test]
fn identify_detect_hot_plug_and_permissions() {
    let rig = Rig::new("misc");
    let _d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    assert_eq!(
        app.call(&Identify { order: vec![] }, WAIT).unwrap().shown,
        2
    );
    rig.wait_for("numbers shown", |s| s.identify == 2);
    rig.advance_compositor(5_000);
    rig.wait_for("numbers gone", |s| s.identify == 0);
    rig.plug(display(2, false));
    let outputs = app.subscribe::<Outputs>().unwrap();
    eventually("three displays published", || {
        outputs
            .recv_timeout(Duration::from_millis(200))
            .is_ok_and(|o| o.outputs.len() == 3)
    });
    rig.wait_for("all three on", |s| s.enabled() == vec![0, 1, 2]);
    let found = app.call(&Detect {}, WAIT).unwrap();
    assert_eq!((found.found, found.new), (3, 0));
    let unknown = Apply {
        change: Change::Mode {
            id: "nope".into(),
            mode: Mode {
                width: 1,
                height: 1,
                refresh_mhz: 1,
            },
        },
    };
    let r = app.call(&unknown, WAIT);
    assert!(
        matches!(r, Err(BusError::Remote(ErrorCode::Service(c))) if c == code::UNKNOWN_DISPLAY)
    );
    let evil = rig.client("app.evil");
    let r = evil.call(
        &Apply {
            change: Change::Projection(Projection::Mirror),
        },
        WAIT,
    );
    assert!(matches!(
        r,
        Err(BusError::Remote(ErrorCode::PermissionDenied))
    ));
}

#[test]
fn a_mode_change_switches_the_display_and_waits_too() {
    let rig = Rig::new("mode");
    let _d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    let mode = Apply {
        change: Change::Mode {
            id: "del-0011-101".into(),
            mode: Mode {
                width: 1024,
                height: 768,
                refresh_mhz: 60_000,
            },
        },
    };
    app.call(&mode, WAIT).unwrap();
    rig.wait_for("1024 wide", |s| s.outputs[1].1 == 1024 && s.provisional);
    app.call(&Keep { more: false }, WAIT).unwrap();
    rig.wait_for("kept", |s| !s.provisional && s.outputs[1].1 == 1024);
}

fn set(app: &Client, key: &str, value: SettingValue) {
    let call = Set {
        key: key.into(),
        value,
    };
    app.call(&call, WAIT).unwrap();
}

#[test]
fn pointer_and_key_repeat_settings_reach_the_compositor() {
    let rig = Rig::new("input");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    // The schema's defaults arrive with the connection.
    rig.wait_for("defaults", |s| {
        s.pointer == PointerSnap::default() && s.key_repeat == (true, 500, 25)
    });
    set(&app, "pointer.speed", SettingValue::Integer(6));
    set(&app, "pointer.natural_scroll", SettingValue::Flag(true));
    rig.wait_for("pointer", |s| {
        s.pointer
            == PointerSnap {
                speed: 6,
                natural_scroll: true,
                ..PointerSnap::default()
            }
    });
    set(&app, "keyboard.repeat.delay_ms", SettingValue::Integer(300));
    set(
        &app,
        "keyboard.repeat.per_second",
        SettingValue::Integer(10),
    );
    rig.wait_for("repeat", |s| s.key_repeat == (true, 300, 10));
    set(&app, "keyboard.repeat.enabled", SettingValue::Flag(false));
    rig.wait_for("repeat off", |s| s.key_repeat == (false, 300, 10));
    // A setting changed while the display service is away arrives when it is back.
    drop(d);
    set(&app, "pointer.speed", SettingValue::Integer(-3));
    let _d = rig.start_displayd();
    rig.wait_for("after the restart", |s| {
        s.pointer
            == PointerSnap {
                speed: -3,
                natural_scroll: true,
                ..PointerSnap::default()
            }
            && s.key_repeat == (false, 300, 10)
    });
}

#[test]
fn the_keyboard_switch_and_per_window_reach_the_compositor() {
    use ultar_surface::proto::SwitchShortcut;
    let rig = Rig::new("kbd");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    // The schema's defaults arrive with the connection.
    rig.wait_for("defaults", |s| {
        s.keyboard == (Some(SwitchShortcut::SuperSpace), false)
    });
    set(&app, "keyboard.switch_shortcut", text("alt_shift"));
    set(&app, "keyboard.per_window", SettingValue::Flag(true));
    rig.wait_for("alt_shift and per window", |s| {
        s.keyboard == (Some(SwitchShortcut::AltShift), true)
    });
    // Changed while the display service is away: read again when it is back.
    drop(d);
    set(&app, "keyboard.switch_shortcut", text("ctrl_shift"));
    let _d = rig.start_displayd();
    rig.wait_for("after the restart", |s| {
        s.keyboard == (Some(SwitchShortcut::CtrlShift), true)
    });
}

#[test]
fn pointer_tuning_and_the_large_pointer_reach_the_compositor() {
    use ultar_surface::proto::PointerSize;
    let rig = Rig::new("tuning");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    rig.wait_for("defaults", |s| s.pointer == PointerSnap::default());
    // Button swap, acceleration, cursor size and the double-click time.
    set(&app, "pointer.primary_button", text("right"));
    set(&app, "pointer.acceleration", SettingValue::Flag(false));
    set(&app, "pointer.size", text("large"));
    set(
        &app,
        "accessibility.double_click_ms",
        SettingValue::Integer(700),
    );
    let tuned = PointerSnap {
        swap_buttons: true,
        acceleration: false,
        size: PointerSize::Large,
        double_click_ms: 700,
        ..PointerSnap::default()
    };
    rig.wait_for("pointer tuning", |s| s.pointer == tuned);
    // A large pointer forces at least the large size, even over "normal".
    set(&app, "pointer.size", text("normal"));
    set(
        &app,
        "accessibility.large_pointer",
        SettingValue::Flag(true),
    );
    rig.wait_for("large pointer", |s| {
        s.pointer
            == PointerSnap {
                size: PointerSize::Large,
                ..tuned
            }
    });
    // Changed while the display service is away: read again when it is back.
    drop(d);
    set(
        &app,
        "accessibility.double_click_ms",
        SettingValue::Integer(250),
    );
    let _d = rig.start_displayd();
    rig.wait_for("after the restart", |s| {
        s.pointer
            == PointerSnap {
                double_click_ms: 250,
                ..tuned
            }
    });
}

#[test]
fn the_accessibility_view_settings_reach_the_compositor() {
    use ultar_surface::proto::ColourFilter;
    let rig = Rig::new("visual");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    // The schema's defaults arrive with the connection.
    rig.wait_for("view defaults", |s| s.visual == VisualSnap::default());
    set(&app, "accessibility.zoom", SettingValue::Flag(true));
    set(
        &app,
        "accessibility.zoom_percent",
        SettingValue::Integer(400),
    );
    set(&app, "accessibility.colour_filter", text("deuteranopia"));
    let wanted = VisualSnap {
        zoom: true,
        zoom_percent: 400,
        filter: ColourFilter::Deuteranopia,
    };
    rig.wait_for("magnifier and filter", |s| s.visual == wanted);
    // A runtime change of one setting keeps the others.
    set(&app, "accessibility.colour_filter", text("invert"));
    rig.wait_for("filter changed", |s| {
        s.visual
            == VisualSnap {
                filter: ColourFilter::Invert,
                ..wanted
            }
    });
    // Changed while the display service is away: read again when it is back.
    drop(d);
    set(
        &app,
        "accessibility.zoom_percent",
        SettingValue::Integer(250),
    );
    let _d = rig.start_displayd();
    rig.wait_for("after the restart", |s| {
        s.visual
            == VisualSnap {
                zoom_percent: 250,
                filter: ColourFilter::Invert,
                ..wanted
            }
    });
}

#[test]
fn keyboard_helpers_and_the_compose_key_reach_the_compositor() {
    use ultar_surface::proto::ComposeKey;
    let rig = Rig::new("helpers");
    let d = rig.start_displayd();
    let app = rig.client("app.settings");
    ready(&rig, &app);
    // The schema's defaults arrive with the connection.
    rig.wait_for("defaults", |s| {
        s.key_helpers == (false, false, false, false, ComposeKey::None)
    });
    set(&app, "accessibility.sticky_keys", SettingValue::Flag(true));
    set(&app, "accessibility.slow_keys", SettingValue::Flag(true));
    set(&app, "accessibility.bounce_keys", SettingValue::Flag(true));
    set(&app, "accessibility.mouse_keys", SettingValue::Flag(true));
    set(&app, "keyboard.compose_key", text("menu"));
    rig.wait_for("helpers on", |s| {
        s.key_helpers == (true, true, true, true, ComposeKey::Menu)
    });
    // A runtime change of the compose key.
    set(&app, "keyboard.compose_key", text("alt_gr"));
    rig.wait_for("compose alt_gr", |s| s.key_helpers.4 == ComposeKey::AltGr);
    // Changed while the display service is away: read again when it is back.
    drop(d);
    set(&app, "accessibility.sticky_keys", SettingValue::Flag(false));
    set(&app, "keyboard.compose_key", text("caps_lock"));
    let _d = rig.start_displayd();
    rig.wait_for("after the restart", |s| {
        s.key_helpers == (false, true, true, true, ComposeKey::CapsLock)
    });
}
