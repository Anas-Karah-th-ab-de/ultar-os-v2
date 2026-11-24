//! Test stand-ins of the Wi-Fi core: an access point (an authenticator
//! built from the IEEE 802.11 rules: its frames are checked against the
//! supplicant, whose own answers are checked against an independent
//! reference in `ultar-netproto`) and a DHCP server.
#![cfg(test)]

use super::*;
use crate::wifi::driver::{IpLayer, WifiDriver};
use aes_kw::KekAes128;
use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex};
use ultar_hal_netlink::nl80211::{ConnectParams, WifiEvent, WifiInterface};
use ultar_hal_netlink::NetError;
use ultar_netproto::dhcp::msg::{opt, Message, MessageType};
use ultar_netproto::dhcp::Lease;
use ultar_netproto::wpa::eapol::{info, KeyFrame};
use ultar_netproto::wpa::{Pmk, Ptk};

pub const STA: [u8; 6] = [2, 0, 0, 0, 0, 0x10];
pub const ANONCE: [u8; 32] = [0x42; 32];
pub const GTK: [u8; 16] = [0x77; 16];

pub fn rsn_psk() -> Vec<u8> {
    vec![
        48, 20, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 2, 0x0c, 0,
    ]
}

pub fn ies(ssid: &[u8], rsn: Option<Vec<u8>>) -> Vec<u8> {
    let mut v = vec![0, ssid.len() as u8];
    v.extend_from_slice(ssid);
    if let Some(r) = rsn {
        v.extend(r);
    }
    v
}

pub struct Ap {
    pub bss: Bss,
    pub pass: Option<&'static str>,
    pub ptk: Option<Ptk>,
    pub replay: u64,
}

#[derive(Default)]
pub struct Air {
    pub aps: Vec<Ap>,
    pub events: VecDeque<WifiEvent>,
    pub keys: Vec<(Option<[u8; 6]>, u8, Vec<u8>)>,
    pub authorized: Vec<[u8; 6]>,
    pub connects: usize,
    pub disconnects: usize,
    pub done: bool,
}

pub struct FakeAir(pub Arc<Mutex<Air>>);

pub fn frame(info_bits: u16, replay: u64, nonce: [u8; 32], data: Vec<u8>) -> KeyFrame {
    KeyFrame {
        version: 2,
        info: info_bits | info::VERSION_2,
        key_len: 16,
        replay,
        nonce,
        rsc: [3, 0, 0, 0, 0, 0, 0, 0],
        mic: [0; 16],
        data,
    }
}

pub fn wrap(kek: &[u8], mut plain: Vec<u8>) -> Vec<u8> {
    plain.push(0xdd);
    while !plain.len().is_multiple_of(8) || plain.len() < 16 {
        plain.push(0);
    }
    let kek: [u8; 16] = kek.try_into().unwrap();
    let mut out = vec![0u8; plain.len() + 8];
    KekAes128::from(kek).wrap(&plain, &mut out).unwrap();
    out
}

pub fn gtk_kde(index: u8, key: &[u8]) -> Vec<u8> {
    let mut k = vec![0xdd, 6 + key.len() as u8, 0x00, 0x0f, 0xac, 0x01, index, 0];
    k.extend_from_slice(key);
    k
}

impl Air {
    pub fn ap_answer(&mut self, to: [u8; 6], bytes: &[u8]) {
        let f = KeyFrame::parse(bytes).unwrap();
        let ap = self.aps.iter_mut().find(|a| a.bss.bssid == to).unwrap();
        let pass = ap.pass.unwrap();
        let ssid = ultar_netproto::ie::ssid(&ap.bss.ies).unwrap();
        let pmk = Pmk::from_passphrase(pass, &ssid).unwrap();
        if !f.has(info::SECURE) {
            let ptk = Ptk::derive(&pmk, &to, &STA, &ANONCE, &f.nonce);
            if !f.mic_ok(ptk.kck()) {
                // Wrong password: the access point gives up.
                self.events.push_back(WifiEvent::Disconnected {
                    ifindex: 3,
                    reason: 15,
                });
                return;
            }
            ap.replay += 1;
            let mut data = ultar_netproto::ie::find_raw(&ap.bss.ies, 48).unwrap();
            data.extend(gtk_kde(1, &GTK));
            let m3 = frame(
                info::ACK
                    | info::MIC
                    | info::PAIRWISE
                    | info::INSTALL
                    | info::SECURE
                    | info::ENCRYPTED,
                ap.replay,
                ANONCE,
                wrap(ptk.kek(), data),
            )
            .sign(ptk.kck());
            ap.ptk = Some(ptk);
            self.events.push_back(WifiEvent::Eapol {
                ifindex: 3,
                from: to,
                frame: m3,
            });
        } else if f.has(info::PAIRWISE) {
            assert!(f.mic_ok(ap.ptk.as_ref().unwrap().kck()));
            self.done = true;
        }
    }

    pub fn rekey(&mut self, key: [u8; 16]) {
        let ap = &mut self.aps[0];
        let ptk = ap.ptk.as_ref().unwrap();
        ap.replay += 1;
        let g1 = frame(
            info::ACK | info::MIC | info::SECURE | info::ENCRYPTED,
            ap.replay,
            [0; 32],
            wrap(ptk.kek(), gtk_kde(2, &key)),
        )
        .sign(ptk.kck());
        let from = ap.bss.bssid;
        self.events.push_back(WifiEvent::Eapol {
            ifindex: 3,
            from,
            frame: g1,
        });
    }
}

impl WifiDriver for FakeAir {
    fn interfaces(&mut self) -> Result<Vec<WifiInterface>, NetError> {
        Ok(vec![WifiInterface {
            index: 3,
            name: "wlan0".into(),
            mac: STA,
        }])
    }
    fn trigger_scan(&mut self, i: u32) -> Result<(), NetError> {
        self.0
            .lock()
            .unwrap()
            .events
            .push_back(WifiEvent::ScanDone(i));
        Ok(())
    }
    fn scan_results(&mut self, _: u32) -> Result<Vec<Bss>, NetError> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .aps
            .iter()
            .map(|a| a.bss.clone())
            .collect())
    }
    fn connect(&mut self, _: u32, p: &ConnectParams) -> Result<(), NetError> {
        let mut air = self.0.lock().unwrap();
        air.connects += 1;
        let ap = air.aps.iter().find(|a| a.bss.bssid == p.bssid).unwrap();
        assert_eq!(p.rsn.is_some(), ap.pass.is_some());
        let secured = ap.pass.is_some();
        let bssid = p.bssid;
        air.events.push_back(WifiEvent::Connected {
            ifindex: 3,
            bssid: Some(bssid),
            status: 0,
        });
        if secured {
            let ap = air.aps.iter_mut().find(|a| a.bss.bssid == bssid).unwrap();
            ap.replay += 1;
            let m1 = frame(info::ACK | info::PAIRWISE, ap.replay, ANONCE, vec![]).to_bytes();
            air.events.push_back(WifiEvent::Eapol {
                ifindex: 3,
                from: bssid,
                frame: m1,
            });
        }
        Ok(())
    }
    fn disconnect(&mut self, _: u32) -> Result<(), NetError> {
        self.0.lock().unwrap().disconnects += 1;
        Ok(())
    }
    fn send_eapol(&mut self, _: u32, to: [u8; 6], frame: &[u8], _: bool) -> Result<(), NetError> {
        self.0.lock().unwrap().ap_answer(to, frame);
        Ok(())
    }
    fn install_key(
        &mut self,
        _: u32,
        peer: Option<[u8; 6]>,
        index: u8,
        key: &[u8],
        _: &[u8; 8],
    ) -> Result<(), NetError> {
        self.0
            .lock()
            .unwrap()
            .keys
            .push((peer, index, key.to_vec()));
        Ok(())
    }
    fn authorize(&mut self, _: u32, peer: [u8; 6]) -> Result<(), NetError> {
        self.0.lock().unwrap().authorized.push(peer);
        Ok(())
    }
    fn signal(&mut self, _: u32, _: [u8; 6]) -> Result<Option<i8>, NetError> {
        Ok(Some(-45))
    }
    fn next_event(&mut self) -> Result<Option<WifiEvent>, NetError> {
        Ok(self.0.lock().unwrap().events.pop_front())
    }
    fn fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        None
    }
}

#[derive(Default)]
pub struct Net {
    pub queue: VecDeque<Vec<u8>>,
    pub configured: Option<Lease>,
    pub open: bool,
}

pub struct FakeIp(pub Arc<Mutex<Net>>);

impl IpLayer for FakeIp {
    fn open(&mut self, name: &str) -> Result<(), NetError> {
        assert_eq!(name, "wlan0");
        self.0.lock().unwrap().open = true;
        Ok(())
    }
    fn close(&mut self) {
        self.0.lock().unwrap().open = false;
    }
    fn send(&mut self, _: Ipv4Addr, bytes: &[u8]) -> Result<(), NetError> {
        let r = Message::parse(bytes).unwrap();
        let kind = match r.kind() {
            Some(MessageType::Discover) => MessageType::Offer,
            Some(MessageType::Request) => MessageType::Ack,
            _ => return Ok(()),
        };
        let reply = Message {
            op: 2,
            xid: r.xid,
            flags: r.flags,
            ciaddr: r.ciaddr,
            yiaddr: Ipv4Addr::new(192, 168, 7, 50),
            chaddr: r.chaddr,
            options: vec![
                (opt::TYPE, vec![kind as u8]),
                (opt::SERVER, vec![192, 168, 7, 1]),
                (opt::MASK, vec![255, 255, 255, 0]),
                (opt::ROUTER, vec![192, 168, 7, 1]),
                (opt::DNS, vec![192, 168, 7, 1]),
                (opt::LEASE, 3600u32.to_be_bytes().to_vec()),
            ],
        };
        self.0.lock().unwrap().queue.push_back(reply.to_bytes());
        Ok(())
    }
    fn recv(&mut self, buf: &mut [u8]) -> Result<Option<usize>, NetError> {
        Ok(self.0.lock().unwrap().queue.pop_front().map(|m| {
            buf[..m.len()].copy_from_slice(&m);
            m.len()
        }))
    }
    fn configure(&mut self, _: &str, lease: &Lease) -> Result<(), NetError> {
        self.0.lock().unwrap().configured = Some(lease.clone());
        Ok(())
    }
    fn deconfigure(&mut self, _: &str) -> Result<(), NetError> {
        self.0.lock().unwrap().configured = None;
        Ok(())
    }
    fn fd(&self) -> Option<std::os::fd::BorrowedFd<'_>> {
        None
    }
}

pub struct Rig {
    pub core: WifiCore,
    pub air: Arc<Mutex<Air>>,
    pub net: Arc<Mutex<Net>>,
    pub dir: std::path::PathBuf,
    pub now: u64,
}

pub fn ap(n: u8, ssid: &[u8], mbm: i32, pass: Option<&'static str>, rsn: Option<Vec<u8>>) -> Ap {
    Ap {
        bss: Bss {
            bssid: [2, 0, 0, 0, 0, n],
            freq_mhz: 2412,
            signal_mbm: mbm,
            capability: if rsn.is_some() { 0x11 } else { 0x01 },
            ies: ies(ssid, rsn),
        },
        pass,
        ptk: None,
        replay: 0,
    }
}

impl Rig {
    pub fn new(tag: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("netd-core-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Rig::open(dir, Arc::default())
    }

    pub fn open(dir: std::path::PathBuf, air: Arc<Mutex<Air>>) -> Rig {
        if air.lock().unwrap().aps.is_empty() {
            let mut a = air.lock().unwrap();
            a.aps.push(ap(
                1,
                b"home",
                -4000,
                Some("correct horse battery"),
                Some(rsn_psk()),
            ));
            a.aps.push(ap(2, b"cafe", -6000, None, None));
            let sae = vec![
                48, 20, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 4, 1, 0, 0, 0x0f, 0xac, 8,
                0xc0, 0,
            ];
            a.aps
                .push(ap(3, b"wpa3", -5000, Some("whatever-pass"), Some(sae)));
        }
        let net = Arc::new(Mutex::new(Net::default()));
        let mut n = 0u8;
        let core = WifiCore::new(
            Some(Box::new(FakeAir(air.clone()))),
            Box::new(FakeIp(net.clone())),
            Saved::load(dir.join("wifi")),
            Box::new(move |b| {
                n = n.wrapping_add(1);
                b.fill(n);
                true
            }),
        );
        Rig {
            core,
            air,
            net,
            dir,
            now: 1_000,
        }
    }

    /// Lets events and timers run for `ms`.
    pub fn run(&mut self, ms: u64) {
        let end = self.now + ms;
        loop {
            while let Some(e) = self.core.driver.as_mut().unwrap().next_event().unwrap() {
                self.core.event(e, self.now);
            }
            self.core.tick(self.now);
            if self.now >= end {
                break;
            }
            self.now += 100;
        }
    }

    pub fn join(&mut self, ssid: &str, pass: Option<&str>, remember: bool) -> JoinOutcome {
        let (tx, rx) = channel();
        self.core.command(
            Command::Join {
                ssid: ssid.as_bytes().to_vec(),
                secret: pass.map(|p| Zeroizing::new(p.to_owned())),
                remember,
                reply: Box::new(move |o| tx.send(o).unwrap()),
            },
            self.now,
        );
        self.wait(&rx)
    }

    pub fn wait<T>(&mut self, rx: &Receiver<T>) -> T {
        for _ in 0..400 {
            if let Ok(v) = rx.try_recv() {
                return v;
            }
            self.run(100);
        }
        panic!("no answer");
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
