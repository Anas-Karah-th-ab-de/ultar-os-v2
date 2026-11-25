//! DHCP-Client des Kerns je Schnittstelle: Lease, Erneuerung (T1/T2), Ablauf, Neustart nach
//! Kabelwechsel, feste Adressen. Zustand unter `/sys/class/net/<if>/ultar_ipv4` (lesen; schreiben:
//! `dhcp`, `renew`, `static <a.b.c.d>/<nn> [router|-] [dns[,dns2]]`).

use crate::driver::NET;
use crate::net::{active_nic, eth, ipv4, parse_ip, set_active_nic, udp, Config, CONFIG};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};
use spin::Mutex;

fn dhcp_msg(mac: [u8; 6], xid: u32, typ: u8, ciaddr: [u8; 4], req_ip: Option<[u8; 4]>, server: Option<[u8; 4]>) -> Vec<u8> {
    let mut m = vec![0u8; 240];
    m[0] = 1; // Anfrage
    m[1] = 1; // Ethernet
    m[2] = 6;
    m[4..8].copy_from_slice(&xid.to_be_bytes());
    m[10] = 0x80; // Antwort als Rundsendung
    m[12..16].copy_from_slice(&ciaddr);
    m[28..34].copy_from_slice(&mac);
    m[236..240].copy_from_slice(&[99, 130, 83, 99]);
    m.extend_from_slice(&[53, 1, typ]);
    // client identifier: hardware type + MAC (the same lease after every restart)
    m.extend_from_slice(&[61, 7, 1]);
    m.extend_from_slice(&mac);
    if let Some(ip) = req_ip {
        m.extend_from_slice(&[50, 4]);
        m.extend_from_slice(&ip);
    }
    if let Some(s) = server {
        m.extend_from_slice(&[54, 4]);
        m.extend_from_slice(&s);
    }
    m.extend_from_slice(&[12, 5, b'u', b'l', b't', b'a', b'r']); // Rechnername
    m.extend_from_slice(&[55, 6, 1, 3, 6, 51, 58, 59]);
    m.push(255);
    m
}

/// A DHCP answer: type, offered address, configuration, server, lease times (seconds).
struct Reply {
    typ: u8,
    yiaddr: [u8; 4],
    cfg: Config,
    server: [u8; 4],
    lease: u32,
    t1: u32,
    t2: u32,
}

/// DHCP-Antwort auswerten (nur Antworten auf `xid`)
fn parse_dhcp(frame: &[u8], xid: u32) -> Option<Reply> {
    if frame.len() < 14 + 20 + 8 + 240 || u16::from_be_bytes([frame[12], frame[13]]) != 0x0800 {
        return None;
    }
    let ip = &frame[14..];
    let ihl = (ip[0] & 0xf) as usize * 4;
    if ip[9] != 17 || ip.len() < ihl + 8 {
        return None;
    }
    let u = &ip[ihl..];
    if u16::from_be_bytes([u[2], u[3]]) != 68 {
        return None;
    }
    let d = &u[8..];
    if d.len() < 240 || d[0] != 2 || u32::from_be_bytes([d[4], d[5], d[6], d[7]]) != xid || d[236..240] != [99, 130, 83, 99] {
        return None;
    }
    let yi = [d[16], d[17], d[18], d[19]];
    let mut r = Reply { typ: 0, yiaddr: yi, cfg: Config { ip: yi, ..Default::default() }, server: [d[20], d[21], d[22], d[23]], lease: 0, t1: 0, t2: 0 };
    let mut p = 240;
    while p < d.len() && d[p] != 255 {
        if d[p] == 0 {
            p += 1;
            continue;
        }
        if p + 1 >= d.len() {
            break;
        }
        let (o, l) = (d[p], d[p + 1] as usize);
        let v = d.get(p + 2..p + 2 + l)?;
        let be = |v: &[u8]| u32::from_be_bytes([v[0], v[1], v[2], v[3]]);
        match o {
            53 if l >= 1 => r.typ = v[0],
            1 if l >= 4 => r.cfg.mask.copy_from_slice(&v[..4]),
            3 if l >= 4 => r.cfg.gw.copy_from_slice(&v[..4]),
            6 if l >= 4 => {
                r.cfg.dns.copy_from_slice(&v[..4]);
                if l >= 8 {
                    r.cfg.dns2.copy_from_slice(&v[4..8]);
                }
            }
            54 if l >= 4 => r.server.copy_from_slice(&v[..4]),
            51 if l >= 4 => r.lease = be(v),
            58 if l >= 4 => r.t1 = be(v),
            59 if l >= 4 => r.t2 = be(v),
            _ => {}
        }
        p += 2 + l;
    }
    if r.cfg.mask == [0; 4] {
        // no mask option: the class default (RFC 2132 §3.3 leaves it to the client)
        r.cfg.mask = match yi[0] {
            0..=127 => [255, 0, 0, 0],
            128..=191 => [255, 255, 0, 0],
            _ => [255, 255, 255, 0],
        };
    }
    Some(r)
}

fn bcast_dhcp(n: usize, mac: [u8; 6], src: [u8; 4], msg: &[u8]) -> bool {
    let f = eth([0xff; 6], mac, 0x0800, &ipv4(src, [255; 4], 17, &udp(68, 67, msg)));
    NET.lock().get_mut(n).map(|d| d.send(&f).is_ok()).unwrap_or(false)
}

/// Where an interface stands with its IPv4 address.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Nothing started yet (the interface was just found).
    Idle,
    /// No cable / no carrier.
    NoLink,
    /// DHCP: DISCOVER sent, waiting for an offer.
    Selecting,
    /// DHCP: REQUEST for an offer sent.
    Requesting,
    /// DHCP: lease held.
    Bound,
    /// DHCP: T1 passed, renewing with the server.
    Renewing,
    /// DHCP: T2 passed, asking any server.
    Rebinding,
    /// Fixed address set by the network service.
    Static,
}

impl Phase {
    pub fn word(self) -> &'static str {
        match self {
            Phase::Idle => "idle",
            Phase::NoLink => "nolink",
            Phase::Selecting => "selecting",
            Phase::Requesting => "requesting",
            Phase::Bound => "bound",
            Phase::Renewing => "renewing",
            Phase::Rebinding => "rebinding",
            Phase::Static => "static",
        }
    }
}

#[derive(Clone, Copy)]
struct Lease {
    server: [u8; 4],
    secs: u32,
    t1: u64,
    t2: u64,
    end: u64,
}

/// IPv4 state of one interface (index as in `NET`).
#[derive(Clone, Copy)]
struct IfState {
    fixed: Option<Config>,
    phase: Phase,
    xid: u32,
    offer: Option<([u8; 4], [u8; 4])>,
    next_tx: u64,
    tries: u32,
    lease: Option<Lease>,
    cfg: Option<Config>,
    /// link as seen at the last look (`None`: not looked yet)
    link: Option<bool>,
    link_changes: u32,
}

impl IfState {
    const NEW: IfState = IfState { fixed: None, phase: Phase::Idle, xid: 0, offer: None, next_tx: 0, tries: 0, lease: None, cfg: None, link: None, link_changes: 0 };
}

static IFS: Mutex<Vec<IfState>> = Mutex::new(Vec::new());
static LAST_TICK: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
/// Bumped whenever an address, a lease or a link changes (readers can tell new from old).
pub static NET_GEN: AtomicUsize = AtomicUsize::new(0);

fn with_if<R>(n: usize, f: impl FnOnce(&mut IfState) -> R) -> R {
    crate::heap::kernel(|| {
        let mut v = IFS.lock();
        while v.len() <= n {
            v.push(IfState::NEW);
        }
        f(&mut v[n])
    })
}

fn new_xid() -> u32 {
    0x554c_0000 ^ (crate::arch::zufall() as u32)
}

/// Start DHCP on interface `n` from the beginning (DISCOVER at the next look).
pub fn start_dhcp(n: usize) {
    with_if(n, |s| {
        s.fixed = None;
        s.phase = Phase::Selecting;
        s.xid = new_xid();
        s.offer = None;
        s.tries = 0;
        s.next_tx = 0;
    });
    NET_GEN.fetch_add(1, Ordering::Relaxed);
}

/// The interface that carries the address of the sockets follows the one with an address.
fn settle_active() {
    let (active, cfgs): (usize, Vec<Option<Config>>) = (active_nic(), IFS.lock().iter().map(|s| s.cfg).collect());
    let pick = if cfgs.get(active).copied().flatten().is_some() { Some(active) } else { cfgs.iter().position(|c| c.is_some()) };
    match pick {
        Some(p) => {
            set_active_nic(p);
            *CONFIG.lock() = cfgs[p];
        }
        None => *CONFIG.lock() = None,
    }
}

/// Fixed address on interface `n` (no DHCP until `start_dhcp`).
pub fn set_static(n: usize, cfg: Config) {
    with_if(n, |s| {
        s.fixed = Some(cfg);
        s.phase = Phase::Static;
        s.lease = None;
        s.offer = None;
        s.cfg = Some(cfg);
    });
    if CONFIG.lock().is_none() || active_nic() == n {
        set_active_nic(n);
    }
    settle_active();
    NET_GEN.fetch_add(1, Ordering::Relaxed);
}

/// Ask the server for the lease again now (or start over without one).
pub fn renew(n: usize) {
    let bound = with_if(n, |s| {
        if s.fixed.is_some() {
            return true;
        }
        if s.lease.is_some() && s.phase == Phase::Bound {
            s.phase = Phase::Renewing;
            s.xid = new_xid();
            s.tries = 0;
            s.next_tx = 0;
            return true;
        }
        false
    });
    if !bound {
        start_dhcp(n);
    }
}

fn send_for(n: usize, s: &IfState) {
    let Some(mac) = NET.lock().get(n).map(|d| d.mac()) else { return };
    match s.phase {
        Phase::Selecting => {
            bcast_dhcp(n, mac, [0; 4], &dhcp_msg(mac, s.xid, 1, [0; 4], None, None));
        }
        Phase::Requesting => {
            if let Some((ip, srv)) = s.offer {
                bcast_dhcp(n, mac, [0; 4], &dhcp_msg(mac, s.xid, 3, [0; 4], Some(ip), Some(srv)));
            }
        }
        Phase::Renewing | Phase::Rebinding => {
            if let Some(c) = s.cfg {
                bcast_dhcp(n, mac, c.ip, &dhcp_msg(mac, s.xid, 3, c.ip, None, None));
            }
        }
        _ => {}
    }
}

/// Timers of interface `n`: retransmissions, T1/T2, the end of a lease.
fn dhcp_timers(n: usize, now: u64) {
    let (send, state) = with_if(n, |s| {
        if let (Some(l), Phase::Bound | Phase::Renewing | Phase::Rebinding) = (s.lease, s.phase) {
            if now >= l.end {
                // lease over: the address goes, DHCP starts again
                s.cfg = None;
                s.lease = None;
                s.phase = Phase::Selecting;
                s.xid = new_xid();
                s.tries = 0;
                s.next_tx = 0;
            } else if s.phase == Phase::Bound && now >= l.t1 {
                s.phase = Phase::Renewing;
                s.xid = new_xid();
                s.tries = 0;
                s.next_tx = 0;
            } else if s.phase == Phase::Renewing && now >= l.t2 {
                s.phase = Phase::Rebinding;
                s.tries = 0;
                s.next_tx = 0;
            }
        }
        if !matches!(s.phase, Phase::Selecting | Phase::Requesting | Phase::Renewing | Phase::Rebinding) || now < s.next_tx {
            return (false, *s);
        }
        if s.phase == Phase::Requesting && s.tries >= 4 {
            s.phase = Phase::Selecting;
            s.offer = None;
            s.tries = 0;
        }
        // 1, 2, 4, 8 s, then every 16 s (RFC 2131 §4.1); renewing: every 10 s
        let wait = match s.phase {
            Phase::Renewing | Phase::Rebinding => 10_000,
            _ => 1000u64 << s.tries.min(4),
        };
        s.tries += 1;
        s.next_tx = now + wait;
        (true, *s)
    });
    if send {
        send_for(n, &state);
    }
}

/// A frame received on interface `n`: `true` if it was a DHCP answer for it.
pub fn input(n: usize, f: &[u8]) -> bool {
    // fast path: only UDP to port 68
    if f.len() < 14 + 20 + 8 || f[12..14] != [8, 0] || f[23] != 17 {
        return false;
    }
    let ihl = (f[14] & 0xf) as usize * 4;
    if f.len() < 14 + ihl + 4 || f[14 + ihl + 2..14 + ihl + 4] != [0, 68] {
        return false;
    }
    let xid = with_if(n, |s| s.xid);
    let Some(r) = parse_dhcp(f, xid) else { return true };
    let now = crate::arch::uptime_ms();
    let mut changed = false;
    with_if(n, |s| match (s.phase, r.typ) {
        (Phase::Selecting, 2) => {
            s.offer = Some((r.yiaddr, r.server));
            s.phase = Phase::Requesting;
            s.tries = 0;
            s.next_tx = 0;
        }
        (Phase::Requesting | Phase::Renewing | Phase::Rebinding, 5) => {
            let secs = if r.lease == 0 { 3600 } else { r.lease };
            let t1 = if r.t1 == 0 || r.t1 >= secs { secs / 2 } else { r.t1 };
            let t2 = if r.t2 == 0 || r.t2 >= secs || r.t2 <= t1 { secs / 8 * 7 } else { r.t2 };
            let ms = |x: u32| now + x as u64 * 1000;
            let mut cfg = r.cfg;
            if cfg.ip == [0; 4] {
                cfg.ip = r.yiaddr;
            }
            s.cfg = Some(cfg);
            s.lease = Some(Lease { server: r.server, secs, t1: ms(t1), t2: ms(t2), end: if r.lease == u32::MAX { u64::MAX } else { ms(secs) } });
            s.phase = Phase::Bound;
            s.offer = None;
            changed = true;
        }
        (Phase::Requesting | Phase::Renewing | Phase::Rebinding, 6) => {
            // NAK: the address is not ours (any more)
            s.cfg = None;
            s.lease = None;
            s.phase = Phase::Selecting;
            s.xid = new_xid();
            s.tries = 0;
            s.next_tx = now + 500;
            changed = true;
        }
        _ => {}
    });
    if changed {
        settle_active();
        NET_GEN.fetch_add(1, Ordering::Relaxed);
    }
    true
}

/// Look at every interface: link changes, DHCP timers, answers on interfaces other than the
/// active one (the socket layer reads the active one). Called from the network poll; does
/// its work at most every 100 ms.
pub fn tick() {
    let now = crate::arch::uptime_ms();
    let last = LAST_TICK.load(Ordering::Relaxed);
    if now.saturating_sub(last) < 100 || LAST_TICK.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_err() {
        return;
    }
    let count = NET.lock().len();
    for n in 0..count {
        let (link, detached) = match NET.lock().get(n) {
            Some(d) => (d.link().unwrap_or(true), d.detached()),
            None => continue,
        };
        if detached {
            continue;
        }
        let (was, phase, fixed) = with_if(n, |s| (s.link, s.phase, s.fixed));
        if was != Some(link) {
            with_if(n, |s| {
                s.link = Some(link);
                if was.is_some() {
                    s.link_changes += 1;
                }
            });
            NET_GEN.fetch_add(1, Ordering::Relaxed);
            if !link {
                with_if(n, |s| s.phase = Phase::NoLink);
            } else if let Some(c) = fixed {
                set_static(n, c);
            } else if was.is_some() || phase == Phase::Idle || phase == Phase::NoLink {
                // cable (back) in: ask for an address (the server gives the old one again)
                start_dhcp(n);
            }
        }
        if !link {
            continue;
        }
        if n != active_nic() || CONFIG.lock().is_none() {
            let busy = with_if(n, |s| matches!(s.phase, Phase::Selecting | Phase::Requesting | Phase::Renewing | Phase::Rebinding));
            if busy && n != active_nic() {
                let mut buf = [0u8; 2048];
                for _ in 0..16 {
                    let got = NET.lock().get_mut(n).and_then(|d| d.recv(&mut buf));
                    match got {
                        Some(len) => {
                            input(n, &buf[..len]);
                        }
                        None => break,
                    }
                }
            }
        }
        dhcp_timers(n, now);
    }
}

/// Adresse per DHCP holen (Discover → Offer → Request → Ack), wartend (Start, Kommando `dhcp`)
pub fn dhcp(n: usize) -> Option<Config> {
    NET.lock().get(n)?;
    start_dhcp(n);
    let end = crate::arch::uptime_ms() + 4500;
    let mut buf = [0u8; 2048];
    while crate::arch::uptime_ms() < end {
        dhcp_timers(n, crate::arch::uptime_ms());
        let got = NET.lock().get_mut(n).and_then(|d| d.recv(&mut buf));
        match got {
            Some(len) => {
                input(n, &buf[..len]);
            }
            None => crate::arch::pause(),
        }
        if with_if(n, |s| s.phase == Phase::Bound) {
            let link = NET.lock().get(n).and_then(|d| d.link()).unwrap_or(true);
            with_if(n, |s| s.link = Some(link));
            set_active_nic(n);
            settle_active();
            return with_if(n, |s| s.cfg);
        }
    }
    None
}

/// Try DHCP on every present, linked network interface. This keeps Ethernet
/// fallback working on laptops/docks when eth0 is not the cable-facing NIC.
/// Interfaces without an answer keep trying in the background (`tick`).
pub fn dhcp_all() -> Option<(usize, Config)> {
    let count = NET.lock().len();
    for n in 0..count {
        let (linked, detached) = NET.lock().get(n).map(|d| (d.link().unwrap_or(true), d.detached())).unwrap_or((false, true));
        if detached {
            continue;
        }
        with_if(n, |s| s.link = Some(linked));
        if linked {
            if let Some(c) = dhcp(n) {
                return Some((n, c));
            }
        } else {
            with_if(n, |s| s.phase = Phase::NoLink);
        }
    }
    None
}

/// How often the link of interface `n` came or went since it was found.
pub fn link_changes(n: usize) -> u32 {
    with_if(n, |s| s.link_changes)
}

fn ip_text(a: [u8; 4]) -> String {
    alloc::format!("{}.{}.{}.{}", a[0], a[1], a[2], a[3])
}

fn prefix(mask: [u8; 4]) -> u32 {
    u32::from_be_bytes(mask).count_ones()
}

/// `/sys/class/net/<if>/ultar_ipv4`: the address state of interface `n` as `key=value` lines.
pub fn ipv4_text(n: usize) -> String {
    let now = crate::arch::uptime_ms();
    let s = with_if(n, |s| *s);
    let active = active_nic() == n && CONFIG.lock().is_some();
    let mut t = alloc::format!("mode={}\nstate={}\nactive={}\nlink_changes={}\n", if s.fixed.is_some() { "static" } else { "dhcp" }, s.phase.word(), u8::from(active), s.link_changes);
    if let Some(c) = s.cfg {
        t += &alloc::format!("address={}/{}\n", ip_text(c.ip), prefix(c.mask));
        if c.gw != [0; 4] {
            t += &alloc::format!("gateway={}\n", ip_text(c.gw));
        }
        let dns: Vec<String> = [c.dns, c.dns2].iter().filter(|d| **d != [0; 4]).map(|d| ip_text(*d)).collect();
        if !dns.is_empty() {
            t += &alloc::format!("dns={}\n", dns.join(" "));
        }
    }
    if let Some(l) = s.lease {
        let left = |x: u64| if x == u64::MAX { u64::MAX / 1000 } else { x.saturating_sub(now) / 1000 };
        t += &alloc::format!("server={}\nlease={}\nrenew_in={}\nexpires_in={}\n", ip_text(l.server), l.secs, left(l.t1), left(l.end));
    }
    t
}

/// A write to `ultar_ipv4`: `dhcp`, `renew` or `static <a.b.c.d>/<nn> [router] [dns[,dns2]]`.
pub fn ipv4_write(n: usize, text: &str) -> Result<(), &'static str> {
    let mut w = text.split_whitespace();
    match w.next() {
        Some("dhcp") => start_dhcp(n),
        Some("renew") => renew(n),
        Some("static") => {
            let a = w.next().ok_or("address missing")?;
            let (ipt, nn) = a.split_once('/').ok_or("prefix missing")?;
            let ip = parse_ip(ipt).ok_or("bad address")?;
            let nn: u32 = nn.parse().ok().filter(|x| (1..=32).contains(x)).ok_or("bad prefix")?;
            let mask = (u32::MAX << (32 - nn)).to_be_bytes();
            let gw = match w.next() {
                Some("-") | None => [0; 4],
                Some(g) => parse_ip(g).ok_or("bad router")?,
            };
            let mut cfg = Config { ip, mask, gw, ..Default::default() };
            if let Some(d) = w.next() {
                let mut it = d.split(',');
                cfg.dns = it.next().and_then(parse_ip).ok_or("bad dns")?;
                if let Some(d2) = it.next() {
                    cfg.dns2 = parse_ip(d2).ok_or("bad dns")?;
                }
            }
            if w.next().is_some() {
                return Err("too many words");
            }
            set_static(n, cfg);
        }
        _ => return Err("unknown command"),
    }
    Ok(())
}

