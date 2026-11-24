//! The controls that work through the system services (docs/integration/system-services.md):
//! each is enabled from its service's report and makes exactly its contract's call; without a
//! report it stays disabled with its reason.

mod common;

use common::*;
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::power::PowerActions;
use ultar_bus::messages::session::SessionState;
use ultar_bus::messages::volumes::Mounted;
use ultar_desktop::model::quick::TileId;
use ultar_desktop::model::services::Call;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::{Effect, Opening, Outcome};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::event::ActionKind;
use ultar_ui::tokens::ThemeKind;

fn net(wifi_on: bool, airplane: bool) -> NetState {
    NetState {
        wifi_present: true,
        wifi_on,
        wifi_switch_off: false,
        ssid: None,
        signal_percent: None,
        wired: None,
        airplane,
        airplane_possible: true,
        metered: false,
        networks_supported: false,
    }
}

fn served() -> Harness {
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.svc.net = Some(net(true, false));
    d.svc.bluetooth = Some(BluetoothState {
        present: true,
        on: false,
        switch_off: false,
        connected: 0,
        devices_supported: false,
        scanning: false,
        pairing: None,
        found: 0,
        devices_revision: 0,
    });
    d.svc.audio = Some(AudioState {
        present: true,
        volume_percent: 40,
        muted: false,
        output_name: "Speakers".into(),
        output_id: Some("hw0".into()),
        has_volume: true,
    });
    d.svc.power = Some(PowerActions {
        suspend: false,
        restart: true,
        power_off: true,
        needs_admin: false,
    });
    d.svc.session = Some(SessionState {
        user: Some("anas".into()),
        signed_in: vec!["anas".into()],
    });
    d.presence.net_service = true;
    d.presence.audio_service = true;
    d.presence.bus = true;
    let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
    h.click_bar(Target::Status);
    h.take();
    h
}

fn enabled(h: &Harness, t: &Target) -> bool {
    let p = h.popup.as_ref().unwrap();
    p.view.node(t).is_some_and(|n| !p.view.ui.is_disabled(n))
}

#[test]
fn the_radios_switch_through_their_services() {
    let mut h = served();
    for (tile, call) in [
        (TileId::Wifi, Call::SetWifi(false)),
        (TileId::Airplane, Call::SetAirplane(true)),
        (TileId::Bluetooth, Call::SetBluetooth(true)),
    ] {
        assert!(enabled(&h, &Target::Tile(tile)), "{tile:?}");
        h.click_popup(Target::Tile(tile));
        assert_eq!(h.take(), vec![Effect::Service(call)], "{tile:?}");
    }
    // Choosing a network or a device waits for the service's round 2.
    let p = h.popup.as_ref().unwrap();
    let more = p.view.node(&Target::TileMore(TileId::Wifi)).unwrap();
    assert!(!p.view.ui.is_disabled(more) && p.view.ui.tooltip(more).is_some());
    // Airplane mode on: Wi-Fi and Bluetooth say why they are off, and a click opens
    // their page instead of switching.
    h.shell.d.svc.net = Some(net(false, true));
    h.apply(Outcome::surfaces());
    for (tile, page) in [
        (TileId::Wifi, QuickPage::Wifi),
        (TileId::Bluetooth, QuickPage::Bluetooth),
    ] {
        let p = h.popup.as_ref().unwrap();
        let n = p.view.node(&Target::Tile(tile)).unwrap();
        assert_eq!(
            p.view
                .ui
                .tooltip(n)
                .and_then(|l| l.key_name().map(str::to_owned)),
            Some("desk.reason.airplane".to_owned()),
            "{tile:?}"
        );
        h.click_popup(Target::Tile(tile));
        assert_eq!(h.shell.d.open, Open::Quick(page), "{tile:?}");
        assert!(
            !h.take()
                .iter()
                .any(|e| matches!(e, Effect::Service(Call::SetWifi(_) | Call::SetBluetooth(_)))),
            "{tile:?}: no switch while airplane mode is on"
        );
        h.click_popup(Target::QuickBack);
    }
}

#[test]
fn volume_output_and_screenshot() {
    let mut h = served();
    let out = h
        .shell
        .on_popup(&Target::Volume, &ActionKind::ValueChanged(65.0));
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Service(Call::SetVolume(65))]);
    h.click_popup(Target::Output);
    assert_eq!(h.shell.d.open, Open::OutputMenu);
    assert_eq!(h.take(), vec![Effect::Service(Call::Outputs)]);
    h.shell.d.svc.set_outputs(vec![
        ("hw0".into(), "Speakers".into(), true),
        ("hdmi".into(), "Display".into(), false),
    ]);
    let out = h.shell.on_menu(401);
    h.apply(out);
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::SetOutput("hdmi".into()))]
    );
    h.shell.d.open = Open::Quick(QuickPage::Tiles);
    h.apply(Outcome::surfaces());
    h.click_popup(Target::Screenshot);
    assert_eq!(h.take(), vec![Effect::Service(Call::Screenshot)]);
    // Without a sound device the controls say why.
    h.shell.d.svc.audio.as_mut().unwrap().present = false;
    h.shell.d.open = Open::Quick(QuickPage::Tiles);
    h.apply(Outcome::surfaces());
    assert!(!enabled(&h, &Target::Volume) && !enabled(&h, &Target::Output));
}

#[test]
fn the_power_menu_follows_power_actions_and_the_session() {
    let mut h = served();
    h.click_popup(Target::Power);
    assert_eq!(h.shell.d.open, Open::PowerMenu);
    let calls: Vec<Vec<Effect>> = (200..205)
        .map(|id| {
            h.shell.d.open = Open::PowerMenu;
            let out = h.shell.on_menu(id);
            h.apply(out);
            h.take()
        })
        .collect();
    assert_eq!(calls[1], vec![Effect::Service(Call::Restart)]);
    assert_eq!(calls[2], vec![Effect::Service(Call::PowerOff)]);
    assert_eq!(calls[3], vec![Effect::Service(Call::SignOut)]);
    assert_eq!(calls[4], vec![Effect::Lock], "switch user: the lock screen");
    let entries = h.shell.d.svc.power_entries();
    assert_eq!(
        entries,
        (false, true, true, true),
        "no sleep on this machine"
    );
}

#[test]
fn a_mounted_drive_is_a_bar_tile_that_opens_and_ejects() {
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    d.svc.set_drives(vec![Mounted {
        id: "sdb1".into(),
        name: "USB".into(),
        uri: "file:///media/anas/USB".into(),
    }]);
    let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
    assert!(h.bar.node(&Target::Drive(0)).is_some());
    h.click_bar(Target::Drive(0));
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.files".into(),
            open: Opening::uri("file:///media/anas/USB"),
        }]
    );
    let out = h.shell.on_bar_menu(&Target::Drive(0));
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::DriveMenu(0));
    let out = h.shell.on_menu(108);
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Service(Call::Eject("sdb1".into()))]);
    // No drive service: no tile.
    let h = Harness::with(
        board_desktop(Lang::Ar, ThemeKind::Dark),
        Lang::Ar,
        ThemeKind::Dark,
    );
    assert!(h.bar.node(&Target::Drive(0)).is_none());
}

#[test]
fn dismissing_a_notification_tells_the_service() {
    let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
    board_notes(&mut d, Lang::Ar);
    let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
    h.click_bar(Target::Clock);
    h.take();
    h.click_popup(Target::Dismiss(101));
    assert_eq!(h.take(), vec![Effect::Service(Call::NoteDismiss(101))]);
    // The board's buttons are drawn (note.action).
    assert!(h.popup_target(&Target::NoteAction(100, 1)));
}

fn device(id: &str, paired: bool, connected: bool) -> ultar_bus::messages::bluetooth::Device {
    ultar_bus::messages::bluetooth::Device {
        id: id.into(),
        name: format!("dev {id}"),
        connected,
        battery_percent: connected.then_some(80),
        paired,
        kind: ultar_bus::messages::bluetooth::DeviceKind::Keyboard,
        rssi: None,
        le: true,
        input: true,
        audio_unsupported: false,
    }
}

#[test]
fn the_bluetooth_devices_page_pairs_connects_and_forgets() {
    let mut h = served();
    {
        let bt = h.shell.d.svc.bluetooth.as_mut().unwrap();
        bt.on = true;
        bt.devices_supported = true;
    }
    h.apply(Outcome::surfaces());
    h.click_popup(Target::TileMore(TileId::Bluetooth));
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Bluetooth));
    assert_eq!(h.take(), vec![Effect::Service(Call::BtDevices)]);
    h.shell
        .d
        .svc
        .set_bt_devices(vec![device("new", false, false), device("kbd", true, true)]);
    h.apply(Outcome::surfaces());
    h.click_popup(Target::BtDevice("new".into()));
    assert_eq!(h.take(), vec![Effect::Service(Call::BtPair("new".into()))]);
    h.click_popup(Target::BtDevice("kbd".into()));
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::BtConnect("kbd".into(), false))]
    );
    h.click_popup(Target::BtForget("kbd".into()));
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::BtForget("kbd".into()))]
    );
    assert!(
        !h.popup_target(&Target::BtForget("new".into())),
        "only paired ones"
    );
    let out = h
        .shell
        .on_popup(&Target::BtScan, &ActionKind::Toggled(true));
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Service(Call::BtScan(true))]);
}

/// Every widget of the popup, as text (keys and data), for "is it shown".
fn popup_widgets(h: &Harness) -> String {
    let ui = &h.popup.as_ref().unwrap().view.ui;
    let mut out = String::new();
    let mut stack = vec![ui.root()];
    while let Some(n) = stack.pop() {
        if let Some(w) = ui.widget(n) {
            out.push_str(&format!("{w:?}\n"));
        }
        stack.extend(ui.children(n).iter().rev());
    }
    out
}

fn found(id: &str, name: &str, rssi: i8) -> ultar_bus::messages::bluetooth::Device {
    ultar_bus::messages::bluetooth::Device {
        name: name.into(),
        connected: false,
        battery_percent: None,
        paired: false,
        rssi: Some(rssi),
        input: false,
        ..device(id, false, false)
    }
}

#[test]
fn the_bluetooth_page_follows_a_scan_and_says_when_nothing_was_found() {
    let mut h = served();
    {
        let bt = h.shell.d.svc.bluetooth.as_mut().unwrap();
        bt.on = true;
        bt.devices_supported = true;
    }
    h.apply(Outcome::surfaces());
    h.click_popup(Target::TileMore(TileId::Bluetooth));
    h.take();
    let text = popup_widgets(&h);
    assert!(text.contains("desk.bt.none"), "before any scan: the hint");
    assert!(!text.contains("Progress"), "no busy bar");

    // scanning, nothing yet: busy bar and "searching"
    h.shell.d.svc.bluetooth.as_mut().unwrap().scanning = true;
    h.shell.d.svc.bt_searched = true;
    h.apply(Outcome::surfaces());
    let text = popup_widgets(&h);
    assert!(text.contains("Progress"), "busy bar while scanning");
    assert!(text.contains("desk.bt.searching"));

    // devices found while scanning: shown at once, named and nearer first, a nameless one by
    // its address, each pairable
    h.shell.d.svc.set_bt_devices(vec![
        found("C0:BB:AA:00:00:D4/r", "", -50),
        found("11:22:33:44:55:B2", "BT-Tastatur", -70),
        found("C0:BB:AA:00:00:C3/r", "BT-Maus", -60),
    ]);
    h.apply(Outcome::surfaces());
    let text = popup_widgets(&h);
    let at = |s: &str| text.find(s).unwrap_or_else(|| panic!("{s} not shown"));
    assert!(
        at("BT-Maus") < at("BT-Tastatur"),
        "the stronger signal first"
    );
    assert!(
        at("BT-Tastatur") < at("C0:BB:AA:00:00:D4/r"),
        "nameless last, by address"
    );
    assert!(text.contains("Progress"), "still searching");
    h.click_popup(Target::BtDevice("C0:BB:AA:00:00:C3/r".into()));
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::BtPair("C0:BB:AA:00:00:C3/r".into()))]
    );

    // the scan ended without a device: said so, no busy bar
    h.shell.d.svc.bluetooth.as_mut().unwrap().scanning = false;
    h.shell.d.svc.set_bt_devices(vec![]);
    h.apply(Outcome::surfaces());
    let text = popup_widgets(&h);
    assert!(text.contains("desk.bt.not_found"));
    assert!(!text.contains("Progress"));
}
