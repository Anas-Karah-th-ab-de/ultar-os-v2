//! Sockets für Programme (Linux-ABI): AF_INET mit UDP und TCP (Client und Server), Loopback 127.0.0.1.
//! Eigene TCP-Umsetzung: Drei-Wege-Handshake, Folgenummern, Bestätigungen, Wiederholung nach Zeitablauf,
//! FIN/RST, Annahme-Warteschlange. Der Netzwerkstapel wird vom Planer (≈100 Hz) und bei jedem Aufruf abgefragt.

use crate::driver::NET;
use crate::net::{self, csum, eth, ipv4, CONFIG};
use alloc::collections::{BTreeMap, VecDeque};
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};
use spin::Mutex;

pub const EAGAIN: i64 = 11;
pub const ECONNREFUSED: i64 = 111;
pub const ENOTCONN: i64 = 107;
pub const ETIMEDOUT: i64 = 110;
pub const ECONNRESET: i64 = 104;
pub const EADDRINUSE: i64 = 98;

/// erhöht sich bei jedem Netzereignis (weckt Wartende)
pub static GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tcp {
    Closed,
    Listen,
    SynSent,
    SynRcvd,
    Established,
    FinWait,
    CloseWait,
    LastAck,
}

pub struct Sock {
    pub udp: bool,
    pub lip: [u8; 4],
    pub lport: u16,
    pub rip: [u8; 4],
    pub rport: u16,
    pub st: Tcp,
    snd_una: u32,
    snd_nxt: u32,
    rcv_nxt: u32,
    /// noch nicht bestätigte Daten (ab snd_una)
    unacked: Vec<u8>,
    fin_sent: bool,
    last_tx: u64,
    tries: u32,
    pub rx: VecDeque<u8>,
    pub dgrams: VecDeque<([u8; 4], u16, Vec<u8>)>,
    pub fin_rcvd: bool,
    pub err: i64,
    pub backlog: VecDeque<usize>,
    parent: Option<usize>,
    pub nonblock: bool,
    peer_win: u32,
    /// UDP: Gegenstellen, an die dieses Socket gesendet hat (deren Antworten dürfen herein)
    peers: Vec<([u8; 4], u16)>,
}

impl Sock {
    fn new(udp: bool) -> Sock {
        Sock {
            udp,
            lip: [0; 4],
            lport: 0,
            rip: [0; 4],
            rport: 0,
            st: Tcp::Closed,
            snd_una: 0,
            snd_nxt: 0,
            rcv_nxt: 0,
            unacked: Vec::new(),
            fin_sent: false,
            last_tx: 0,
            tries: 0,
            rx: VecDeque::new(),
            dgrams: VecDeque::new(),
            fin_rcvd: false,
            err: 0,
            backlog: VecDeque::new(),
            parent: None,
            nonblock: false,
            peer_win: 65535,
            peers: Vec::new(),
        }
    }
}

pub static SOCKS: Mutex<BTreeMap<usize, Sock>> = Mutex::new(BTreeMap::new());
/// Obergrenzen gegen Überflutung aus dem Netz
const MAX_SOCKS: usize = 4096;
const MAX_HALBOFFEN: usize = 64;
const MAX_RX: usize = 256 * 1024;
const MAX_ARP: usize = 256;
const MAX_TXQ: usize = 1024;

/// SYN-Cookies: ist die Warteschlange halboffener Verbindungen voll (SYN-Flut), wird kein Zustand mehr
/// angelegt – die Folgenummer der Antwort selbst trägt einen Prüfwert. Nur wer die Antwort wirklich
/// erhalten hat (echter Gegenüber), kann die Verbindung mit dem passenden ACK aufbauen.
static COOKIE_GEHEIM: AtomicU64 = AtomicU64::new(0);

fn cookie(src: [u8; 4], sp: u16, dp: u16, fenster: u64) -> u32 {
    let mut g = COOKIE_GEHEIM.load(Ordering::Relaxed);
    if g == 0 {
        g = crate::arch::zufall() | 1;
        COOKIE_GEHEIM.store(g, Ordering::Relaxed);
    }
    // splitmix64 über Geheimnis, Adressen, Ports und Zeitfenster (64 s)
    let mut x = g ^ (u32::from_be_bytes(src) as u64) << 32 ^ (sp as u64) << 16 ^ dp as u64 ^ fenster.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    for _ in 0..2 {
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        x ^= x >> 31;
    }
    x as u32
}

fn cookie_fenster() -> u64 {
    crate::arch::uptime_ms() / 64_000
}
static NEXT_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_PORT: AtomicU64 = AtomicU64::new(40000);
static ARP: Mutex<Vec<([u8; 4], [u8; 6])>> = Mutex::new(Vec::new());
static LOOP: Mutex<VecDeque<Vec<u8>>> = Mutex::new(VecDeque::new());

/// Griff auf einen Socket im Dateideskriptor; schließt beim letzten Freigeben
pub struct Handle(pub usize);
impl Drop for Handle {
    fn drop(&mut self) {
        close(self.0);
    }
}

pub fn create(udp: bool) -> usize {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed) as usize;
    SOCKS.lock().insert(id, Sock::new(udp));
    id
}

/// zufälliger freier Port 40000–59999 (nicht vorhersagbar – erschwert das Einschleusen von Paketen)
fn ephemeral() -> u16 {
    let _ = NEXT_PORT.fetch_add(1, Ordering::Relaxed);
    let ss = SOCKS.try_lock();
    for _ in 0..64 {
        let p = (40000 + crate::arch::zufall() % 20000) as u16;
        match &ss {
            Some(ss) if ss.values().any(|s| s.lport == p) => continue,
            _ => return p,
        }
    }
    (40000 + crate::arch::zufall() % 20000) as u16
}

fn my_ip() -> [u8; 4] {
    CONFIG.lock().map(|c| c.ip).unwrap_or([10, 0, 2, 15])
}

fn is_local(ip: [u8; 4]) -> bool {
    ip[0] == 127 || ip == my_ip()
}

fn mac() -> Option<[u8; 6]> {
    NET.lock().first().map(|d| d.mac())
}

fn raw_send(frame: &[u8]) {
    if let Some(d) = NET.lock().first_mut() {
        let _ = d.send(frame);
    }
}

static TXQ: Mutex<VecDeque<(u64, [u8; 4], Vec<u8>)>> = Mutex::new(VecDeque::new());
static ARP_ASKED: Mutex<Vec<([u8; 4], u64)>> = Mutex::new(Vec::new());

/// IP-Paket zum Senden einreihen (gesendet wird in `flush`, ohne gehaltene Sperren)
fn send_ip(dst: [u8; 4], proto: u8, payload: &[u8]) {
    if (proto == 17 || proto == 6) && payload.len() >= 4 {
        let (sp, dp) = (u16::from_be_bytes([payload[0], payload[1]]), u16::from_be_bytes([payload[2], payload[3]]));
        if !crate::verbund::erlaubt(proto, sp, dp) {
            return; // Verbund AUS: nichts verlässt das Gerät
        }
    }
    let src = if dst[0] == 127 { [127, 0, 0, 1] } else { my_ip() };
    let pkt = ipv4(src, dst, proto, payload);
    let mut q = TXQ.lock();
    if q.len() >= MAX_TXQ {
        q.pop_front();
    }
    q.push_back((crate::arch::uptime_ms(), dst, pkt));
}

/// Warteschlange senden; Ziele ohne bekannte MAC per ARP erfragen (bis 3 s)
pub fn flush() {
    let now = crate::arch::uptime_ms();
    let q: Vec<(u64, [u8; 4], Vec<u8>)> = TXQ.lock().drain(..).collect();
    let Some(cfg) = *CONFIG.lock() else { return };
    let Some(my) = mac() else { return };
    let mut keep = Vec::new();
    for (t, dst, pkt) in q {
        if is_local(dst) {
            LOOP.lock().push_back(eth([0; 6], [0; 6], 0x0800, &pkt));
            continue;
        }
        // Rundruf (255.255.255.255 oder Netz-Broadcast): an alle, ohne ARP
        let bc = [cfg.ip[0] | !cfg.mask[0], cfg.ip[1] | !cfg.mask[1], cfg.ip[2] | !cfg.mask[2], cfg.ip[3] | !cfg.mask[3]];
        if dst == [255; 4] || dst == bc {
            raw_send(&eth([0xff; 6], my, 0x0800, &pkt));
            continue;
        }
        let hop = if net::same_net(cfg, dst) { dst } else { cfg.gw };
        let m = ARP.lock().iter().find(|e| e.0 == hop).map(|e| e.1);
        match m {
            Some(m) => raw_send(&eth(m, my, 0x0800, &pkt)),
            None => {
                if now - t < 3000 {
                    let mut asked = ARP_ASKED.lock();
                    let last = asked.iter().find(|a| a.0 == hop).map(|a| a.1).unwrap_or(0);
                    if now.saturating_sub(last) > 300 || last == 0 {
                        asked.retain(|a| a.0 != hop);
                        asked.push((hop, now));
                        let mut a = vec![0, 1, 8, 0, 6, 4, 0, 1];
                        a.extend_from_slice(&my);
                        a.extend_from_slice(&cfg.ip);
                        a.extend_from_slice(&[0; 6]);
                        a.extend_from_slice(&hop);
                        raw_send(&eth([0xff; 6], my, 0x0806, &a));
                    }
                    keep.push((t, dst, pkt));
                }
            }
        }
    }
    let mut q = TXQ.lock();
    for k in keep.into_iter().rev() {
        q.push_front(k);
    }
}

fn tcp_csum(src: [u8; 4], dst: [u8; 4], seg: &[u8]) -> u16 {
    let mut ph = Vec::with_capacity(12 + seg.len());
    ph.extend_from_slice(&src);
    ph.extend_from_slice(&dst);
    ph.extend_from_slice(&[0, 6]);
    ph.extend_from_slice(&(seg.len() as u16).to_be_bytes());
    ph.extend_from_slice(seg);
    csum(&ph)
}

const FIN: u8 = 1;
const SYN: u8 = 2;
const RST: u8 = 4;
const PSH: u8 = 8;
const ACK: u8 = 16;

fn tcp_out(s: &Sock, seq: u32, flags: u8, data: &[u8]) {
    let mut t = vec![0u8; 20];
    t[0..2].copy_from_slice(&s.lport.to_be_bytes());
    t[2..4].copy_from_slice(&s.rport.to_be_bytes());
    t[4..8].copy_from_slice(&seq.to_be_bytes());
    t[8..12].copy_from_slice(&s.rcv_nxt.to_be_bytes());
    if flags & SYN != 0 {
        t[12] = 6 << 4;
        t.extend_from_slice(&[2, 4, 0x05, 0xb4]); // MSS 1460
    } else {
        t[12] = 5 << 4;
    }
    t[13] = flags;
    t[14..16].copy_from_slice(&65535u16.to_be_bytes());
    t.extend_from_slice(data);
    let src = if s.rip[0] == 127 { [127, 0, 0, 1] } else { my_ip() };
    let c = tcp_csum(src, s.rip, &t);
    t[16..18].copy_from_slice(&c.to_be_bytes());
    send_ip(s.rip, 6, &t);
}

/// eingehende Rahmen verarbeiten, Wiederholungen senden
pub fn poll() {
    let mut frames: Vec<Vec<u8>> = LOOP.lock().drain(..).collect();
    {
        let mut buf = [0u8; 2048];
        let mut nl = NET.lock();
        if let Some(d) = nl.first_mut() {
            for _ in 0..64 {
                match d.recv(&mut buf) {
                    Some(n) => frames.push(buf[..n].to_vec()),
                    None => break,
                }
            }
        }
    }
    let had = !frames.is_empty();
    for f in frames {
        handle(&f);
    }
    retransmit();
    flush();
    if had || !LOOP.lock().is_empty() {
        // Loopback-Antworten sofort verarbeiten
        let more: Vec<Vec<u8>> = LOOP.lock().drain(..).collect();
        for f in more {
            handle(&f);
        }
        flush();
    }
}

fn handle(f: &[u8]) {
    if f.len() < 14 {
        return;
    }
    let typ = u16::from_be_bytes([f[12], f[13]]);
    if typ == 0x0806 && f.len() >= 42 {
        let (op, sha, spa, tpa) = (u16::from_be_bytes([f[20], f[21]]), &f[22..28], &f[28..32], &f[38..42]);
        let mut ip = [0u8; 4];
        ip.copy_from_slice(spa);
        let mut m = [0u8; 6];
        m.copy_from_slice(sha);
        // nur lernen, was uns betrifft: Antwort auf eigene Frage oder Anfrage an uns (gegen ARP-Fälschung/-Flut)
        let gefragt = ARP_ASKED.lock().iter().any(|a| a.0 == ip);
        if (op == 2 && gefragt) || (op == 1 && tpa == my_ip()) {
            let mut a = ARP.lock();
            a.retain(|e| e.0 != ip);
            if a.len() >= MAX_ARP {
                a.remove(0);
            }
            a.push((ip, m));
        }
        if op == 1 && tpa == my_ip() {
            if let Some(my) = mac() {
                let mut r = vec![0, 1, 8, 0, 6, 4, 0, 2];
                r.extend_from_slice(&my);
                r.extend_from_slice(&my_ip());
                r.extend_from_slice(&m);
                r.extend_from_slice(&ip);
                raw_send(&eth(m, my, 0x0806, &r));
            }
        }
        return;
    }
    if typ != 0x0800 || f.len() < 34 {
        return;
    }
    let ip = &f[14..];
    let ihl = (ip[0] & 0xf) as usize * 4;
    let tot = (u16::from_be_bytes([ip[2], ip[3]]) as usize).min(ip.len());
    if tot < ihl {
        return;
    }
    let mut src = [0u8; 4];
    src.copy_from_slice(&ip[12..16]);
    let mut dst = [0u8; 4];
    dst.copy_from_slice(&ip[16..20]);
    let body = &ip[ihl..tot];
    match ip[9] {
        1 if !body.is_empty() && body[0] == 8 && (src[0] == 127 || crate::firewall::PING.load(Ordering::Relaxed)) => {
            // Echo-Antwort
            let mut r = body.to_vec();
            r[0] = 0;
            r[2] = 0;
            r[3] = 0;
            let c = csum(&r);
            r[2..4].copy_from_slice(&c.to_be_bytes());
            send_ip(src, 1, &r);
        }
        17 if body.len() >= 8 => {
            let sp = u16::from_be_bytes([body[0], body[1]]);
            let dp = u16::from_be_bytes([body[2], body[3]]);
            let len = (u16::from_be_bytes([body[4], body[5]]) as usize).clamp(8, body.len());
            if !crate::verbund::erlaubt(17, sp, dp) {
                return;
            }
            let mut ss = SOCKS.lock();
            if let Some(s) = ss.values_mut().find(|s| s.udp && s.lport == dp) {
                let antwort = s.peers.contains(&(src, sp)) || (s.rip == src && s.rport == sp);
                if !antwort && !crate::firewall::eingehend_erlaubt(17, src, dp) {
                    return; // Brandmauer: niemand hat diesen Port geöffnet
                }
                if s.dgrams.len() < 256 {
                    s.dgrams.push_back((src, sp, body[8..len].to_vec()));
                    GEN.fetch_add(1, Ordering::Relaxed);
                }
            } else if dp == crate::verbund::UDP_PORT {
                drop(ss);
                crate::verbund::meldung(src, &body[8..len]);
            }
        }
        6 if body.len() >= 20 => {
            let (sp, dp) = (u16::from_be_bytes([body[0], body[1]]), u16::from_be_bytes([body[2], body[3]]));
            if crate::verbund::erlaubt(6, sp, dp) {
                tcp_in(src, dst, body)
            }
        }
        _ => {}
    }
}

fn tcp_in(src: [u8; 4], _dst: [u8; 4], seg: &[u8]) {
    let sp = u16::from_be_bytes([seg[0], seg[1]]);
    let dp = u16::from_be_bytes([seg[2], seg[3]]);
    let seq = u32::from_be_bytes(seg[4..8].try_into().unwrap());
    let ack = u32::from_be_bytes(seg[8..12].try_into().unwrap());
    let off = ((seg[12] >> 4) as usize * 4).min(seg.len());
    let fl = seg[13];
    let win = u16::from_be_bytes([seg[14], seg[15]]) as u32;
    let data = &seg[off..];
    let mut ss = SOCKS.lock();
    GEN.fetch_add(1, Ordering::Relaxed);
    let id = ss.iter().find(|(_, s)| !s.udp && s.lport == dp && s.rport == sp && s.rip == src && s.st != Tcp::Listen).map(|(k, _)| *k);
    let Some(id) = id else {
        // neue Verbindung an einem lauschenden Socket?
        if fl & SYN != 0 && fl & ACK == 0 {
            if !crate::firewall::eingehend_erlaubt(6, src, dp) {
                return; // Brandmauer: still verwerfen – von außen sieht der Port „gefiltert“ aus
            }
            if let Some((&lid, _)) = ss.iter().find(|(_, s)| !s.udp && s.st == Tcp::Listen && s.lport == dp) {
                let halboffen = ss.values().filter(|s| s.parent == Some(lid) && s.st == Tcp::SynRcvd).count();
                if halboffen >= MAX_HALBOFFEN || ss.len() >= MAX_SOCKS {
                    // SYN-Flut: zustandslos mit Cookie antworten
                    let mut t = Sock::new(false);
                    t.lport = dp;
                    t.rport = sp;
                    t.rip = src;
                    t.rcv_nxt = seq.wrapping_add(1);
                    tcp_out(&t, cookie(src, sp, dp, cookie_fenster()), SYN | ACK, &[]);
                    return;
                }
                if ss[&lid].backlog.len() < 64 {
                    let nid = NEXT_ID.fetch_add(1, Ordering::Relaxed) as usize;
                    let mut c = Sock::new(false);
                    c.lip = my_ip();
                    c.lport = dp;
                    c.rip = src;
                    c.rport = sp;
                    c.st = Tcp::SynRcvd;
                    c.rcv_nxt = seq.wrapping_add(1);
                    let iss = crate::arch::zufall() as u32;
                    c.snd_una = iss;
                    c.snd_nxt = iss.wrapping_add(1);
                    c.parent = Some(lid);
                    c.last_tx = crate::arch::uptime_ms();
                    tcp_out(&c, iss, SYN | ACK, &[]);
                    ss.insert(nid, c);
                }
                return;
            }
        }
        // ACK auf eine Cookie-Antwort: Verbindung jetzt anlegen
        if fl & ACK != 0 && fl & (SYN | RST) == 0 && crate::firewall::eingehend_erlaubt(6, src, dp) {
            let f = cookie_fenster();
            let gueltig = [f, f.wrapping_sub(1)].iter().any(|&w| ack.wrapping_sub(1) == cookie(src, sp, dp, w));
            if gueltig && ss.len() < MAX_SOCKS {
                if let Some((&lid, _)) = ss.iter().find(|(_, s)| !s.udp && s.st == Tcp::Listen && s.lport == dp) {
                    if ss[&lid].backlog.len() < 64 {
                        let nid = NEXT_ID.fetch_add(1, Ordering::Relaxed) as usize;
                        let mut c = Sock::new(false);
                        c.lip = my_ip();
                        c.lport = dp;
                        c.rip = src;
                        c.rport = sp;
                        c.st = Tcp::Established;
                        c.rcv_nxt = seq;
                        c.snd_una = ack;
                        c.snd_nxt = ack;
                        c.parent = Some(lid);
                        c.last_tx = crate::arch::uptime_ms();
                        c.peer_win = win;
                        if !data.is_empty() {
                            c.rx.extend(data.iter().copied());
                            c.rcv_nxt = seq.wrapping_add(data.len() as u32);
                            tcp_out(&c, ack, ACK, &[]);
                        }
                        ss.insert(nid, c);
                        if let Some(l) = ss.get_mut(&lid) {
                            l.backlog.push_back(nid);
                        }
                        return;
                    }
                }
            }
        }
        if fl & RST == 0 && src[0] == 127 {
            // unbekannt: nur lokal zurücksetzen – nach außen keine Antwort (verrät nichts)
            let mut t = Sock::new(false);
            t.lport = dp;
            t.rport = sp;
            t.rip = src;
            t.rcv_nxt = seq.wrapping_add(data.len() as u32 + (fl & SYN != 0) as u32);
            tcp_out(&t, if fl & ACK != 0 { ack } else { 0 }, RST | ACK, &[]);
        }
        return;
    };
    let s = ss.get_mut(&id).unwrap();
    s.peer_win = win;
    if fl & RST != 0 {
        // nur echte RST annehmen (passende Folgenummer bzw. Bestätigung) – gegen blind eingeschleuste Abbrüche
        let echt = if s.st == Tcp::SynSent { fl & ACK != 0 && ack == s.snd_nxt } else { seq == s.rcv_nxt };
        if !echt {
            return;
        }
        s.err = if s.st == Tcp::SynSent { -ECONNREFUSED } else { -ECONNRESET };
        s.st = Tcp::Closed;
        s.fin_rcvd = true;
        return;
    }
    match s.st {
        Tcp::SynSent => {
            if fl & SYN != 0 && fl & ACK != 0 && ack == s.snd_nxt {
                s.rcv_nxt = seq.wrapping_add(1);
                s.snd_una = ack;
                s.st = Tcp::Established;
                tcp_out(s, s.snd_nxt, ACK, &[]);
            }
            return;
        }
        Tcp::SynRcvd => {
            if fl & ACK != 0 && ack == s.snd_nxt {
                s.snd_una = ack;
                s.st = Tcp::Established;
                if let Some(p) = s.parent {
                    if let Some(l) = ss.get_mut(&p) {
                        l.backlog.push_back(id);
                    }
                }
            }
            if data.is_empty() {
                return;
            }
        }
        _ => {}
    }
    let s = ss.get_mut(&id).unwrap();
    // Bestätigung eigener Daten
    if fl & ACK != 0 {
        let acked = ack.wrapping_sub(s.snd_una);
        if acked > 0 && acked <= s.snd_nxt.wrapping_sub(s.snd_una) {
            let n = (acked as usize).min(s.unacked.len());
            s.unacked.drain(..n);
            s.snd_una = ack;
            s.tries = 0;
            if s.fin_sent && s.snd_una == s.snd_nxt {
                if s.st == Tcp::LastAck {
                    s.st = Tcp::Closed;
                }
            }
        }
    }
    // Daten in Reihenfolge annehmen
    let mut need_ack = false;
    if !data.is_empty() {
        if seq == s.rcv_nxt && s.rx.len() + data.len() <= MAX_RX {
            s.rx.extend(data.iter().copied());
            s.rcv_nxt = s.rcv_nxt.wrapping_add(data.len() as u32);
        }
        need_ack = true;
    }
    if fl & FIN != 0 && seq.wrapping_add(data.len() as u32) == s.rcv_nxt {
        s.rcv_nxt = s.rcv_nxt.wrapping_add(1);
        s.fin_rcvd = true;
        s.st = match s.st {
            Tcp::Established | Tcp::SynRcvd => Tcp::CloseWait,
            Tcp::FinWait => Tcp::Closed,
            x => x,
        };
        need_ack = true;
    }
    if need_ack {
        tcp_out(s, s.snd_nxt, ACK, &[]);
    }
}

fn retransmit() {
    let now = crate::arch::uptime_ms();
    let mut ss = SOCKS.lock();
    let mut dead = Vec::new();
    for (id, s) in ss.iter_mut() {
        if s.udp || matches!(s.st, Tcp::Closed | Tcp::Listen) {
            continue;
        }
        let pending = s.st == Tcp::SynSent || s.st == Tcp::SynRcvd || !s.unacked.is_empty() || (s.fin_sent && s.snd_una != s.snd_nxt);
        if !pending || now < s.last_tx + 400 * (1 << s.tries.min(5)) {
            continue;
        }
        s.tries += 1;
        s.last_tx = now;
        if s.tries > 8 {
            s.err = -ETIMEDOUT;
            s.st = Tcp::Closed;
            s.fin_rcvd = true;
            GEN.fetch_add(1, Ordering::Relaxed);
            if s.parent.is_some() {
                dead.push(*id);
            }
            continue;
        }
        match s.st {
            Tcp::SynSent => tcp_out(s, s.snd_una, SYN, &[]),
            Tcp::SynRcvd => tcp_out(s, s.snd_una, SYN | ACK, &[]),
            _ => {
                if !s.unacked.is_empty() {
                    let n = s.unacked.len().min(1460);
                    let d = s.unacked[..n].to_vec();
                    tcp_out(s, s.snd_una, ACK | PSH, &d);
                } else if s.fin_sent {
                    tcp_out(s, s.snd_nxt.wrapping_sub(1), FIN | ACK, &[]);
                }
            }
        }
    }
    // geschlossene Sockets ohne Besitzer entfernen
    for (id, s) in ss.iter() {
        if s.parent == Some(usize::MAX) && (s.st == Tcp::Closed || now > s.last_tx + 10_000) {
            dead.push(*id);
        }
    }
    for id in dead {
        ss.remove(&id);
    }
}

// ------------------------------------------------------------------ Aufrufe der Programme

pub fn parse_addr(b: &[u8]) -> Option<([u8; 4], u16)> {
    if b.len() < 8 || u16::from_le_bytes([b[0], b[1]]) != 2 {
        return None;
    }
    Some(([b[4], b[5], b[6], b[7]], u16::from_be_bytes([b[2], b[3]])))
}

pub fn make_addr(ip: [u8; 4], port: u16) -> Vec<u8> {
    let mut b = vec![0u8; 16];
    b[0..2].copy_from_slice(&2u16.to_le_bytes());
    b[2..4].copy_from_slice(&port.to_be_bytes());
    b[4..8].copy_from_slice(&ip);
    b
}

pub fn bind(id: usize, ip: [u8; 4], port: u16) -> i64 {
    let mut ss = SOCKS.lock();
    let udp = ss.get(&id).map(|s| s.udp).unwrap_or(false);
    if port != 0 && ss.iter().any(|(k, s)| *k != id && s.udp == udp && s.lport == port && s.parent.is_none()) {
        return -EADDRINUSE;
    }
    let Some(s) = ss.get_mut(&id) else { return -9 };
    s.lip = ip;
    s.lport = if port == 0 { ephemeral() } else { port };
    0
}

pub fn listen(id: usize) -> i64 {
    let mut ss = SOCKS.lock();
    let Some(s) = ss.get_mut(&id) else { return -9 };
    if s.lport == 0 {
        s.lport = ephemeral();
    }
    s.st = Tcp::Listen;
    0
}

/// Verbindung aufbauen; Ok(true) = fertig, Ok(false) = noch im Aufbau
pub fn connect(id: usize, ip: [u8; 4], port: u16) -> Result<bool, i64> {
    let ip = if ip == [0; 4] { [127, 0, 0, 1] } else { ip };
    let mut ss = SOCKS.lock();
    let Some(s) = ss.get_mut(&id) else { return Err(-9) };
    if s.lport == 0 {
        s.lport = ephemeral();
    }
    if s.udp {
        s.rip = ip;
        s.rport = port;
        return Ok(true);
    }
    match s.st {
        Tcp::Established | Tcp::CloseWait => return Ok(true),
        Tcp::SynSent => return Ok(false),
        Tcp::Closed if s.err != 0 => return Err(core::mem::replace(&mut s.err, 0)),
        _ => {}
    }
    s.rip = ip;
    s.rport = port;
    let iss = crate::arch::zufall() as u32;
    s.snd_una = iss;
    s.snd_nxt = iss.wrapping_add(1);
    s.st = Tcp::SynSent;
    s.last_tx = crate::arch::uptime_ms();
    s.tries = 0;
    tcp_out(s, iss, SYN, &[]);
    drop(ss);
    poll();
    let ss = SOCKS.lock();
    Ok(ss.get(&id).map(|s| s.st == Tcp::Established).unwrap_or(false))
}

pub fn accept(id: usize) -> Option<usize> {
    SOCKS.lock().get_mut(&id)?.backlog.pop_front()
}

/// Daten senden; Anzahl oder Fehler
pub fn send(id: usize, data: &[u8], to: Option<([u8; 4], u16)>) -> i64 {
    let mut ss = SOCKS.lock();
    let Some(s) = ss.get_mut(&id) else { return -9 };
    if s.udp {
        if s.lport == 0 {
            s.lport = ephemeral();
        }
        let (ip, port) = to.unwrap_or((s.rip, s.rport));
        if port == 0 {
            return -89; // EDESTADDRREQ
        }
        let ip = if ip == [0; 4] { [127, 0, 0, 1] } else { ip };
        if !s.peers.contains(&(ip, port)) {
            if s.peers.len() >= 16 {
                s.peers.remove(0);
            }
            s.peers.push((ip, port));
        }
        let mut u = Vec::with_capacity(8 + data.len());
        u.extend_from_slice(&s.lport.to_be_bytes());
        u.extend_from_slice(&port.to_be_bytes());
        u.extend_from_slice(&((8 + data.len()) as u16).to_be_bytes());
        u.extend_from_slice(&[0, 0]);
        u.extend_from_slice(data);
        drop(ss);
        send_ip(ip, 17, &u);
        flush();
        poll();
        return data.len() as i64;
    }
    if !matches!(s.st, Tcp::Established | Tcp::CloseWait) {
        return if s.err != 0 { s.err } else { -32 };
    }
    // in Segmente zu 1460 Byte, höchstens 64 KB unbestätigt
    let room = 65536usize.saturating_sub(s.unacked.len());
    let n = data.len().min(room);
    if n == 0 {
        return -EAGAIN;
    }
    let mut off = 0;
    let mut segs = Vec::new();
    while off < n {
        let k = (n - off).min(1460);
        segs.push((s.snd_nxt, data[off..off + k].to_vec()));
        s.unacked.extend_from_slice(&data[off..off + k]);
        s.snd_nxt = s.snd_nxt.wrapping_add(k as u32);
        off += k;
    }
    s.last_tx = crate::arch::uptime_ms();
    for (seq, d) in segs {
        tcp_out(s, seq, ACK | PSH, &d);
    }
    drop(ss);
    flush();
    n as i64
}

pub enum Recv {
    Data(Vec<u8>, Option<([u8; 4], u16)>),
    Eof,
    Wait,
    Err(i64),
}

pub fn recv(id: usize, max: usize, peek: bool) -> Recv {
    poll();
    let mut ss = SOCKS.lock();
    let Some(s) = ss.get_mut(&id) else { return Recv::Err(-9) };
    if s.udp {
        return match if peek { s.dgrams.front().cloned() } else { s.dgrams.pop_front() } {
            Some((ip, port, mut d)) => {
                d.truncate(max);
                Recv::Data(d, Some((ip, port)))
            }
            None => Recv::Wait,
        };
    }
    if !s.rx.is_empty() {
        let n = max.min(s.rx.len());
        let d: Vec<u8> = if peek { s.rx.iter().take(n).copied().collect() } else { s.rx.drain(..n).collect() };
        return Recv::Data(d, None);
    }
    if s.fin_rcvd || s.st == Tcp::Closed {
        if s.err != 0 && s.err != -ECONNREFUSED {
            return Recv::Err(core::mem::replace(&mut s.err, 0));
        }
        return Recv::Eof;
    }
    if matches!(s.st, Tcp::Listen | Tcp::Closed) && s.rport == 0 {
        return Recv::Err(-ENOTCONN);
    }
    Recv::Wait
}

pub fn shutdown_write(id: usize) {
    let mut ss = SOCKS.lock();
    if let Some(s) = ss.get_mut(&id) {
        if !s.udp && !s.fin_sent && matches!(s.st, Tcp::Established | Tcp::CloseWait) {
            s.fin_sent = true;
            let seq = s.snd_nxt;
            s.snd_nxt = s.snd_nxt.wrapping_add(1);
            s.st = if s.st == Tcp::CloseWait { Tcp::LastAck } else { Tcp::FinWait };
            s.last_tx = crate::arch::uptime_ms();
            tcp_out(s, seq, FIN | ACK, &[]);
        }
    }
    drop(ss);
    flush();
}

pub fn close(id: usize) {
    shutdown_write(id);
    let mut ss = SOCKS.lock();
    let gone = match ss.get(&id) {
        Some(s) => s.udp || matches!(s.st, Tcp::Closed | Tcp::Listen | Tcp::SynSent),
        None => false,
    };
    if gone {
        ss.remove(&id);
    } else if let Some(s) = ss.get_mut(&id) {
        // wird nach FIN-Austausch/Zeitablauf entfernt
        s.parent = Some(usize::MAX);
    }
}

/// Lesebereit (Daten, Verbindung zum Annehmen, Ende), schreibbereit
pub fn readiness(id: usize) -> (bool, bool) {
    let ss = SOCKS.lock();
    let Some(s) = ss.get(&id) else { return (true, true) };
    if s.udp {
        return (!s.dgrams.is_empty(), true);
    }
    let r = !s.rx.is_empty() || !s.backlog.is_empty() || s.fin_rcvd || s.st == Tcp::Closed;
    let w = matches!(s.st, Tcp::Established | Tcp::CloseWait) && s.unacked.len() < 65536 || s.st == Tcp::Closed;
    (r, w)
}

pub fn names(id: usize) -> Option<(([u8; 4], u16), ([u8; 4], u16))> {
    let ss = SOCKS.lock();
    let s = ss.get(&id)?;
    let lip = if s.lip == [0; 4] { my_ip() } else { s.lip };
    Some(((lip, s.lport), (s.rip, s.rport)))
}

pub fn set_nonblock(id: usize, nb: bool) {
    if let Some(s) = SOCKS.lock().get_mut(&id) {
        s.nonblock = nb;
    }
}

pub fn is_nonblock(id: usize) -> bool {
    SOCKS.lock().get(&id).map(|s| s.nonblock).unwrap_or(false)
}

pub fn pending_error(id: usize) -> i64 {
    SOCKS.lock().get_mut(&id).map(|s| core::mem::replace(&mut s.err, 0)).unwrap_or(0)
}

/// Anzeige für `netz`: offene Verbindungen
pub fn list() -> Vec<alloc::string::String> {
    SOCKS
        .lock()
        .values()
        .map(|s| {
            alloc::format!(
                "{} {}.{}.{}.{}:{} -> {}.{}.{}.{}:{} {:?}",
                if s.udp { "udp" } else { "tcp" },
                s.lip[0], s.lip[1], s.lip[2], s.lip[3], s.lport, s.rip[0], s.rip[1], s.rip[2], s.rip[3], s.rport, s.st
            )
        })
        .collect()
}

/// Ereignis melden (Rohr beschrieben/geschlossen) – weckt Wartende
pub fn bump() {
    GEN.fetch_add(1, Ordering::Relaxed);
}

pub fn gen() -> u64 {
    GEN.load(Ordering::Relaxed)
}
