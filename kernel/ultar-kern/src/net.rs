//! Kleiner Netzwerkstapel im Kern: Ethernet, ARP, IPv4, ICMP (Ping), UDP, DHCP-Client.

use crate::driver::NET;
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Clone, Copy, Default)]
pub struct Config {
    pub ip: [u8; 4],
    pub mask: [u8; 4],
    pub gw: [u8; 4],
    pub dns: [u8; 4],
}

pub static CONFIG: Mutex<Option<Config>> = Mutex::new(None);

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
    let mut h = vec![0x45, 0, (len >> 8) as u8, len as u8, 0, 1, 0x40, 0, 64, proto, 0, 0];
    h.extend_from_slice(&src);
    h.extend_from_slice(&dst);
    let c = csum(&h);
    h[10] = (c >> 8) as u8;
    h[11] = c as u8;
    h.extend_from_slice(payload);
    h
}

fn udp(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let len = 8 + payload.len();
    let mut u = Vec::with_capacity(len);
    u.extend_from_slice(&sport.to_be_bytes());
    u.extend_from_slice(&dport.to_be_bytes());
    u.extend_from_slice(&(len as u16).to_be_bytes());
    u.extend_from_slice(&[0, 0]); // Prüfsumme optional bei IPv4
    u.extend_from_slice(payload);
    u
}

fn dhcp_msg(mac: [u8; 6], xid: u32, typ: u8, req_ip: Option<[u8; 4]>, server: Option<[u8; 4]>) -> Vec<u8> {
    let mut m = vec![0u8; 240];
    m[0] = 1; // Anfrage
    m[1] = 1; // Ethernet
    m[2] = 6;
    m[4..8].copy_from_slice(&xid.to_be_bytes());
    m[10] = 0x80; // Antwort als Rundsendung
    m[28..34].copy_from_slice(&mac);
    m[236..240].copy_from_slice(&[99, 130, 83, 99]);
    m.extend_from_slice(&[53, 1, typ]);
    if let Some(ip) = req_ip {
        m.extend_from_slice(&[50, 4]);
        m.extend_from_slice(&ip);
    }
    if let Some(s) = server {
        m.extend_from_slice(&[54, 4]);
        m.extend_from_slice(&s);
    }
    m.extend_from_slice(&[12, 5, b'u', b'l', b't', b'a', b'r']); // Rechnername
    m.extend_from_slice(&[55, 3, 1, 3, 6, 255]);
    m
}

/// DHCP-Antwort auswerten: (Typ, angebotene IP, Optionen)
fn parse_dhcp(frame: &[u8], xid: u32) -> Option<(u8, [u8; 4], Config, [u8; 4])> {
    if frame.len() < 14 + 20 + 8 + 240 || u16::from_be_bytes([frame[12], frame[13]]) != 0x0800 {
        return None;
    }
    let ip = &frame[14..];
    let ihl = (ip[0] & 0xf) as usize * 4;
    if ip[9] != 17 {
        return None;
    }
    let u = &ip[ihl..];
    if u16::from_be_bytes([u[2], u[3]]) != 68 {
        return None;
    }
    let d = &u[8..];
    if d.len() < 240 || d[0] != 2 || u32::from_be_bytes([d[4], d[5], d[6], d[7]]) != xid {
        return None;
    }
    let yi = [d[16], d[17], d[18], d[19]];
    let mut cfg = Config { ip: yi, ..Default::default() };
    let (mut typ, mut server) = (0, [0u8; 4]);
    let mut p = 240;
    while p + 1 < d.len() && d[p] != 255 {
        let (o, l) = (d[p], d[p + 1] as usize);
        let v = d.get(p + 2..p + 2 + l)?;
        match o {
            53 => typ = v[0],
            1 if l >= 4 => cfg.mask.copy_from_slice(&v[..4]),
            3 if l >= 4 => cfg.gw.copy_from_slice(&v[..4]),
            6 if l >= 4 => cfg.dns.copy_from_slice(&v[..4]),
            54 if l >= 4 => server.copy_from_slice(&v[..4]),
            _ => {}
        }
        p += 2 + l;
    }
    Some((typ, yi, cfg, server))
}

fn bcast_dhcp(n: usize, mac: [u8; 6], msg: &[u8]) -> bool {
    let f = eth([0xff; 6], mac, 0x0800, &ipv4([0; 4], [255; 4], 17, &udp(68, 67, msg)));
    NET.lock().get_mut(n).map(|d| d.send(&f).is_ok()).unwrap_or(false)
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

/// Adresse per DHCP holen (Discover → Offer → Request → Ack)
pub fn dhcp(n: usize) -> Option<Config> {
    let mac = NET.lock().get(n)?.mac();
    let xid = 0x554c_0000 | (crate::arch::rdtsc() as u32 & 0xffff);
    for _ in 0..3 {
        bcast_dhcp(n, mac, &dhcp_msg(mac, xid, 1, None, None));
        let mut offer = None;
        wait_frame(n, 1500, |f| {
            if let Some((2, ip, cfg, srv)) = parse_dhcp(f, xid) {
                offer = Some((ip, cfg, srv));
                true
            } else {
                false
            }
        });
        let Some((ip, _cfg, srv)) = offer else { continue };
        bcast_dhcp(n, mac, &dhcp_msg(mac, xid, 3, Some(ip), Some(srv)));
        let mut ack = None;
        wait_frame(n, 1500, |f| {
            if let Some((5, _, cfg, _)) = parse_dhcp(f, xid) {
                ack = Some(cfg);
                true
            } else {
                false
            }
        });
        if let Some(c) = ack {
            *CONFIG.lock() = Some(c);
            return Some(c);
        }
    }
    None
}

fn arp_resolve(n: usize, mac: [u8; 6], my: [u8; 4], target: [u8; 4]) -> Option<[u8; 6]> {
    let mut a = vec![0, 1, 8, 0, 6, 4, 0, 1];
    a.extend_from_slice(&mac);
    a.extend_from_slice(&my);
    a.extend_from_slice(&[0; 6]);
    a.extend_from_slice(&target);
    NET.lock().get_mut(n)?.send(&eth([0xff; 6], mac, 0x0806, &a)).ok()?;
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
    icmp.extend_from_slice(b"Ultar-Kern KM Code Craft");
    let c = csum(&icmp);
    icmp[2] = (c >> 8) as u8;
    icmp[3] = c as u8;
    let t0 = crate::arch::uptime_ms();
    NET.lock().get_mut(n)?.send(&eth(hmac, mac, 0x0800, &ipv4(cfg.ip, dst, 1, &icmp))).ok()?;
    let ok = wait_frame(n, 2000, |f| f.len() >= 42 && f[12..14] == [8, 0] && f[23] == 1 && f[26..30] == dst && f[34] == 0);
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
