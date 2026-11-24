//! The Wi-Fi core against the access point and DHCP server of `core_fakes`.

use super::fakes::*;
use super::*;
use std::sync::mpsc::channel;
use std::sync::Arc;
use ultar_hal_netlink::nl80211::WifiEvent;
use ultar_netproto::wpa::{Pmk, Ptk};

#[test]
fn no_wifi_device_is_said_honestly() {
    let mut c = WifiCore::new(
        None,
        Box::new(FakeIp(Arc::default())),
        Saved::load("/nonexistent/w".into()),
        Box::new(|_| true),
    );
    assert!(!c.present());
    assert_eq!(c.status().phase, WifiPhase::NoDevice);
    let (tx, rx) = channel();
    c.command(Command::Scan(Box::new(move |a| tx.send(a).unwrap())), 0);
    assert_eq!(rx.recv().unwrap(), Err(NetOutcome::NoDevice));
    let (tx, rx) = channel();
    c.command(
        Command::Join {
            ssid: b"x".to_vec(),
            secret: None,
            remember: false,
            reply: Box::new(move |o| tx.send(o).unwrap()),
        },
        0,
    );
    assert_eq!(rx.recv().unwrap(), JoinOutcome::NoDevice);
    assert_eq!(c.next_deadline(), None);
}

#[test]
fn scan_lists_every_network_strongest_first() {
    let mut r = Rig::new("scan");
    let (tx, rx) = channel();
    r.core
        .command(Command::Scan(Box::new(move |a| tx.send(a).unwrap())), r.now);
    let list = r.wait(&rx).unwrap();
    let names: Vec<_> = list.iter().map(|s| (s.ssid.clone(), s.security)).collect();
    assert_eq!(
        names,
        vec![
            (b"home".to_vec(), Security::Wpa2Personal),
            (b"wpa3".to_vec(), Security::Wpa3Personal),
            (b"cafe".to_vec(), Security::Open),
        ]
    );
}

#[test]
fn an_open_network_gets_an_address_and_dns() {
    let mut r = Rig::new("open");
    assert_eq!(r.join("cafe", None, false), JoinOutcome::Connected);
    r.run(500);
    let s = r.core.status();
    assert_eq!(s.phase, WifiPhase::Connected);
    assert_eq!(s.ssid.as_deref(), Some("cafe"));
    assert_eq!(s.address.as_deref(), Some("192.168.7.50/24"));
    assert_eq!(s.dns, vec!["192.168.7.1".to_string()]);
    assert!(r.net.lock().unwrap().configured.is_some());
    assert!(r.core.saved.is_empty(), "not asked to remember");
}

#[test]
fn wpa2_handshake_installs_the_right_keys_and_opens_the_port() {
    let mut r = Rig::new("wpa2");
    assert_eq!(
        r.join("home", Some("correct horse battery"), true),
        JoinOutcome::Connected
    );
    let air = r.air.lock().unwrap();
    assert!(air.done, "message 4 checked by the access point");
    let pmk = Pmk::from_passphrase("correct horse battery", b"home").unwrap();
    let snonce = [1u8; 32]; // the rig's first random fill
    let ptk = Ptk::derive(&pmk, &[2, 0, 0, 0, 0, 1], &STA, &ANONCE, &snonce);
    assert_eq!(
        air.keys[0],
        (Some([2, 0, 0, 0, 0, 1]), 0, ptk.tk().to_vec())
    );
    assert_eq!(air.keys[1], (None, 1, GTK.to_vec()));
    assert_eq!(air.authorized, vec![[2, 0, 0, 0, 0, 1]]);
    drop(air);
    assert_eq!(r.core.saved.names(), vec!["home"]);
    let stored = std::fs::read(r.dir.join("wifi")).unwrap();
    assert!(!stored.windows(5).any(|w| w == b"horse"));
    // A new group key from the access point is installed; the same frame
    // again is a replay and installs nothing.
    r.air.lock().unwrap().rekey([0x99; 16]);
    r.run(200);
    assert_eq!(
        r.air.lock().unwrap().keys.last().unwrap(),
        &(None, 2, vec![0x99; 16])
    );
    let n = r.air.lock().unwrap().keys.len();
    let replay = {
        let mut a = r.air.lock().unwrap();
        a.rekey([0x99; 16]);
        a.events.back().cloned().unwrap()
    };
    r.air.lock().unwrap().events.push_back(replay);
    r.run(200);
    assert_eq!(
        r.air.lock().unwrap().keys.len(),
        n,
        "same key is not installed again"
    );
}

#[test]
fn wrong_password_unsupported_and_refusals() {
    let mut r = Rig::new("wrong");
    assert_eq!(
        r.join("home", Some("not the password"), true),
        JoinOutcome::WrongPassword
    );
    assert!(r.core.saved.is_empty());
    assert!(r.air.lock().unwrap().keys.is_empty());
    assert_eq!(r.join("home", None, false), JoinOutcome::PasswordNeeded);
    assert_eq!(
        r.join("home", Some("short"), false),
        JoinOutcome::BadPassword
    );
    assert_eq!(
        r.join("wpa3", Some("whatever-pass"), false),
        JoinOutcome::Unsupported
    );
    assert_eq!(r.join("nowhere", None, false), JoinOutcome::NotFound);
    assert_eq!(r.join("", None, false), JoinOutcome::NotFound);
    r.core.command(Command::Radio(false), r.now);
    assert_eq!(r.core.status().phase, WifiPhase::Off);
    assert_eq!(r.join("cafe", None, false), JoinOutcome::RadioOff);
    r.core.command(Command::Radio(true), r.now);
    // Two joins at once: the second is busy.
    let (tx, rx) = channel();
    r.core.command(
        Command::Join {
            ssid: b"cafe".to_vec(),
            secret: None,
            remember: false,
            reply: Box::new(move |o| tx.send(o).unwrap()),
        },
        r.now,
    );
    assert_eq!(
        r.join("home", Some("correct horse battery"), false),
        JoinOutcome::Busy
    );
    assert_eq!(r.wait(&rx), JoinOutcome::Connected);
}

#[test]
fn a_saved_network_comes_back_by_itself_until_disconnect() {
    let mut r = Rig::new("auto");
    assert_eq!(
        r.join("home", Some("correct horse battery"), true),
        JoinOutcome::Connected
    );
    r.run(500);
    let connects = r.air.lock().unwrap().connects;
    // The access point drops the link: reconnection with the saved key.
    r.air
        .lock()
        .unwrap()
        .events
        .push_back(WifiEvent::Disconnected {
            ifindex: 3,
            reason: 4,
        });
    r.run(100);
    assert_ne!(r.core.status().phase, WifiPhase::Connected);
    assert!(
        r.net.lock().unwrap().configured.is_none(),
        "address taken away"
    );
    r.run(3_000);
    assert_eq!(r.air.lock().unwrap().connects, connects + 1);
    assert_eq!(r.core.status().phase, WifiPhase::Connected);
    // A restart of the service: the saved network is joined at start.
    let dir = r.dir.clone();
    let air = r.air.clone();
    let mut again = Rig::open(dir.clone(), air);
    again.run(3_000);
    assert_eq!(again.core.status().ssid.as_deref(), Some("home"));
    // Disconnect: no automatic reconnection.
    let (tx, rx) = channel();
    again.core.command(
        Command::Disconnect(Box::new(move |o| tx.send(o).unwrap())),
        again.now,
    );
    assert_eq!(rx.recv().unwrap(), NetOutcome::Done);
    let c = again.air.lock().unwrap().connects;
    again.run(30_000);
    assert_eq!(again.air.lock().unwrap().connects, c);
    assert_eq!(again.core.status().phase, WifiPhase::Disconnected);
    let (tx, rx) = channel();
    again.core.command(
        Command::Known(Box::new(move |l| tx.send(l).unwrap())),
        again.now,
    );
    assert_eq!(rx.recv().unwrap(), vec!["home".to_string()]);
    let (tx, rx) = channel();
    again.core.command(
        Command::Forget(b"home".to_vec(), Box::new(move |o| tx.send(o).unwrap())),
        again.now,
    );
    assert_eq!(rx.recv().unwrap(), NetOutcome::Done);
    assert!(Saved::load(dir.join("wifi")).is_empty());
    std::mem::forget(r);
}

/// Review security S-2: an open access point named like a secured network ("evil twin",
/// stronger signal) is never joined, neither with the password nor automatically later.
#[test]
fn an_open_twin_of_a_secured_network_is_never_joined() {
    let mut r = Rig::new("twin");
    r.air
        .lock()
        .unwrap()
        .aps
        .push(ap(9, b"home", -1000, None, None));
    assert_eq!(
        r.join("home", Some("correct horse battery"), true),
        JoinOutcome::Connected
    );
    let secured = Some([2, 0, 0, 0, 0, 1]);
    assert_eq!(
        r.core.link.as_ref().map(|l| l.bssid),
        secured,
        "not the twin"
    );
    // A restart of the service: the saved network comes back through its secured access point.
    let (dir, air) = (r.dir.clone(), r.air.clone());
    std::mem::forget(r);
    let mut again = Rig::open(dir, air.clone());
    again.run(3_000);
    assert_eq!(again.core.link.as_ref().map(|l| l.bssid), secured);
    // Only the open twin left: nothing is joined.
    air.lock().unwrap().aps.retain(|a| a.bss.bssid[5] != 1);
    let dir = again.dir.clone();
    std::mem::forget(again);
    let mut last = Rig::open(dir, air);
    last.run(3_000);
    assert!(last.core.link.is_none(), "the open twin is not joined");
}
