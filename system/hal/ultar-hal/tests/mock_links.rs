//! Exercises the network, Wi-Fi, Bluetooth, radio and satellite receiver traits
//! through the mock backend, including error paths and scripted failures.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use ultar_hal::mock::MockBackend;
use ultar_hal::{
    AccessTechnology, Backend, GnssFix, GnssFixKind, HalError, InterfaceKind, LinkState,
    ModemState, Registration, RegistrationState, SatelliteInfo, SignalStrength, SimState,
    WifiCredentials, WifiSecurity, WifiState,
};

#[test]
fn network_interfaces_links_and_addresses() {
    let laptop = MockBackend::laptop();
    let network = laptop.network().unwrap();
    let interfaces = network.interfaces().unwrap();
    let names: Vec<&str> = interfaces.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(names, vec!["lo", "eth0", "wlan0"]);
    assert_eq!(interfaces[0].kind, InterfaceKind::Loopback);
    assert_eq!(interfaces[0].mac, None);
    assert!(interfaces[1].mac.is_some());
    assert_ne!(interfaces[1].mac, interfaces[2].mac);

    let mock = &laptop.links.network;
    let v4 = IpAddr::V4(Ipv4Addr::new(192, 168, 100, 20));
    let v6 = IpAddr::V6(Ipv6Addr::LOCALHOST);
    mock.add_address("eth0", v4, 24).unwrap();
    mock.add_address("eth0", v6, 128).unwrap();
    let addresses = network.addresses("eth0").unwrap();
    assert_eq!(addresses.len(), 2);
    assert_eq!((addresses[0].address, addresses[0].prefix_len), (v4, 24));
    assert!(network.addresses("wlan0").unwrap().is_empty());

    assert_eq!(network.link_state("eth0").unwrap(), LinkState::Up);
    mock.set_carrier("eth0", false).unwrap();
    assert_eq!(network.link_state("eth0").unwrap(), LinkState::NoCarrier);
    network.set_link_up("eth0", false).unwrap();
    assert_eq!(network.link_state("eth0").unwrap(), LinkState::Down);
    // A cable plugged into an interface that is off does not bring it up.
    mock.set_carrier("eth0", true).unwrap();
    assert_eq!(network.link_state("eth0").unwrap(), LinkState::Down);
    network.set_link_up("eth0", true).unwrap();
    assert_eq!(network.link_state("eth0").unwrap(), LinkState::Up);
}

#[test]
fn network_errors() {
    let laptop = MockBackend::laptop();
    let network = laptop.network().unwrap();
    assert_eq!(network.link_state("eth9"), Err(HalError::NotFound));
    assert_eq!(network.addresses("eth9"), Err(HalError::NotFound));
    assert_eq!(network.set_link_up("eth9", true), Err(HalError::NotFound));
    let mock = &laptop.links.network;
    assert_eq!(mock.set_carrier("eth9", true), Err(HalError::NotFound));
    let address = IpAddr::V4(Ipv4Addr::LOCALHOST);
    assert_eq!(
        mock.add_address("eth9", address, 8),
        Err(HalError::NotFound)
    );
    mock.fail_next(HalError::PermissionDenied);
    assert_eq!(
        network.set_link_up("eth0", false),
        Err(HalError::PermissionDenied)
    );
    assert_eq!(network.link_state("eth0").unwrap(), LinkState::Up);
}

#[test]
fn wifi_scan_connect_disconnect() {
    let phone = MockBackend::phone();
    let wifi = phone.wifi().unwrap();
    let mock = &phone.links.wifi;
    assert!(wifi.is_powered().unwrap());
    assert!(wifi.scan().unwrap().is_empty());
    mock.add_network("home", Some("correct horse"), -48);
    mock.add_network("cafe", None, -71);

    let found = wifi.scan().unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].security, WifiSecurity::Wpa2);
    assert_eq!(found[1].security, WifiSecurity::Open);
    assert_ne!(found[0].bssid, found[1].bssid);

    assert_eq!(wifi.status().unwrap().state, WifiState::Disconnected);
    wifi.connect(&WifiCredentials::with_passphrase("home", "correct horse"))
        .unwrap();
    let status = wifi.status().unwrap();
    assert_eq!(status.state, WifiState::Connected);
    assert_eq!(status.ssid.as_deref(), Some("home"));
    assert_eq!(status.signal_dbm, Some(-48));
    assert!(status.frequency_mhz.is_some());

    wifi.connect(&WifiCredentials::open("cafe")).unwrap();
    assert_eq!(wifi.status().unwrap().ssid.as_deref(), Some("cafe"));
    wifi.disconnect().unwrap();
    assert_eq!(wifi.status().unwrap().state, WifiState::Disconnected);
    assert_eq!(wifi.status().unwrap().ssid, None);
}

#[test]
fn wifi_errors_and_power() {
    let phone = MockBackend::phone();
    let wifi = phone.wifi().unwrap();
    let mock = &phone.links.wifi;
    mock.add_network("home", Some("correct horse"), -48);
    let good = WifiCredentials::with_passphrase("home", "correct horse");

    let wrong = WifiCredentials::with_passphrase("home", "wrong horse!");
    assert_eq!(wifi.connect(&wrong), Err(HalError::Rejected));
    assert_eq!(
        wifi.connect(&WifiCredentials::open("home")),
        Err(HalError::Rejected)
    );
    assert_eq!(
        wifi.connect(&WifiCredentials::open("elsewhere")),
        Err(HalError::NotFound)
    );
    let short = WifiCredentials::with_passphrase("home", "short");
    assert_eq!(wifi.connect(&short), Err(HalError::InvalidArgument));
    assert_eq!(wifi.status().unwrap().state, WifiState::Disconnected);

    wifi.connect(&good).unwrap();
    wifi.set_powered(false).unwrap();
    assert!(!wifi.is_powered().unwrap());
    assert_eq!(wifi.status().unwrap().state, WifiState::Off);
    assert_eq!(wifi.scan(), Err(HalError::Rejected));
    assert_eq!(wifi.connect(&good), Err(HalError::Rejected));
    wifi.set_powered(true).unwrap();
    // Switching off dropped the connection.
    assert_eq!(wifi.status().unwrap().state, WifiState::Disconnected);

    wifi.connect(&good).unwrap();
    mock.clear_networks();
    assert_eq!(wifi.status().unwrap().state, WifiState::Disconnected);
    mock.fail_next(HalError::Timeout);
    assert_eq!(wifi.scan(), Err(HalError::Timeout));
    assert!(wifi.scan().unwrap().is_empty());
}

#[test]
fn bluetooth_power_and_scan() {
    let phone = MockBackend::phone();
    let bluetooth = phone.bluetooth().unwrap();
    let mock = &phone.links.bluetooth;
    mock.add_device(Some("Headphones"), true);
    mock.add_device(None, false);
    assert!(!bluetooth.is_powered().unwrap());
    assert_eq!(bluetooth.scan(1_000), Err(HalError::Rejected));
    bluetooth.set_powered(true).unwrap();
    assert!(bluetooth.is_powered().unwrap());
    let found = bluetooth.scan(1_000).unwrap();
    assert_eq!(found.len(), 2);
    assert_eq!(found[0].name.as_deref(), Some("Headphones"));
    assert!(found[0].paired && !found[1].paired);
    assert_eq!(found[1].name, None);
    assert_ne!(found[0].address, found[1].address);
    mock.fail_next(HalError::Busy);
    assert_eq!(bluetooth.set_powered(false), Err(HalError::Busy));
    assert!(bluetooth.is_powered().unwrap());
}

#[test]
fn radio_default_state_and_flight_mode() {
    let phone = MockBackend::phone();
    let radio = phone.radio().unwrap();
    assert_eq!(radio.modem_state().unwrap(), ModemState::Ready);
    assert_eq!(radio.sim_slots().unwrap(), 1);
    assert_eq!(radio.sim_state(0).unwrap(), SimState::Ready);
    assert_eq!(radio.sim_state(1), Err(HalError::NotFound));
    let signal = radio.signal().unwrap().unwrap();
    assert_eq!(signal.technology, AccessTechnology::Lte);
    assert_eq!(signal.bars(), 3);
    let registration = radio.registration().unwrap();
    assert!(registration.state.is_registered());
    assert!(registration.operator_code.is_some());

    radio.set_powered(false).unwrap();
    assert_eq!(radio.modem_state().unwrap(), ModemState::Off);
    assert_eq!(radio.signal().unwrap(), None);
    let offline = radio.registration().unwrap();
    assert_eq!(offline.state, RegistrationState::NotRegistered);
    assert_eq!(offline.operator_name, None);
    radio.set_powered(true).unwrap();
    assert_eq!(radio.signal().unwrap(), Some(signal));
}

#[test]
fn radio_scripted_states_and_errors() {
    let phone = MockBackend::phone();
    let radio = phone.radio().unwrap();
    let mock = &phone.links.radio;
    mock.set_sims(&[SimState::PinRequired, SimState::Absent]);
    assert_eq!(radio.sim_slots().unwrap(), 2);
    assert_eq!(radio.sim_state(0).unwrap(), SimState::PinRequired);
    assert_eq!(radio.sim_state(1).unwrap(), SimState::Absent);
    mock.set_signal(None);
    assert_eq!(radio.signal().unwrap(), None);
    mock.set_signal(Some(SignalStrength {
        dbm: -125,
        technology: AccessTechnology::Nr,
    }));
    assert_eq!(radio.signal().unwrap().map(|s| s.bars()), Some(0));
    mock.set_registration(Registration {
        state: RegistrationState::Roaming,
        operator_code: Some("41701".into()),
        operator_name: None,
        technology: AccessTechnology::Umts,
    });
    assert_eq!(
        radio.registration().unwrap().state,
        RegistrationState::Roaming
    );

    mock.set_modem_state(ModemState::Failed);
    assert_eq!(radio.set_powered(true), Err(HalError::Disconnected));
    mock.set_modem_state(ModemState::Absent);
    assert_eq!(radio.set_powered(true), Err(HalError::NotSupported));
    assert_eq!(radio.modem_state().unwrap(), ModemState::Absent);
    mock.fail_next(HalError::Timeout);
    assert_eq!(radio.modem_state(), Err(HalError::Timeout));
}

fn sample_fix() -> GnssFix {
    GnssFix {
        kind: GnssFixKind::ThreeDimensional,
        latitude_deg: 35.9306,
        longitude_deg: 36.6339,
        altitude_m: 446.0,
        accuracy_m: Some(3.5),
        speed_mps: 0.0,
        course_deg: 0.0,
        satellites_used: 2,
        unix_time_ms: 1_795_000_000_000,
    }
}

#[test]
fn gnss_reports_only_while_running() {
    let phone = MockBackend::phone();
    let gnss = phone.gnss().unwrap();
    let mock = &phone.links.gnss;
    let satellites = [
        SatelliteInfo {
            id: 7,
            elevation_deg: 55,
            azimuth_deg: 120,
            snr_dbhz: Some(41),
            used: true,
        },
        SatelliteInfo {
            id: 19,
            elevation_deg: 8,
            azimuth_deg: 300,
            snr_dbhz: None,
            used: false,
        },
    ];
    mock.set_fix(Some(sample_fix()));
    mock.set_satellites(&satellites);

    assert!(!gnss.is_running().unwrap());
    assert_eq!(gnss.fix().unwrap(), None);
    assert!(gnss.satellites().unwrap().is_empty());
    gnss.start().unwrap();
    assert!(gnss.is_running().unwrap());
    let fix = gnss.fix().unwrap().unwrap();
    assert!(fix.is_plausible());
    assert_eq!(fix, sample_fix());
    assert_eq!(gnss.satellites().unwrap(), satellites.to_vec());
    mock.set_fix(None);
    assert_eq!(gnss.fix().unwrap(), None);
    gnss.stop().unwrap();
    assert!(!gnss.is_running().unwrap());

    mock.fail_next(HalError::Io(110));
    assert_eq!(gnss.start(), Err(HalError::Io(110)));
    assert!(!gnss.is_running().unwrap());
}
