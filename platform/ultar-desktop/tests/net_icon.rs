//! The network icon of the bar follows the real state of the interfaces in `net.state`: the
//! cable only while a wired interface has an address, Wi-Fi while connected to a network,
//! otherwise offline. Before, the bar showed Wi-Fi whenever the network service ran, also on a
//! machine with only a cable (VMware) or with no connection at all.

mod common;

use common::*;
use ultar_bus::messages::net::{Internet, LinkKind, LinkState, NetLink, NetState, Wired};
use ultar_desktop::shell::status_icons;
use ultar_i18n::Lang;
use ultar_ui::icon::IconId;
use ultar_ui::tokens::ThemeKind;

fn eth(state: LinkState) -> NetLink {
    NetLink {
        name: "eth0".into(),
        kind: LinkKind::Ethernet,
        mac: "00:0c:29:12:34:56".into(),
        driver: Some("vmxnet3".into()),
        state,
        speed_mbit: (state != LinkState::NoCarrier).then_some(10_000),
        address: (state == LinkState::Connected).then(|| "192.168.100.50/24".to_owned()),
        ..NetLink::default()
    }
}

fn icon(net: Option<NetState>) -> IconId {
    let mut d = board_desktop(Lang::En, ThemeKind::Dark);
    d.presence.net_service = true;
    d.svc.net = net;
    let h = Harness::with(d, Lang::En, ThemeKind::Dark);
    status_icons(&h.shell)[0]
}

#[test]
fn bar_icon_shows_the_cable_only_with_an_address() {
    let wired = |state| NetState {
        ethernet: vec![eth(state)],
        wired: (state != LinkState::NoCarrier).then_some(Wired {
            speed_mbit: Some(10_000),
        }),
        internet: Internet::Connected,
        ..NetState::default()
    };
    assert_eq!(icon(Some(wired(LinkState::Connected))), IconId::Cable);
    // link up but DHCP has not answered: not connected yet
    assert_eq!(icon(Some(wired(LinkState::Connecting))), IconId::Offline);
    assert_eq!(icon(Some(wired(LinkState::NoCarrier))), IconId::Offline);
    // no adapter at all
    assert_eq!(icon(Some(NetState::default())), IconId::Offline);
}

#[test]
fn bar_icon_wifi_and_cable_together() {
    let wifi = NetLink {
        name: "wlan0".into(),
        kind: LinkKind::Wifi,
        state: LinkState::Connected,
        address: Some("192.168.1.7/24".into()),
        ..NetLink::default()
    };
    let mut s = NetState {
        wifi_present: true,
        wifi_on: true,
        ssid: Some("home".into()),
        wifi_link: Some(wifi),
        ethernet: vec![eth(LinkState::NoCarrier)],
        ..NetState::default()
    };
    assert_eq!(icon(Some(s.clone())), IconId::Wifi);
    // the cable wins while both are connected (it carries the default route)
    s.ethernet = vec![eth(LinkState::NoCarrier), eth(LinkState::Connected)];
    assert_eq!(icon(Some(s)), IconId::Cable);
    // a service of the first contract version: only the cable flag
    let old = NetState {
        wired: Some(Wired { speed_mbit: None }),
        ..NetState::default()
    };
    assert_eq!(icon(Some(old)), IconId::Cable);
    // the service runs but has not reported yet: as before
    assert_eq!(icon(None), IconId::Wifi);
    // no network service: offline
    let mut d = board_desktop(Lang::En, ThemeKind::Dark);
    d.svc.net = Some(NetState {
        ethernet: vec![eth(LinkState::Connected)],
        ..NetState::default()
    });
    let h = Harness::with(d, Lang::En, ThemeKind::Dark);
    assert_eq!(status_icons(&h.shell)[0], IconId::Offline);
}
