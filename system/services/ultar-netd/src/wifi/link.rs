//! The link to an access point after the join: group rekeying, DHCP,
//! signal, and its end.

use super::core::WifiCore;
use super::join::Join;
use std::net::Ipv4Addr;
use ultar_bus::messages::net::{JoinOutcome, WifiLink, WifiPhase};
use ultar_hal_netlink::nl80211::Bss;
use ultar_netproto::dhcp::{Action, Client, Lease};
use ultar_netproto::wpa::{Pmk, Step, Supplicant};

/// How often the signal is read while connected.
pub const SIGNAL_MS: u64 = 10_000;

/// The link to an access point.
pub(super) struct Link {
    pub ssid: Vec<u8>,
    pub bssid: [u8; 6],
    pub sup: Option<Box<Supplicant>>,
    pub dhcp: Client,
    pub lease: Option<Lease>,
    pub signal: Option<u8>,
    pub signal_at: u64,
}

impl Link {
    pub fn phase(&self) -> WifiPhase {
        if self.lease.is_some() {
            WifiPhase::Connected
        } else {
            WifiPhase::ObtainingAddress
        }
    }

    pub fn deadline(&self) -> u64 {
        self.dhcp.next_deadline().min(self.signal_at + SIGNAL_MS)
    }

    pub fn describe(&self, s: &mut WifiLink) {
        s.ssid = Some(String::from_utf8_lossy(&self.ssid).into_owned());
        s.signal_percent = self.signal;
        if let Some(l) = &self.lease {
            s.address = Some(format!("{}/{}", l.address, l.prefix));
            s.router = l.router.map(|r| r.to_string());
            s.dns = l.dns.iter().map(|d| d.to_string()).collect();
        }
    }
}

impl WifiCore {
    pub(super) fn rekey(&mut self, from: [u8; 6], frame: &[u8]) {
        let (Some(d), Some(i), Some(l)) = (
            self.driver.as_mut(),
            self.iface.as_ref(),
            self.link.as_mut(),
        ) else {
            return;
        };
        let Some(sup) = l.sup.as_mut().filter(|_| l.bssid == from) else {
            return;
        };
        match sup.receive(frame) {
            Ok(Step::Rekey { reply, group }) => {
                if d.send_eapol(i.index, from, &reply, true).is_ok() {
                    let _ = d.install_key(i.index, None, group.index, &group.key, &group.rsc);
                }
            }
            Ok(Step::Send(reply)) => {
                let _ = d.send_eapol(i.index, from, &reply, true);
            }
            _ => {}
        }
    }

    pub(super) fn link_up(
        &mut self,
        j: Join,
        bss: Bss,
        sup: Option<Box<Supplicant>>,
        pmk: Option<Pmk>,
        now: u64,
    ) {
        let Some(iface) = self.iface.clone() else {
            return;
        };
        if j.remember || self.saved.get(&j.ssid).is_some() {
            let _ = self.saved.remember(&j.ssid, pmk);
        }
        let _ = self.ip.open(&iface.name);
        let mut xid = [0u8; 4];
        // A transaction id needs no secrecy: a failed source keeps zeros.
        let _ = (self.random)(&mut xid);
        self.link = Some(Link {
            ssid: j.ssid.clone(),
            bssid: bss.bssid,
            sup,
            dhcp: Client::new(iface.mac, u32::from_ne_bytes(xid), now),
            lease: None,
            signal: Some(WifiCore::percent(bss.signal_mbm)),
            signal_at: now,
        });
        self.reset_backoff();
        j.finish(JoinOutcome::Connected);
        self.dhcp(now);
    }

    pub(super) fn drop_link(&mut self) {
        if let Some(l) = self.link.take() {
            drop(l);
            self.hang_up();
            if let Some(i) = &self.iface {
                let _ = self.ip.deconfigure(&i.name);
            }
            self.ip.close();
        }
    }

    /// DHCP and the signal of the link.
    pub(super) fn dhcp(&mut self, now: u64) {
        let (Some(link), Some(iface)) = (self.link.as_mut(), self.iface.as_ref()) else {
            return;
        };
        let mut actions = Vec::new();
        let mut buf = [0u8; 1500];
        for _ in 0..16 {
            match self.ip.recv(&mut buf) {
                Ok(Some(n)) => actions.extend(link.dhcp.receive(&buf[..n], now)),
                _ => break,
            }
        }
        actions.extend(link.dhcp.poll(now));
        for a in actions {
            match a {
                Action::Broadcast(b) => {
                    let _ = self.ip.send(Ipv4Addr::BROADCAST, &b);
                }
                Action::Unicast(to, b) => {
                    let _ = self.ip.send(to, &b);
                }
                Action::Configure(lease) => {
                    if self.ip.configure(&iface.name, &lease).is_ok() {
                        link.lease = Some(lease);
                    }
                }
                Action::Deconfigure => {
                    let _ = self.ip.deconfigure(&iface.name);
                    link.lease = None;
                }
            }
        }
        if now >= link.signal_at + SIGNAL_MS {
            link.signal_at = now;
            if let Some(d) = self.driver.as_mut() {
                if let Ok(Some(dbm)) = d.signal(iface.index, link.bssid) {
                    link.signal = Some(WifiCore::percent(i32::from(dbm) * 100));
                }
            }
        }
    }
}
