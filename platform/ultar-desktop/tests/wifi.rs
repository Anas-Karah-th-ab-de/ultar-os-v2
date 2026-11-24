//! qs.wifi.list, the join dialog, qs.vpn and qs.vpn.list (docs/integration/system-services.md
//! §3): each click makes exactly its contract's call; the password goes from the field into
//! `net.join` only; the networks and connections follow the service; desktop and phone sheet.

mod common;

use common::*;
use ultar_bus::messages::auth::Secret;
use ultar_bus::messages::net::{
    JoinOutcome, NetState, NetworkInfo, WifiLink, WifiPhase, WifiSecurity,
};
use ultar_bus::messages::vpn::{VpnProfile, VpnState, VpnStatus, VpnUnavailable};
use ultar_desktop::model::quick::TileId;
use ultar_desktop::model::services::Call;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::{Effect, Opening, Outcome};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::tokens::ThemeKind;

fn net_state() -> NetState {
    NetState {
        wifi_present: true,
        wifi_on: true,
        wifi_switch_off: false,
        ssid: Some("KM-Office".into()),
        signal_percent: Some(80),
        wired: None,
        airplane: false,
        airplane_possible: true,
        metered: false,
        networks_supported: true,
    }
}

fn info(
    ssid: &str,
    signal: u8,
    security: WifiSecurity,
    known: bool,
    connected: bool,
) -> NetworkInfo {
    NetworkInfo {
        ssid: ssid.into(),
        bssid: "aa:bb:cc:dd:ee:ff".into(),
        signal_percent: signal,
        frequency_mhz: 5180,
        security,
        known,
        joinable: security != WifiSecurity::Wpa3Personal,
        connected,
    }
}

fn vpn(profiles: &[&str], status: VpnStatus) -> VpnState {
    VpnState {
        available: true,
        unavailable: None,
        profiles: profiles
            .iter()
            .map(|n| VpnProfile {
                name: (*n).into(),
                endpoint: "vpn.code-craft.tech:51820".into(),
                addresses: vec!["10.0.0.2/32".into()],
                full_tunnel: true,
            })
            .collect(),
        active: profiles.first().map(|n| (*n).into()),
        status,
        handshake_age_secs: None,
        rx_bytes: 0,
        tx_bytes: 0,
        dns: Vec::new(),
    }
}

fn harness(phone: bool) -> Harness {
    let (d, shape, screen) = if phone {
        (
            board_phone(Lang::Ar, ThemeKind::Dark),
            ultar_desktop::model::Shape::COMPACT,
            PHONE,
        )
    } else {
        (
            board_desktop(Lang::Ar, ThemeKind::Dark),
            ultar_desktop::model::Shape::default(),
            SCREEN,
        )
    };
    let mut d = d;
    d.svc.net = Some(net_state());
    d.svc.wifi.link = Some(WifiLink {
        phase: WifiPhase::Connected,
        ssid: Some("KM-Office".into()),
        signal_percent: Some(80),
        address: Some("192.168.1.20/24".into()),
        router: None,
        dns: Vec::new(),
    });
    d.presence.net_service = true;
    d.presence.bus = true;
    d.presence.launcher = true;
    let mut h = Harness::shaped(d, Lang::Ar, ThemeKind::Dark, shape, screen);
    h.shell.d.open = Open::Quick(QuickPage::Tiles);
    h.apply(Outcome::surfaces());
    h.take();
    h
}

fn networks() -> Vec<NetworkInfo> {
    vec![
        info("KM-Office", 80, WifiSecurity::Wpa2Personal, true, true),
        info("Cafe", 50, WifiSecurity::Open, false, false),
        info("Home", 70, WifiSecurity::Wpa2Personal, false, false),
        info("Lab", 30, WifiSecurity::Wpa3Personal, false, false),
    ]
}

fn open_networks(h: &mut Harness) {
    open_networks_page(h);
    // WPA3-only: listed, disabled with the reason.
    let p = h.popup.as_ref().unwrap();
    let lab = p.view.node(&Target::WifiNetwork("Lab".into())).unwrap();
    assert!(p.view.ui.is_disabled(lab) && p.view.ui.tooltip(lab).is_some());
}

fn open_networks_page(h: &mut Harness) {
    h.click_popup(Target::TileMore(TileId::Wifi));
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Wifi));
    assert_eq!(
        h.take(),
        vec![
            Effect::Service(Call::Networks),
            Effect::Service(Call::Known)
        ]
    );
    assert!(
        h.popup_target(&Target::WifiSettings),
        "searching: settings stay"
    );
    h.shell.d.svc.wifi.set_networks(networks());
    h.shell.d.svc.wifi.set_known(vec!["KM-Office".into()]);
    h.apply(Outcome::surfaces());
}

fn flow(phone: bool) {
    let mut h = harness(phone);
    open_networks(&mut h);
    // Open network: joined at once; the connected one is left; a saved one is forgotten.
    h.click_popup(Target::WifiNetwork("Cafe".into()));
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::Join("Cafe".into(), None, true))]
    );
    h.click_popup(Target::WifiNetwork("KM-Office".into()));
    assert_eq!(h.take(), vec![Effect::Service(Call::Disconnect)]);
    h.click_popup(Target::WifiForget("KM-Office".into()));
    assert_eq!(
        h.take(),
        [
            Call::Forget("KM-Office".into()),
            Call::Known,
            Call::Networks
        ]
        .map(Effect::Service)
    );
    assert!(
        !h.popup_target(&Target::WifiForget("Cafe".into())),
        "only saved ones"
    );
    // A secured network asks for the password.
    h.click_popup(Target::WifiNetwork("Home".into()));
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::WifiJoin));
    assert!(h.popup_target(&Target::JoinPassword) && h.popup_target(&Target::JoinRemember));
    let out = h
        .shell
        .on_popup(&Target::JoinRemember, &ActionKind::Toggled(false));
    h.apply(out);
    let out = h.shell.join_with(Secret::default());
    assert!(out.effects.is_empty(), "an empty password is not sent");
    let out = h.shell.join_with(Secret::new("geheim123".into()));
    h.apply(out);
    let effects = h.take();
    assert_eq!(
        effects,
        vec![Effect::Service(Call::Join(
            "Home".into(),
            Some(Secret::new("geheim123".into())),
            false
        ))]
    );
    assert!(
        format!("{effects:?}").contains("<hidden>") && !format!("{effects:?}").contains("geheim")
    );
    assert!(
        h.shell
            .join_with(Secret::new("again".into()))
            .effects
            .is_empty(),
        "one at a time"
    );
    dialog(&mut h);
}

/// The join dialog after a try: wrong password, Esc, connected, a saved network's key that
/// no longer works, the radio's switch and "Wi-Fi settings".
fn dialog(h: &mut Harness) {
    // A wrong password: the dialog stays and says so.
    let (out, say) = h.shell.joined("Home", Some(JoinOutcome::WrongPassword));
    assert!(!say);
    h.apply(out);
    let p = h.popup.as_ref().unwrap();
    let field = p.view.node(&Target::JoinPassword).unwrap();
    assert!(p.view.ui.error(field).is_some() && !p.view.ui.is_disabled(field));
    // Esc goes back to the networks, the dialog is gone.
    h.key_popup(Key::Escape);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Wifi));
    assert!(h.shell.d.svc.wifi.joining.is_none());
    // Joining again, connected: back to the list, asked again.
    h.click_popup(Target::WifiNetwork("Home".into()));
    let out = h.shell.join_with(Secret::new("richtig123".into()));
    h.apply(out);
    h.take();
    let (out, _) = h.shell.joined("Home", Some(JoinOutcome::Connected));
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Wifi));
    assert_eq!(h.take(), [Call::Networks, Call::Known].map(Effect::Service));
    // A saved network whose key no longer works opens the dialog; others say it in a notice.
    let (out, say) = h.shell.joined("Cafe", Some(JoinOutcome::PasswordNeeded));
    assert!(!say);
    h.apply(out);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::WifiJoin));
    h.click_popup(Target::JoinCancel);
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Wifi));
    assert!(h.shell.joined("Cafe", Some(JoinOutcome::Timeout)).1);
    assert!(h.shell.joined("Cafe", None).1, "no answer: a notice");
    // The radio's switch and "Wi-Fi settings".
    let out = h
        .shell
        .on_popup(&Target::WifiRadio, &ActionKind::Toggled(false));
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Service(Call::SetWifi(false))]);
    h.click_popup(Target::WifiSettings);
    let launched = h.take();
    assert!(
        launched.contains(&Effect::Launch {
            app: "org.ultar.settings".into(),
            open: Opening::settings_page("wifi"),
        }),
        "{launched:?}"
    );
}

#[test]
fn the_networks_page_joins_asks_for_the_password_and_forgets() {
    flow(false);
}

#[test]
fn the_phone_sheet_shows_the_same_rows() {
    flow(true);
}

#[test]
fn the_wifi_tile_names_the_network_and_the_list_needs_a_device() {
    let mut h = harness(false);
    let tiles =
        ultar_desktop::model::quick::tiles(&h.shell.d.presence, &h.shell.d.values, &h.shell.d.svc);
    let wifi = tiles.iter().find(|t| t.id == TileId::Wifi).unwrap();
    assert_eq!(
        ultar_desktop::model::quick::tile_data(wifi, &h.shell.d.svc).as_deref(),
        Some("KM-Office")
    );
    h.shell.d.svc.net.as_mut().unwrap().networks_supported = false;
    h.apply(Outcome::surfaces());
    let p = h.popup.as_ref().unwrap();
    let more = p.view.node(&Target::TileMore(TileId::Wifi)).unwrap();
    assert!(p.view.ui.is_disabled(more));
    assert_eq!(
        p.view
            .ui
            .tooltip(more)
            .and_then(|l| l.key_name().map(str::to_owned)),
        Some("desk.reason.no_wifi_device".to_owned())
    );
}

#[test]
fn the_vpn_tile_and_its_connections() {
    let mut h = harness(false);
    // No VPN service: disabled with the reason.
    let p = h.popup.as_ref().unwrap();
    assert!(p
        .view
        .node(&Target::Tile(TileId::Vpn))
        .is_some_and(|n| p.view.ui.is_disabled(n)));
    // No TUN in the kernel: disabled with that reason.
    let mut no_tun = vpn(&[], VpnStatus::Off);
    no_tun.available = false;
    no_tun.unavailable = Some(VpnUnavailable::NoTun);
    h.shell.d.svc.vpn = Some(no_tun);
    h.apply(Outcome::surfaces());
    let p = h.popup.as_ref().unwrap();
    let tile = p.view.node(&Target::Tile(TileId::Vpn)).unwrap();
    assert_eq!(
        p.view
            .ui
            .tooltip(tile)
            .and_then(|l| l.key_name().map(str::to_owned)),
        Some("desk.reason.vpn_no_tun".to_owned())
    );
    // Nothing set up: the tile opens Settings > Network > VPN.
    h.shell.d.svc.vpn = Some(vpn(&[], VpnStatus::Off));
    h.apply(Outcome::surfaces());
    h.click_popup(Target::Tile(TileId::Vpn));
    assert!(h.take().contains(&Effect::Launch {
        app: "org.ultar.settings".into(),
        open: Opening::settings_page("network/vpn"),
    }));
    // Set up: the last used one on and off.
    h.shell.d.open = Open::Quick(QuickPage::Tiles);
    h.shell.d.svc.vpn = Some(vpn(&["office", "home"], VpnStatus::Off));
    h.apply(Outcome::surfaces());
    h.click_popup(Target::Tile(TileId::Vpn));
    assert_eq!(h.take(), vec![Effect::Service(Call::SetVpn(true, None))]);
    h.shell.d.svc.vpn = Some(vpn(&["office", "home"], VpnStatus::Up));
    h.apply(Outcome::surfaces());
    h.click_popup(Target::Tile(TileId::Vpn));
    assert_eq!(h.take(), vec![Effect::Service(Call::SetVpn(false, None))]);
    // qs.vpn.list: choose which one is used.
    h.click_popup(Target::TileMore(TileId::Vpn));
    assert_eq!(h.shell.d.open, Open::Quick(QuickPage::Vpn));
    h.click_popup(Target::VpnProfile("home".into()));
    assert_eq!(
        h.take(),
        vec![Effect::Service(Call::SetVpn(true, Some("home".into())))]
    );
    let out = h
        .shell
        .on_popup(&Target::VpnSwitch, &ActionKind::Toggled(false));
    h.apply(out);
    assert_eq!(h.take(), vec![Effect::Service(Call::SetVpn(false, None))]);
    h.click_popup(Target::VpnSettings);
    assert!(!h.take().is_empty());
}
