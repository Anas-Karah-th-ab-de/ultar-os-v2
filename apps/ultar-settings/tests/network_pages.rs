//! Settings › Network and Settings › Wi-Fi each show their own interface from `net.state`
//! (before, both pages showed the same rows): Network the wired interfaces with MAC, address,
//! router, DNS and speed, Wi-Fi the Wi-Fi interface – or "No Wi-Fi adapter" on a machine
//! without one (a VMware guest with only a wired card). At 1024x768, in ar/en/de.

mod common;

use common::*;
use ultar_bus::messages::net::{
    AddressMethod, Internet, LinkKind, LinkState, NetLink, NetState, Wired,
};
use ultar_i18n::Lang;
use ultar_ui::tokens::ThemeKind;

fn texts(app: &mut ultar_settings::App) -> Vec<String> {
    app.ui
        .a11y_tree()
        .iter()
        .filter_map(|n| n.name.as_ref())
        .map(|l| l.resolve(app.ui.translator()).into_owned())
        .collect()
}

fn page(net: &NetState, lang: Lang, area: &str) -> Vec<String> {
    let world = board_world(1, false);
    world.borrow_mut().net = Some(net.clone());
    let mut app = app(
        &world,
        lang,
        ThemeKind::Dark,
        content_size(1024.0, 768.0),
        area,
    );
    assert_eq!(app.state.status.error, None, "{area}");
    texts(&mut app)
}

fn has(texts: &[String], part: &str) -> bool {
    texts.iter().any(|t| t.contains(part))
}

fn eth() -> NetLink {
    NetLink {
        name: "eth0".into(),
        kind: LinkKind::Ethernet,
        mac: "00:0c:29:aa:bb:cc".into(),
        driver: Some("vmxnet3".into()),
        state: LinkState::Connected,
        speed_mbit: Some(10_000),
        method: AddressMethod::Dhcp,
        address: Some("192.168.100.50/24".into()),
        gateway: Some("192.168.100.1".into()),
        dns: vec!["192.168.100.1".into(), "1.1.1.1".into()],
        lease_secs: Some(3600),
        ipv6: Vec::new(),
    }
}

fn wlan() -> NetLink {
    NetLink {
        name: "wlan0".into(),
        kind: LinkKind::Wifi,
        mac: "a4:b1:c1:00:11:22".into(),
        driver: None,
        state: LinkState::Connected,
        speed_mbit: Some(866),
        method: AddressMethod::Dhcp,
        address: Some("10.20.30.40/24".into()),
        gateway: Some("10.20.30.1".into()),
        dns: vec!["10.20.30.1".into()],
        lease_secs: None,
        ipv6: Vec::new(),
    }
}

fn both() -> NetState {
    NetState {
        wifi_present: true,
        wifi_on: true,
        ssid: Some("Werkstatt".into()),
        signal_percent: Some(71),
        wired: Some(Wired {
            speed_mbit: Some(10_000),
        }),
        ethernet: vec![eth()],
        wifi_link: Some(wlan()),
        internet: Internet::Connected,
        ..NetState::default()
    }
}

#[test]
fn network_and_wifi_pages_show_their_own_interface() {
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        let net = page(&both(), lang, "network");
        for own in [
            "eth0 (vmxnet3)",
            "00:0c:29:aa:bb:cc",
            "192.168.100.50/24",
            "192.168.100.1, 1.1.1.1",
            "10000 Mbit/s",
        ] {
            assert!(has(&net, own), "{lang:?} network: {own} in {net:?}");
        }
        for other in ["wlan0", "a4:b1:c1:00:11:22", "10.20.30.40", "Werkstatt"] {
            assert!(!has(&net, other), "{lang:?} network shows Wi-Fi: {other}");
        }
        let wifi = page(&both(), lang, "wifi");
        for own in [
            "wlan0",
            "a4:b1:c1:00:11:22",
            "10.20.30.40/24",
            "10.20.30.1",
            "Werkstatt",
            "71 %",
        ] {
            assert!(has(&wifi, own), "{lang:?} wifi: {own} in {wifi:?}");
        }
        for other in ["eth0", "00:0c:29:aa:bb:cc", "192.168.100.50", "vmxnet3"] {
            assert!(!has(&wifi, other), "{lang:?} wifi shows the cable: {other}");
        }
    }
}

#[test]
fn wifi_page_says_no_adapter_and_network_page_says_no_cable() {
    let tr = |lang, key: &str| translator(lang).t(key).into_owned();
    for lang in [Lang::Ar, Lang::En, Lang::De] {
        // a VMware guest: one wired card, no Wi-Fi
        let wired_only = NetState {
            wifi_link: None,
            wifi_present: false,
            wifi_on: false,
            ssid: None,
            signal_percent: None,
            ..both()
        };
        let wifi = page(&wired_only, lang, "wifi");
        let none = tr(lang, "app.settings.status.net.no_wifi_adapter");
        assert!(has(&wifi, &none), "{lang:?}: {none} in {wifi:?}");
        for other in ["eth0", "192.168.100.50", "00:0c:29:aa:bb:cc"] {
            assert!(!has(&wifi, other), "{lang:?} wifi shows the cable: {other}");
        }
        let net = page(&wired_only, lang, "network");
        assert!(has(&net, "192.168.100.50/24"), "{lang:?}");
        assert!(!has(&net, &none), "{lang:?}");

        // cable pulled: the interface stays, with its state and without an address
        let mut pulled = wired_only.clone();
        pulled.wired = None;
        pulled.internet = Internet::Offline;
        pulled.ethernet[0] = NetLink {
            state: LinkState::NoCarrier,
            speed_mbit: None,
            address: None,
            gateway: None,
            dns: Vec::new(),
            lease_secs: None,
            ..eth()
        };
        let net = page(&pulled, lang, "network");
        assert!(has(&net, "eth0 (vmxnet3)"), "{lang:?}");
        assert!(has(&net, &tr(lang, "app.settings.status.net.no_cable")));
        assert!(has(
            &net,
            &tr(lang, "app.settings.status.net.internet.offline")
        ));
        assert!(!has(&net, "192.168.100.50"), "{lang:?}");

        // no wired card: the Network page says so
        let wifi_only = NetState {
            ethernet: Vec::new(),
            wired: None,
            ..both()
        };
        let net = page(&wifi_only, lang, "network");
        assert!(has(
            &net,
            &tr(lang, "app.settings.status.net.no_wired_adapter")
        ));
        assert!(!has(&net, "wlan0"), "{lang:?}");
    }
}
