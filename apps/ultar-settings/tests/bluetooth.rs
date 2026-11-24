//! Settings › Bluetooth with a usable controller: the list follows a running search
//! (`devices_revision`), a busy bar while searching, a found device pairs on click, and a
//! search without result says so.

mod common;

use common::*;
use ultar_bus::messages::bluetooth::{BluetoothState, Device, DeviceKind};
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::App;
use ultar_ui::tokens::ThemeKind;

fn found(id: &str, name: &str, rssi: i8) -> Device {
    Device {
        id: id.into(),
        name: name.into(),
        connected: false,
        battery_percent: None,
        paired: false,
        kind: DeviceKind::Other,
        rssi: Some(rssi),
        le: true,
        input: false,
        audio_unsupported: false,
    }
}

fn state(scanning: bool, revision: u32, found: u16) -> BluetoothState {
    BluetoothState {
        present: true,
        on: true,
        devices_supported: true,
        scanning,
        found,
        devices_revision: revision,
        ..BluetoothState::default()
    }
}

/// Every widget of the window as text (keys and data), in tree order.
fn shown(app: &App) -> String {
    let mut out = String::new();
    let mut stack = vec![app.ui.root()];
    while let Some(n) = stack.pop() {
        if let Some(w) = app.ui.widget(n) {
            out.push_str(&format!("{w:?}\n"));
        }
        stack.extend(app.ui.children(n).iter().rev());
    }
    out
}

#[test]
fn the_list_follows_a_search_and_pairs_a_found_device() {
    let world = board_world(1, true);
    world.borrow_mut().bluetooth = Some(state(true, 1, 0));
    world.borrow_mut().bt_devices = Some(vec![]);
    let mut app = app(
        &world,
        Lang::En,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "bluetooth",
    );
    let text = shown(&app);
    assert!(text.contains("Progress"), "busy bar while searching");
    assert!(text.contains("app.settings.bt.searching"));
    assert!(app.view.node(Act::BtScan).is_some(), "the search switch");

    // the service found devices: the next poll shows them, nearer and named first
    world.borrow_mut().bt_devices = Some(vec![
        found("C0:BB:AA:00:00:D4/r", "", -50),
        found("11:22:33:44:55:B2", "BT-Tastatur", -70),
        found("C0:BB:AA:00:00:C3/r", "BT-Maus", -60),
    ]);
    world.borrow_mut().bluetooth = Some(state(true, 2, 3));
    app.poll().unwrap();
    let text = shown(&app);
    let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} not shown"));
    assert!(at("BT-Maus") < at("BT-Tastatur"));
    assert!(
        at("BT-Tastatur") < at("C0:BB:AA:00:00:D4/r"),
        "nameless by address, last"
    );
    let first = app.view.node(Act::BtPair(0)).expect("rows are clickable");
    click(&mut app, first);
    assert_eq!(
        log(&world).last().map(String::as_str),
        Some("bluetooth.pair C0:BB:AA:00:00:C3/r")
    );

    // the search ended without anything: said so
    world.borrow_mut().bt_devices = Some(vec![]);
    world.borrow_mut().bluetooth = Some(state(false, 3, 0));
    app.poll().unwrap();
    let text = shown(&app);
    assert!(text.contains("app.settings.bt.not_found"), "{text}");
    assert!(!text.contains("Progress"));
    let switch = app.view.node(Act::BtScan).unwrap();
    click(&mut app, switch);
    assert_eq!(
        log(&world).last().map(String::as_str),
        Some("bluetooth.scan true")
    );
}
