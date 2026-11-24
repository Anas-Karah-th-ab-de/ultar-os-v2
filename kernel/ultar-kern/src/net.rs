//! Kleiner Netzwerkstapel im Kern: Ethernet, ARP, IPv4, ICMP (Ping), UDP; der DHCP-Client mit Lease,
//! Erneuerung und festen Adressen steht in `dhcp.rs`.

use crate::driver::NET;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

#[derive(Clone, Copy, Default)]
pub struct Config {
    pub ip: [u8; 4],
    pub mask: [u8; 4],
    pub gw: [u8; 4],
    pub dns: [u8; 4],
    /// second name server (`0.0.0.0`: none)
    pub dns2: [u8; 4],
}

pub static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

pub use crate::dhcp::{
    dhcp, dhcp_all, input, ipv4_text, ipv4_write, link_changes, set_static, tick,
};
static ACTIVE_NIC: AtomicUsize = AtomicUsize::new(0);

pub fn active_nic() -> usize {
    ACTIVE_NIC.load(Ordering::Relaxed)
}

pub fn set_active_nic(n: usize) {
    ACTIVE_NIC.store(n, Ordering::Relaxed);
}

pub fn active_mac() -> Option<[u8; 6]> {
    NET.lock().get(active_nic()).map(|d| d.mac())
}

pub fn active_send(frame: &[u8]) -> Result<(), String> {
    NET.lock()
        .get_mut(active_nic())
        .ok_or_else(|| String::from("no active network interface"))?
        .send(frame)
}

pub fn csum(data: &[u8]) -> u16 {
    let mut s: u32 = 0;
    for c in data.chunks(2) {
        s += u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32;
    }
    while s >> 16 != 0 {
        s = (s & 0xffff) + (s >> 16);
    }
    !(s as u16)
}

pub fn eth(dst: [u8; 6], src: [u8; 6], typ: u16, payload: &[u8]) -> Vec<u8> {
    let mut f = Vec::with_capacity(14 + payload.len());
    f.extend_from_slice(&dst);
    f.extend_from_slice(&src);
    f.extend_from_slice(&typ.to_be_bytes());
    f.extend_from_slice(payload);
    while f.len() < 60 {
        f.push(0);
    }
    f
}

pub fn ipv4(src: [u8; 4], dst: [u8; 4], proto: u8, payload: &[u8]) -> Vec<u8> {
    let len = 20 + payload.len();
    let mut h = vec![
        0x45,
        0,
        (len >> 8) as u8,
        len as u8,
        0,
        1,
        0x40,
        0,
        64,
        proto,
        0,
        0,
    ];
    h.extend_from_slice(&src);
    h.extend_from_slice(&dst);
    let c = csum(&h);
    h[10] = (c >> 8) as u8;
    h[11] = c as u8;
    h.extend_from_slice(payload);
    h
}

pub(crate) fn udp(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let len = 8 + payload.len();
    let mut u = Vec::with_capacity(len);
    u.extend_from_slice(&sport.to_be_bytes());
    u.extend_from_slice(&dport.to_be_bytes());
    u.extend_from_slice(&(len as u16).to_be_bytes());
    u.extend_from_slice(&[0, 0]); // Prüfsumme optional bei IPv4
    u.extend_from_slice(payload);
    u
}

fn wait_frame(n: usize, ms: u64, mut f: impl FnMut(&[u8]) -> bool) -> bool {
    let end = crate::arch::uptime_ms() + ms;
    let mut buf = [0u8; 2048];
    while crate::arch::uptime_ms() < end {
        let got = NET.lock().get_mut(n).and_then(|d| d.recv(&mut buf));
        if let Some(len) = got {
            if f(&buf[..len]) {
                return true;
            }
        } else {
            crate::arch::pause();
        }
    }
    false
}

fn arp_resolve(n: usize, mac: [u8; 6], my: [u8; 4], target: [u8; 4]) -> Option<[u8; 6]> {
    let mut a = vec![0, 1, 8, 0, 6, 4, 0, 1];
    a.extend_from_slice(&mac);
    a.extend_from_slice(&my);
    a.extend_from_slice(&[0; 6]);
    a.extend_from_slice(&target);
    NET.lock()
        .get_mut(n)?
        .send(&eth([0xff; 6], mac, 0x0806, &a))
        .ok()?;
    let mut res = None;
    wait_frame(n, 1000, |f| {
        if f.len() >= 42 && f[12..14] == [8, 6] && f[20..22] == [0, 2] && f[28..32] == target {
            let mut m = [0u8; 6];
            m.copy_from_slice(&f[22..28]);
            res = Some(m);
            true
        } else {
            false
        }
    });
    res
}

/// ICMP-Echo; Laufzeit in ms
pub fn ping(n: usize, dst: [u8; 4]) -> Option<u64> {
    let cfg = (*CONFIG.lock())?;
    let mac = NET.lock().get(n)?.mac();
    let hop = if same_net(cfg, dst) { dst } else { cfg.gw };
    let hmac = arp_resolve(n, mac, cfg.ip, hop)?;
    let mut icmp = vec![8, 0, 0, 0, 0x55, 0x4c, 0, 1];
    icmp.extend_from_slice(concat!("Ultar-Kern ", ultar_brand::vendor!()).as_bytes());
    let c = csum(&icmp);
    icmp[2] = (c >> 8) as u8;
    icmp[3] = c as u8;
    let t0 = crate::arch::uptime_ms();
    NET.lock()
        .get_mut(n)?
        .send(&eth(hmac, mac, 0x0800, &ipv4(cfg.ip, dst, 1, &icmp)))
        .ok()?;
    let ok = wait_frame(n, 2000, |f| {
        f.len() >= 42 && f[12..14] == [8, 0] && f[23] == 1 && f[26..30] == dst && f[34] == 0
    });
    ok.then(|| crate::arch::uptime_ms() - t0)
}

pub fn same_net(c: Config, a: [u8; 4]) -> bool {
    (0..4).all(|i| a[i] & c.mask[i] == c.ip[i] & c.mask[i])
}

pub fn parse_ip(s: &str) -> Option<[u8; 4]> {
    let mut o = [0u8; 4];
    let mut it = s.split('.');
    for b in o.iter_mut() {
        *b = it.next()?.parse().ok()?;
    }
    it.next().is_none().then_some(o)
}
