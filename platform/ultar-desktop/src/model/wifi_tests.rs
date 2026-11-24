use super::*;

pub(crate) fn net(ssid: &str, signal: u8, security: WifiSecurity) -> NetworkInfo {
    NetworkInfo {
        ssid: ssid.into(),
        bssid: format!("aa:bb:cc:dd:ee:{signal:02x}"),
        signal_percent: signal,
        frequency_mhz: 2412,
        security,
        known: false,
        joinable: !matches!(
            security,
            WifiSecurity::Wpa3Personal | WifiSecurity::Enterprise | WifiSecurity::Legacy
        ),
        connected: false,
    }
}

#[test]
fn one_row_per_name_connected_then_saved_then_strongest() {
    let mut nets = vec![
        net("cafe", 40, WifiSecurity::Open),
        net("home", 30, WifiSecurity::Wpa2Personal),
        net("home", 70, WifiSecurity::Wpa2Personal),
        net("", 99, WifiSecurity::Open),
        net("office", 90, WifiSecurity::Wpa2Personal),
        net("lab", 10, WifiSecurity::Wpa3Personal),
    ];
    nets[4].known = true;
    nets[1].connected = true;
    let l = listed(&nets);
    let names: Vec<&str> = l.iter().map(|n| n.ssid.as_str()).collect();
    assert_eq!(names, ["home", "office", "cafe", "lab"], "hidden left out");
    assert!(l[0].connected, "the connected access point wins its name");
    let many: Vec<NetworkInfo> = (0..40)
        .map(|i| net(&format!("n{i}"), i, WifiSecurity::Open))
        .collect();
    assert_eq!(listed(&many).len(), MAX_NETWORKS);
}

#[test]
fn a_click_joins_asks_leaves_or_does_nothing() {
    let mut nets = vec![
        net("cafe", 40, WifiSecurity::Open),
        net("home", 70, WifiSecurity::Wpa2Personal),
        net("office", 90, WifiSecurity::Wpa2Wpa3Personal),
        net("lab", 10, WifiSecurity::Wpa3Personal),
    ];
    nets[2].known = true;
    assert_eq!(click(&nets, "cafe"), NetworkClick::Join("cafe".into()));
    assert_eq!(
        click(&nets, "home"),
        NetworkClick::AskPassword("home".into(), WifiSecurity::Wpa2Personal)
    );
    assert_eq!(click(&nets, "office"), NetworkClick::Join("office".into()));
    assert_eq!(click(&nets, "lab"), NetworkClick::Nothing);
    assert_eq!(click(&nets, "gone"), NetworkClick::Nothing);
    nets[1].connected = true;
    assert_eq!(click(&nets, "home"), NetworkClick::Disconnect);
    assert_eq!(security_key(&nets[1]), "desk.wifi.connected");
    assert_eq!(security_key(&nets[0]), "desk.wifi.open");
    assert_eq!(security_key(&nets[2]), "desk.wifi.saved");
    assert_eq!(security_key(&nets[3]), "desk.wifi.unsupported");
    nets[0].known = true;
    assert_eq!(security_key(&nets[0]), "desk.wifi.saved_open");
    assert_eq!(
        security_key(&net("x", 1, WifiSecurity::Wpa2Personal)),
        "desk.wifi.secured"
    );
}

#[test]
fn bars_errors_tile_lines_and_vpn() {
    assert_eq!([0, 19, 20, 59, 60, 100].map(bars), [0, 0, 1, 2, 3, 4]);
    assert_eq!(
        join_error_key(JoinOutcome::WrongPassword),
        "desk.wifi.join.wrong_password"
    );
    let mut link = WifiLink {
        phase: WifiPhase::Connected,
        ssid: Some("KM-Office".into()),
        ..WifiLink::default()
    };
    assert_eq!(connected_ssid(Some(&link)).as_deref(), Some("KM-Office"));
    link.phase = WifiPhase::Scanning;
    assert_eq!(connected_ssid(Some(&link)), None);
    assert_eq!(
        phase_key(WifiPhase::Scanning),
        Some("desk.wifi.phase.scanning")
    );
    assert_eq!(phase_key(WifiPhase::Off), None);
    let mut vpn = VpnState {
        available: false,
        unavailable: Some(VpnUnavailable::NoTun),
        profiles: Vec::new(),
        active: None,
        status: VpnStatus::Off,
        handshake_age_secs: None,
        rx_bytes: 0,
        tx_bytes: 0,
        dns: Vec::new(),
    };
    assert_eq!(vpn_reason(&vpn), Some("desk.reason.vpn_no_tun"));
    assert_eq!(vpn_key(&vpn), "desk.vpn.none");
    vpn.available = true;
    vpn.status = VpnStatus::Connecting;
    assert_eq!(vpn_reason(&vpn), None);
    assert!(vpn_on(&vpn));
    assert_eq!(vpn_key(&vpn), "desk.vpn.connecting");
}
