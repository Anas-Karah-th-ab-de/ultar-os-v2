//! Systemfunktionen ohne Oberfläche (rein und testbar): Zeitzonen mit Sommerzeit, SNTP, PIN,
//! Einheiten, Kalender, Nachtlicht, Auto-Helligkeit, Akku-Regeln, Suche, Netzdiagnose, Sicherung.
//! Die Oberfläche dazu liegt in `app_sys.rs`, der Hintergrunddienst in `sysd.rs`.

use serde::{Deserialize, Serialize};
use std::time::Duration;

// ---------------------------------------------------------------- Einstellungen

/// Zusätzliche Systemeinstellungen (/data/ultar/system.toml) – getrennt von settings.toml.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct SysCfg {
    // Sperre
    pub lock: bool,
    pub pin_hash: String,
    pub pin_salt: String,
    /// Entsperren per Fingerabdruck (nur mit PIN)
    pub fp_unlock: bool,
    /// Entsperren per Gesicht (nur mit PIN; Bequemlichkeit)
    pub face_unlock: bool,
    // Zeit
    pub zone: String,
    pub auto_time: bool,
    pub ntp_server: String,
    pub h12: bool,
    pub last_sync: i64,
    // Energie
    pub saver: bool,
    pub auto_saver: bool,
    pub saver_at: u8,
    pub perf: String,
    pub stay_awake_charging: bool,
    // Anzeige
    pub night: bool,
    pub night_strength: f32,
    pub night_sched: bool,
    pub night_from: u16,
    pub night_to: u16,
    pub auto_bright: bool,
    pub contrast: bool,
    // Ton
    /// 0 Normal, 1 Vibration, 2 Lautlos
    pub profile: u8,
    pub dnd: bool,
    pub dnd_sched: bool,
    pub dnd_from: u16,
    pub dnd_to: u16,
    pub haptic: bool,
    // Netz
    pub dns: String,
    pub data_limit_mb: u64,
    // Entwickler
    pub show_touches: bool,
    pub show_fps: bool,
    // Weltuhr
    pub world: Vec<String>,
}

impl Default for SysCfg {
    fn default() -> Self {
        Self {
            lock: false,
            pin_hash: String::new(),
            pin_salt: String::new(),
            fp_unlock: false,
            face_unlock: false,
            zone: String::new(),
            auto_time: true,
            ntp_server: "pool.ntp.org".into(),
            h12: false,
            last_sync: 0,
            saver: false,
            auto_saver: true,
            saver_at: 15,
            perf: String::new(),
            stay_awake_charging: false,
            night: false,
            night_strength: 0.5,
            night_sched: false,
            night_from: 21 * 60,
            night_to: 7 * 60,
            auto_bright: false,
            contrast: false,
            profile: 0,
            dnd: false,
            dnd_sched: false,
            dnd_from: 22 * 60,
            dnd_to: 7 * 60,
            haptic: false,
            dns: String::new(),
            data_limit_mb: 0,
            show_touches: false,
            show_fps: false,
            world: vec![
                "Europe/Berlin".into(),
                "Asia/Damascus".into(),
                "America/New_York".into(),
            ],
        }
    }
}

/// Liegt `now` (Minuten seit Mitternacht) im Zeitfenster [from, to)? Fenster über Mitternacht erlaubt.
pub fn in_window(now: u16, from: u16, to: u16) -> bool {
    if from == to {
        return false;
    }
    if from < to {
        now >= from && now < to
    } else {
        now >= from || now < to
    }
}

// ---------------------------------------------------------------- Zeitzonen + Sommerzeit

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Dst {
    None,
    /// EU: letzter So im März 01:00 UTC bis letzter So im Oktober 01:00 UTC
    Eu,
    /// USA/Kanada: 2. So im März 02:00 Ortszeit bis 1. So im November 02:00 Ortszeit
    Us,
    /// Ägypten (seit 2023): letzter Fr im April 00:00 bis letzter Do im Oktober 24:00 Ortszeit
    Egypt,
    /// Libanon: letzter So im März 00:00 bis letzter So im Oktober 00:00 Ortszeit
    Lebanon,
}

/// (Kennung, Anzeigename, Normalzeit in Minuten, Regel)
pub const ZONES: &[(&str, &str, i32, Dst)] = &[
    ("UTC", "UTC", 0, Dst::None),
    ("Europe/London", "London", 0, Dst::Eu),
    ("Europe/Berlin", "Berlin", 60, Dst::Eu),
    ("Europe/Vienna", "Wien", 60, Dst::Eu),
    ("Europe/Zurich", "Zürich", 60, Dst::Eu),
    ("Europe/Paris", "Paris", 60, Dst::Eu),
    ("Europe/Amsterdam", "Amsterdam", 60, Dst::Eu),
    ("Europe/Stockholm", "Stockholm", 60, Dst::Eu),
    ("Europe/Athens", "Athen", 120, Dst::Eu),
    ("Europe/Istanbul", "Istanbul", 180, Dst::None),
    ("Europe/Moscow", "Moskau", 180, Dst::None),
    ("Asia/Damascus", "Damaskus", 180, Dst::None),
    ("Asia/Beirut", "Beirut", 120, Dst::Lebanon),
    ("Asia/Amman", "Amman", 180, Dst::None),
    ("Asia/Baghdad", "Bagdad", 180, Dst::None),
    ("Asia/Riyadh", "Riad", 180, Dst::None),
    ("Asia/Qatar", "Doha", 180, Dst::None),
    ("Asia/Dubai", "Dubai", 240, Dst::None),
    ("Africa/Cairo", "Kairo", 120, Dst::Egypt),
    ("Asia/Tehran", "Teheran", 210, Dst::None),
    ("Asia/Karachi", "Karatschi", 300, Dst::None),
    ("Asia/Kolkata", "Delhi", 330, Dst::None),
    ("Asia/Shanghai", "Peking", 480, Dst::None),
    ("Asia/Tokyo", "Tokio", 540, Dst::None),
    ("America/New_York", "New York", -300, Dst::Us),
    ("America/Chicago", "Chicago", -360, Dst::Us),
    ("America/Denver", "Denver", -420, Dst::Us),
    ("America/Los_Angeles", "Los Angeles", -480, Dst::Us),
    ("America/Sao_Paulo", "São Paulo", -180, Dst::None),
];

pub fn zone(id: &str) -> Option<(&'static str, &'static str, i32, Dst)> {
    ZONES.iter().copied().find(|z| z.0 == id)
}

/// Tag (1..31) des n-ten (n>=1) bzw. letzten (n=0) Wochentags `wd` (0=So) im Monat.
fn nth_weekday(y: i64, m: i64, wd: usize, n: u32) -> i64 {
    let dim = crate::sys::days_in_month(y, m);
    let wd_of = |d: i64| {
        crate::sys::from_unix(
            crate::sys::to_unix(
                &crate::sys::DateTime {
                    y,
                    mo: m,
                    d,
                    h: 12,
                    mi: 0,
                    s: 0,
                    wd: 0,
                },
                0,
            ),
            0,
        )
        .wd
    };
    if n == 0 {
        (1..=dim).rev().find(|&d| wd_of(d) == wd).unwrap_or(dim)
    } else {
        let first = (1..=7).find(|&d| wd_of(d) == wd).unwrap_or(1);
        (first + 7 * (n as i64 - 1)).min(dim)
    }
}

fn utc_at(y: i64, m: i64, d: i64, h: i64, off_min: i32) -> i64 {
    crate::sys::to_unix(
        &crate::sys::DateTime {
            y,
            mo: m,
            d,
            h,
            mi: 0,
            s: 0,
            wd: 0,
        },
        off_min,
    )
}

/// Versatz zu UTC in Minuten für eine Zone zu einem Zeitpunkt (inkl. Sommerzeit).
pub fn offset_at(zone_id: &str, t: i64) -> Option<i32> {
    let (_, _, std, rule) = zone(zone_id)?;
    let y = crate::sys::from_unix(t, std).y;
    let summer = match rule {
        Dst::None => false,
        Dst::Eu => {
            t >= utc_at(y, 3, nth_weekday(y, 3, 0, 0), 1, 0)
                && t < utc_at(y, 10, nth_weekday(y, 10, 0, 0), 1, 0)
        }
        Dst::Us => {
            t >= utc_at(y, 3, nth_weekday(y, 3, 0, 2), 2, std)
                && t < utc_at(y, 11, nth_weekday(y, 11, 0, 1), 2, std + 60)
        }
        Dst::Egypt => {
            t >= utc_at(y, 4, nth_weekday(y, 4, 5, 0), 0, std)
                && t < utc_at(y, 10, nth_weekday(y, 10, 4, 0), 24, std + 60)
        }
        Dst::Lebanon => {
            t >= utc_at(y, 3, nth_weekday(y, 3, 0, 0), 0, std)
                && t < utc_at(y, 10, nth_weekday(y, 10, 0, 0), 0, std + 60)
        }
    };
    Some(std + if summer { 60 } else { 0 })
}

pub fn fmt_offset(min: i32) -> String {
    let s = if min < 0 { '-' } else { '+' };
    let a = min.abs();
    if a % 60 == 0 {
        format!("UTC{s}{}", a / 60)
    } else {
        format!("UTC{s}{}:{:02}", a / 60, a % 60)
    }
}

/// Uhrzeit im 24- oder 12-Stunden-Format.
pub fn fmt_clock(h: i64, mi: i64, h12: bool) -> String {
    if h12 {
        let hh = if h % 12 == 0 { 12 } else { h % 12 };
        format!("{hh}:{mi:02} {}", if h < 12 { "AM" } else { "PM" })
    } else {
        format!("{h:02}:{mi:02}")
    }
}

// ---------------------------------------------------------------- SNTP

const NTP_EPOCH: i64 = 2_208_988_800; // 1900 -> 1970

/// SNTP-Anfrage (RFC 4330, Client-Modus 3, Version 4).
pub fn sntp_request() -> [u8; 48] {
    let mut p = [0u8; 48];
    p[0] = 0b00_100_011;
    p
}

/// Antwort auswerten: Unix-Zeit in Millisekunden (aus dem Sende-Zeitstempel des Servers).
pub fn sntp_parse(b: &[u8]) -> Option<i64> {
    if b.len() < 48 || b[0] & 0x07 != 4 || b[1] == 0 {
        return None; // kein Server-Modus oder „Kiss of death" (Stratum 0)
    }
    let secs = u32::from_be_bytes([b[40], b[41], b[42], b[43]]) as i64;
    let frac = u32::from_be_bytes([b[44], b[45], b[46], b[47]]) as i64;
    if secs == 0 {
        return None;
    }
    Some((secs - NTP_EPOCH) * 1000 + (frac * 1000 >> 32))
}

/// Nameserver aus resolv.conf (Ultar schreibt /run/ultar/resolv.conf).
pub fn nameservers() -> Vec<std::net::Ipv4Addr> {
    let mut v = Vec::new();
    for p in ["/run/ultar/resolv.conf", "/etc/resolv.conf"] {
        for l in std::fs::read_to_string(p).unwrap_or_default().lines() {
            if let Some(ip) = l
                .trim()
                .strip_prefix("nameserver")
                .and_then(|x| x.trim().parse().ok())
            {
                if !v.contains(&ip) {
                    v.push(ip);
                }
            }
        }
    }
    v
}

/// DNS-Anfrage (Typ A) bauen.
pub fn dns_query(id: u16, host: &str) -> Vec<u8> {
    let mut q = Vec::new();
    q.extend_from_slice(&id.to_be_bytes());
    q.extend_from_slice(&[0x01, 0x00, 0, 1, 0, 0, 0, 0, 0, 0]);
    for part in host.trim_end_matches('.').split('.') {
        q.push(part.len() as u8);
        q.extend_from_slice(part.as_bytes());
    }
    q.extend_from_slice(&[0, 0, 1, 0, 1]);
    q
}

/// Erste A-Adresse aus einer DNS-Antwort.
pub fn dns_parse(b: &[u8], id: u16) -> Option<std::net::Ipv4Addr> {
    if b.len() < 12 || u16::from_be_bytes([b[0], b[1]]) != id || b[3] & 0x0F != 0 {
        return None;
    }
    let (qd, an) = (
        u16::from_be_bytes([b[4], b[5]]),
        u16::from_be_bytes([b[6], b[7]]),
    );
    let mut i = 12;
    let skip_name = |mut i: usize| -> Option<usize> {
        loop {
            let l = *b.get(i)? as usize;
            if l == 0 {
                return Some(i + 1);
            }
            if l & 0xC0 == 0xC0 {
                return Some(i + 2);
            }
            i += l + 1;
        }
    };
    for _ in 0..qd {
        i = skip_name(i)? + 4;
    }
    for _ in 0..an {
        i = skip_name(i)?;
        let ty = u16::from_be_bytes([*b.get(i)?, *b.get(i + 1)?]);
        let len = u16::from_be_bytes([*b.get(i + 8)?, *b.get(i + 9)?]) as usize;
        i += 10;
        if ty == 1 && len == 4 {
            return Some(std::net::Ipv4Addr::new(b[i], b[i + 1], b[i + 2], b[i + 3]));
        }
        i += len;
    }
    None
}

/// Name auflösen: zuerst über das System, sonst eigene UDP-Anfrage an die Nameserver aus resolv.conf.
pub fn resolve(host: &str) -> Result<std::net::Ipv4Addr, String> {
    use std::net::ToSocketAddrs;
    if let Ok(ip) = host.parse() {
        return Ok(ip);
    }
    if let Some(std::net::SocketAddr::V4(a)) = (host, 0)
        .to_socket_addrs()
        .ok()
        .and_then(|mut it| it.find(|a| a.is_ipv4()))
    {
        return Ok(*a.ip());
    }
    let s = std::net::UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    let id = (std::process::id() as u16) ^ 0x5a5a;
    for ns in nameservers() {
        if s.send_to(&dns_query(id, host), (ns, 53)).is_err() {
            continue;
        }
        let mut buf = [0u8; 512];
        if let Ok((n, _)) = s.recv_from(&mut buf) {
            if let Some(ip) = dns_parse(&buf[..n], id) {
                return Ok(ip);
            }
        }
    }
    Err("DNS".into())
}

/// Zeit vom Server holen (UDP 123, 3 s Zeitlimit). Ergebnis: Unix-Millisekunden.
/// Ist der Name nicht auflösbar, wird ein fester Zeitserver per IP genommen (time.google.com).
pub fn sntp_query(server: &str) -> Result<i64, String> {
    use std::net::UdpSocket;
    let ip = resolve(server).unwrap_or(std::net::Ipv4Addr::new(216, 239, 35, 0));
    let addr = std::net::SocketAddrV4::new(ip, 123);
    let s = UdpSocket::bind("0.0.0.0:0").map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let t0 = std::time::Instant::now();
    s.send_to(&sntp_request(), addr)
        .map_err(|e| e.to_string())?;
    let mut b = [0u8; 64];
    let (n, _) = s.recv_from(&mut b).map_err(|e| e.to_string())?;
    let ms = sntp_parse(&b[..n]).ok_or("Antwort ungültig")?;
    Ok(ms + t0.elapsed().as_millis() as i64 / 2)
}

/// HTTP-Datum („Thu, 19 Nov 2026 19:48:14 GMT") in Unix-Sekunden.
pub fn parse_http_date(s: &str) -> Option<i64> {
    let p: Vec<&str> = s.split_whitespace().collect();
    if p.len() < 5 {
        return None;
    }
    let d: i64 = p[1].parse().ok()?;
    let mo = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|m| *m == p[2])? as i64
        + 1;
    let y: i64 = p[3].parse().ok()?;
    let t: Vec<i64> = p[4].split(':').filter_map(|x| x.parse().ok()).collect();
    if t.len() != 3 {
        return None;
    }
    Some(crate::sys::to_unix(
        &crate::sys::DateTime {
            y,
            mo,
            d,
            h: t[0],
            mi: t[1],
            s: t[2],
            wd: 0,
        },
        0,
    ))
}

/// Ersatz-Zeitquelle, wenn UDP 123 gesperrt ist: „Date"-Kopf einer HTTP-Antwort (TCP 80).
pub fn http_time() -> Result<i64, String> {
    use std::io::{Read, Write};
    let mut last = String::new();
    for (ip, host) in [
        ("1.1.1.1", "one.one.one.one"),
        ("8.8.4.4", "dns.google"),
        ("142.250.185.78", "www.google.com"),
    ] {
        let addr: std::net::SocketAddr = format!("{ip}:80").parse().unwrap();
        let t0 = std::time::Instant::now();
        let mut s = match std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(4)) {
            Ok(s) => s,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(4)));
        let _ = s.write_all(format!("HEAD / HTTP/1.0\r\nHost: {host}\r\n\r\n").as_bytes());
        let mut buf = vec![0u8; 4096];
        let n = s.read(&mut buf).unwrap_or(0);
        let txt = String::from_utf8_lossy(&buf[..n]);
        if let Some(t) = txt
            .lines()
            .find_map(|l| l.strip_prefix("Date:").or_else(|| l.strip_prefix("date:")))
            .and_then(|v| parse_http_date(v.trim()))
        {
            return Ok(t * 1000 + 500 + t0.elapsed().as_millis() as i64 / 2);
        }
        last = "kein Date-Kopf".into();
    }
    Err(last)
}

/// Systemuhr und Hardware-Uhr stellen (root nötig).
pub fn set_clock(unix_ms: i64) -> bool {
    crate::sys::set_time((unix_ms + 500) / 1000).is_ok()
}

// ---------------------------------------------------------------- PIN

/// Kleine SHA-256 (FIPS 180-4) – nur für den PIN-Abgleich auf dem Gerät.
pub fn sha256(data: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut m = data.to_vec();
    let bits = (data.len() as u64) * 8;
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&bits.to_be_bytes());
    for c in m.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([c[4 * i], c[4 * i + 1], c[4 * i + 2], c[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7]
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let mj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(mj);
            v = [
                t1.wrapping_add(t2),
                v[0],
                v[1],
                v[2],
                v[3].wrapping_add(t1),
                v[4],
                v[5],
                v[6],
            ];
        }
        for i in 0..8 {
            h[i] = h[i].wrapping_add(v[i]);
        }
    }
    let mut out = [0u8; 32];
    for i in 0..8 {
        out[4 * i..4 * i + 4].copy_from_slice(&h[i].to_be_bytes());
    }
    out
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// PIN-Hash: 20 000 Runden SHA-256 über Salz + PIN (bremst Durchprobieren der gespeicherten Datei).
pub fn pin_hash(pin: &str, salt: &str) -> String {
    let mut d = sha256(format!("ultar-pin\n{salt}\n{pin}").as_bytes());
    for _ in 0..20_000 {
        let mut x = d.to_vec();
        x.extend_from_slice(salt.as_bytes());
        d = sha256(&x);
    }
    hex(&d)
}

pub fn new_salt() -> String {
    let mut b = [0u8; 16];
    if let Ok(mut f) = std::fs::File::open("/dev/urandom") {
        use std::io::Read;
        let _ = f.read_exact(&mut b);
    } else {
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(1);
        b[..16].copy_from_slice(&t.to_le_bytes());
    }
    hex(&b)
}

pub fn pin_valid(pin: &str) -> bool {
    (4..=8).contains(&pin.len()) && pin.chars().all(|c| c.is_ascii_digit())
}

/// Wartezeit nach Fehlversuchen: ab 5 Fehlern 30 s, danach jeweils doppelt, höchstens 15 min.
pub fn lockout_secs(fails: u32) -> u64 {
    if fails < 5 {
        0
    } else {
        (30u64 << (fails - 5).min(5)).min(900)
    }
}

// ---------------------------------------------------------------- Einheiten

/// (Kategorie, [(Einheit, Faktor zur Basiseinheit)]); Temperatur wird gesondert gerechnet.
pub const UNITS: &[(&str, &[(&str, f64)])] = &[
    (
        "Length",
        &[
            ("mm", 0.001),
            ("cm", 0.01),
            ("m", 1.0),
            ("km", 1000.0),
            ("in", 0.0254),
            ("ft", 0.3048),
            ("yd", 0.9144),
            ("mi", 1609.344),
        ],
    ),
    (
        "Weight",
        &[
            ("g", 0.001),
            ("kg", 1.0),
            ("t", 1000.0),
            ("oz", 0.028349523125),
            ("lb", 0.45359237),
        ],
    ),
    ("Temperature", &[("°C", 1.0), ("°F", 1.0), ("K", 1.0)]),
    (
        "Area",
        &[
            ("m²", 1.0),
            ("km²", 1e6),
            ("ha", 1e4),
            ("ft²", 0.09290304),
            ("acre", 4046.8564224),
        ],
    ),
    (
        "Capacity",
        &[
            ("ml", 0.001),
            ("l", 1.0),
            ("m³", 1000.0),
            ("gal (US)", 3.785411784),
            ("cup", 0.2365882365),
        ],
    ),
    (
        "Speed",
        &[
            ("km/h", 1.0 / 3.6),
            ("m/s", 1.0),
            ("mph", 0.44704),
            ("kn", 0.514444),
        ],
    ),
    (
        "Data",
        &[
            ("B", 1.0),
            ("KB", 1e3),
            ("MB", 1e6),
            ("GB", 1e9),
            ("KiB", 1024.0),
            ("MiB", 1048576.0),
            ("GiB", 1073741824.0),
        ],
    ),
];

pub fn convert(cat: usize, from: usize, to: usize, v: f64) -> f64 {
    let (name, units) = UNITS[cat];
    if name == "Temperature" {
        let c = match from {
            1 => (v - 32.0) * 5.0 / 9.0,
            2 => v - 273.15,
            _ => v,
        };
        return match to {
            1 => c * 9.0 / 5.0 + 32.0,
            2 => c + 273.15,
            _ => c,
        };
    }
    v * units[from].1 / units[to].1
}

pub fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return "–".into();
    }
    let s = if v.abs() >= 1e9 || (v != 0.0 && v.abs() < 1e-4) {
        format!("{v:.4e}")
    } else {
        format!("{v:.6}")
    };
    if s.contains('e') {
        return s;
    }
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

// ---------------------------------------------------------------- Kalender

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(default)]
pub struct Event {
    pub y: i64,
    pub mo: i64,
    pub d: i64,
    /// Minuten seit Mitternacht; -1 = ganztägig
    pub at: i32,
    pub title: String,
    /// Erinnerung so viele Minuten vorher (0 = keine)
    pub remind: i32,
    pub notified: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Calendar {
    pub events: Vec<Event>,
}

impl Event {
    pub fn start_unix(&self, tz_min: i32) -> i64 {
        let m = self.at.max(0) as i64;
        crate::sys::to_unix(
            &crate::sys::DateTime {
                y: self.y,
                mo: self.mo,
                d: self.d,
                h: m / 60,
                mi: m % 60,
                s: 0,
                wd: 0,
            },
            tz_min,
        )
    }
}

/// Monatsraster: (Wochentag des 1. mit Montag=0, Tage im Monat).
pub fn month_grid(y: i64, mo: i64) -> (usize, i64) {
    let wd = crate::sys::from_unix(
        crate::sys::to_unix(
            &crate::sys::DateTime {
                y,
                mo,
                d: 1,
                h: 12,
                mi: 0,
                s: 0,
                wd: 0,
            },
            0,
        ),
        0,
    )
    .wd;
    ((wd + 6) % 7, crate::sys::days_in_month(y, mo))
}

/// Fällige Erinnerungen (Indizes) zum Zeitpunkt `now`.
pub fn due_reminders(cal: &Calendar, now: i64, tz_min: i32) -> Vec<usize> {
    cal.events
        .iter()
        .enumerate()
        .filter(|(_, e)| e.remind > 0 && !e.notified && e.at >= 0)
        .filter(|(_, e)| {
            let s = e.start_unix(tz_min);
            now >= s - e.remind as i64 * 60 && now < s + 3600
        })
        .map(|(i, _)| i)
        .collect()
}

// ---------------------------------------------------------------- Anzeige

/// Nachtlicht: Blau- und Grünanteil absenken (Pixel 0x00RRGGBB). s = Stärke 0..1.
pub fn night_tint(px: &mut [u32], s: f32) {
    let s = s.clamp(0.0, 1.0);
    if s <= 0.0 {
        return;
    }
    let gk = ((1.0 - 0.28 * s) * 256.0) as u32;
    let bk = ((1.0 - 0.68 * s) * 256.0) as u32;
    for p in px.iter_mut() {
        let v = *p;
        let g = (((v >> 8) & 0xFF) * gk) >> 8;
        let b = ((v & 0xFF) * bk) >> 8;
        *p = (v & 0xFFFF_0000) | (g << 8) | b;
    }
}

/// Helligkeit 0.05..1 aus Umgebungslicht (Lux), logarithmisch.
pub fn lux_to_brightness(lux: f32) -> f32 {
    if lux <= 1.0 {
        return 0.05;
    }
    (0.05 + 0.95 * (lux.log10() / 4.0)).clamp(0.05, 1.0)
}

// ---------------------------------------------------------------- Akku und Wärme

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BattEvent {
    Low(u8),
    ChargingStarted,
    Full,
    SaverOn,
    SaverOff,
}

/// Ereignisse beim Übergang `prev` -> `cur` (Prozent, lädt).
pub fn battery_events(
    prev: Option<(u8, bool)>,
    cur: (u8, bool),
    auto_saver: bool,
    saver_at: u8,
    saver_on: bool,
) -> Vec<BattEvent> {
    let mut v = Vec::new();
    let Some((pp, pc)) = prev else { return v };
    let (p, c) = cur;
    if !c {
        for lim in [15u8, 5] {
            if pp > lim && p <= lim {
                v.push(BattEvent::Low(lim));
            }
        }
    }
    if c && !pc {
        v.push(BattEvent::ChargingStarted);
    }
    if c && p >= 100 && pp < 100 {
        v.push(BattEvent::Full);
    }
    if auto_saver && !saver_on && !c && p <= saver_at && pp > saver_at {
        v.push(BattEvent::SaverOn);
    }
    if auto_saver && saver_on && c && !pc {
        v.push(BattEvent::SaverOff);
    }
    v
}

/// Höchste Temperatur aller Thermal-Zonen in °C mit Name.
pub fn hottest_zone() -> Option<(String, f32)> {
    let mut best: Option<(String, f32)> = None;
    for e in std::fs::read_dir("/sys/class/thermal").ok()?.flatten() {
        let p = e.path();
        let Some(t) = std::fs::read_to_string(p.join("temp"))
            .ok()
            .and_then(|s| s.trim().parse::<f32>().ok())
        else {
            continue;
        };
        let c = if t > 1000.0 { t / 1000.0 } else { t };
        if !(1.0..150.0).contains(&c) {
            continue;
        }
        let n = std::fs::read_to_string(p.join("type"))
            .unwrap_or_default()
            .trim()
            .to_string();
        if best.as_ref().map_or(true, |b| c > b.1) {
            best = Some((n, c));
        }
    }
    best
}

/// Verfügbare CPU-Regler (aus cpu0) und den aktuellen.
pub fn governors() -> (Vec<String>, String) {
    let base = "/sys/devices/system/cpu/cpu0/cpufreq";
    let av =
        std::fs::read_to_string(format!("{base}/scaling_available_governors")).unwrap_or_default();
    let cur = std::fs::read_to_string(format!("{base}/scaling_governor")).unwrap_or_default();
    (
        av.split_whitespace().map(String::from).collect(),
        cur.trim().to_string(),
    )
}

/// Regler auf allen Kernen setzen. Liefert die Zahl der Kerne, bei denen es geklappt hat.
pub fn set_governor(g: &str) -> usize {
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir("/sys/devices/system/cpu") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("cpu")
                && name[3..].chars().all(|c| c.is_ascii_digit())
                && !name[3..].is_empty()
            {
                n += std::fs::write(e.path().join("cpufreq/scaling_governor"), g).is_ok() as usize;
            }
        }
    }
    n
}

/// Passender Regler für ein Profil: „power" / „balanced" / „performance".
pub fn governor_for(profile: &str, avail: &[String]) -> Option<String> {
    let want: &[&str] = match profile {
        "power" => &["powersave", "conservative"],
        "performance" => &["performance"],
        _ => &["schedutil", "interactive", "cfinteractive", "ondemand"],
    };
    want.iter()
        .find(|w| avail.iter().any(|a| a == *w))
        .map(|s| s.to_string())
}

// ---------------------------------------------------------------- Suche

fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'ä' => 'a',
            'ö' => 'o',
            'ü' => 'u',
            'ß' => 's',
            'é' | 'è' | 'ê' => 'e',
            'á' | 'à' | 'â' => 'a',
            'أ' | 'إ' | 'آ' => 'ا',
            'ة' => 'ه',
            'ى' => 'ي',
            c => c,
        })
        .filter(|c| !matches!(*c, '\u{064B}'..='\u{0652}'))
        .collect()
}

/// Trefferwert (höher = besser), None = kein Treffer. Anfang > Wortanfang > enthalten > Buchstabenfolge.
pub fn score(query: &str, name: &str) -> Option<u32> {
    let q = fold(query.trim());
    if q.is_empty() {
        return None;
    }
    let n = fold(name);
    if n == q {
        return Some(1000);
    }
    if n.starts_with(&q) {
        return Some(800 - n.len().min(200) as u32);
    }
    if n.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&q))
    {
        return Some(600);
    }
    if n.contains(&q) {
        return Some(400);
    }
    let mut it = n.chars();
    if q.chars().all(|c| it.any(|x| x == c)) && q.chars().count() >= 3 {
        return Some(100);
    }
    None
}

// ---------------------------------------------------------------- Netzdiagnose

fn csum(b: &[u8]) -> u16 {
    let mut s = 0u32;
    for c in b.chunks(2) {
        s += u16::from_be_bytes([c[0], *c.get(1).unwrap_or(&0)]) as u32;
    }
    while s >> 16 != 0 {
        s = (s & 0xFFFF) + (s >> 16);
    }
    !(s as u16)
}

/// ICMP-Echo (root): Antwortzeit in ms.
pub fn ping(ip: std::net::Ipv4Addr, timeout_ms: u64) -> Result<f32, String> {
    use nix::libc;
    unsafe {
        let fd = libc::socket(
            libc::AF_INET,
            libc::SOCK_RAW | libc::SOCK_CLOEXEC,
            libc::IPPROTO_ICMP,
        );
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let tv = libc::timeval {
            tv_sec: (timeout_ms / 1000) as _,
            tv_usec: ((timeout_ms % 1000) * 1000) as _,
        };
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            &tv as *const _ as *const _,
            std::mem::size_of::<libc::timeval>() as u32,
        );
        let id = (std::process::id() & 0xFFFF) as u16;
        let mut pkt = vec![8u8, 0, 0, 0];
        pkt.extend_from_slice(&id.to_be_bytes());
        pkt.extend_from_slice(&1u16.to_be_bytes());
        pkt.extend_from_slice(b"ultar-ping-0123456789");
        let c = csum(&pkt);
        pkt[2..4].copy_from_slice(&c.to_be_bytes());
        let mut sa: libc::sockaddr_in = std::mem::zeroed();
        sa.sin_family = libc::AF_INET as _;
        sa.sin_addr.s_addr = u32::from_ne_bytes(ip.octets());
        let t0 = std::time::Instant::now();
        let r = libc::sendto(
            fd,
            pkt.as_ptr() as *const _,
            pkt.len(),
            0,
            &sa as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as u32,
        );
        if r < 0 {
            let e = std::io::Error::last_os_error().to_string();
            libc::close(fd);
            return Err(e);
        }
        let mut buf = [0u8; 256];
        loop {
            let n = libc::recv(fd, buf.as_mut_ptr() as *mut _, buf.len(), 0);
            if n < 0 {
                libc::close(fd);
                return Err("Zeitüberschreitung".into());
            }
            let n = n as usize;
            let ihl = ((buf[0] & 0x0F) as usize) * 4;
            if n >= ihl + 8
                && buf[ihl] == 0
                && u16::from_be_bytes([buf[ihl + 4], buf[ihl + 5]]) == id
            {
                libc::close(fd);
                return Ok(t0.elapsed().as_secs_f32() * 1000.0);
            }
            if t0.elapsed() > Duration::from_millis(timeout_ms) {
                libc::close(fd);
                return Err("Zeitüberschreitung".into());
            }
        }
    }
}

/// DNS-Auflösung testen (System-Resolver): Dauer in ms und erste Adresse.
pub fn dns_test(host: &str) -> Result<(f32, String), String> {
    let t0 = std::time::Instant::now();
    let ip = resolve(host)?;
    Ok((t0.elapsed().as_secs_f32() * 1000.0, ip.to_string()))
}

/// TCP-Verbindung zu einem Dienst im Internet: Dauer in ms.
pub fn tcp_test(addr: &str) -> Result<f32, String> {
    use std::net::ToSocketAddrs;
    let a = addr
        .to_socket_addrs()
        .map_err(|e| e.to_string())?
        .next()
        .ok_or("Adresse")?;
    let t0 = std::time::Instant::now();
    std::net::TcpStream::connect_timeout(&a, Duration::from_secs(4)).map_err(|e| e.to_string())?;
    Ok(t0.elapsed().as_secs_f32() * 1000.0)
}

/// Datenverbrauch des laufenden Monats (Bytes, rx+tx) aus der Monitor-Statistik (Tagesschlüssel JJJJ-MM-TT).
pub fn month_usage(days: &std::collections::BTreeMap<String, (u64, u64)>, y: i64, mo: i64) -> u64 {
    let p = format!("{y:04}-{mo:02}");
    days.iter()
        .filter(|(k, _)| k.starts_with(&p))
        .map(|(_, v)| v.0 + v.1)
        .sum()
}

// ---------------------------------------------------------------- Sicherung, Bereinigen, Protokoll

/// Was gesichert wird (relativ zu /data/ultar). Zugangsdaten (wifi.conf, Konto) bewusst ausgenommen.
pub const BACKUP_FILES: &[&str] = &[
    "settings.toml",
    "system.toml",
    "net.toml",
    "notes.toml",
    "contacts.toml",
    "calendar.toml",
    "alarms.toml",
    "launches.toml",
    "agent.toml",
    "apps.toml",
    "messages.toml",
    "calc-history.toml",
];

pub fn backup_root() -> String {
    std::env::var("ULTAR_BACKUP").unwrap_or_else(|_| "/data/Backups".into())
}

/// Sicherung anlegen: Ordner /data/Backups/ultar-JJJJMMTT-HHMMSS. Liefert Pfad und Dateizahl.
pub fn backup_create(stamp: &str) -> Result<(String, usize), String> {
    backup_create_in(&crate::sys::data_dir(), &backup_root(), stamp)
}

fn backup_create_in(src: &str, root: &str, stamp: &str) -> Result<(String, usize), String> {
    let dir = format!("{root}/ultar-{stamp}");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut n = 0;
    for f in BACKUP_FILES {
        let s = format!("{src}/{f}");
        if std::path::Path::new(&s).exists() {
            std::fs::copy(&s, format!("{dir}/{f}")).map_err(|e| format!("{f}: {e}"))?;
            n += 1;
        }
    }
    Ok((dir, n))
}

pub fn backup_list() -> Vec<String> {
    backup_list_in(&backup_root())
}

fn backup_list_in(root: &str) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().is_dir())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("ultar-"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Sicherung zurückspielen (nur bekannte Dateinamen). Liefert die Zahl der Dateien.
pub fn backup_restore(name: &str) -> Result<usize, String> {
    backup_restore_in(&crate::sys::data_dir(), &backup_root(), name)
}

fn backup_restore_in(dst: &str, root: &str, name: &str) -> Result<usize, String> {
    if name.contains('/') || name.contains("..") {
        return Err("Name".into());
    }
    let dir = format!("{root}/{name}");
    let mut n = 0;
    for f in BACKUP_FILES {
        let s = format!("{dir}/{f}");
        if std::path::Path::new(&s).exists() {
            let tmp = format!("{dst}/{f}.restore");
            std::fs::copy(&s, &tmp).map_err(|e| e.to_string())?;
            std::fs::rename(&tmp, format!("{dst}/{f}")).map_err(|e| e.to_string())?;
            n += 1;
        }
    }
    Ok(n)
}

/// Werkseinstellungen: alle Nutzerdaten unter /data/ultar löschen außer Gerätewissen
/// (Hardware-Profil, Anzeige-Fall). Medien unter /data (Fotos usw.) bleiben, wenn `keep_media`.
pub fn factory_reset(keep_media: bool) -> usize {
    factory_reset_in(&crate::sys::data_dir(), keep_media)
}

fn factory_reset_in(data: &str, keep_media: bool) -> usize {
    let keep = ["hw-profile.toml", "display.case", "tt"];
    let mut n = 0;
    if let Ok(rd) = std::fs::read_dir(data) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if keep.contains(&name.as_str()) {
                continue;
            }
            let p = e.path();
            let ok = if p.is_dir() {
                std::fs::remove_dir_all(&p).is_ok()
            } else {
                std::fs::remove_file(&p).is_ok()
            };
            n += ok as usize;
        }
    }
    if !keep_media {
        for d in [
            "/data/DCIM",
            "/data/Pictures",
            "/data/media",
            "/data/Documents",
        ] {
            n += std::fs::remove_dir_all(d).is_ok() as usize;
        }
    }
    n
}

fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(m) = std::fs::symlink_metadata(p) else {
        return 0;
    };
    if m.is_dir() {
        std::fs::read_dir(p)
            .map(|rd| rd.flatten().map(|e| dir_size(&e.path())).sum())
            .unwrap_or(0)
    } else {
        m.len()
    }
}

/// Zu bereinigende Pfade (Zwischenspeicher, alte Programmstände, Bildschirmfotos der Tür, temporäre Dateien).
pub fn cleanup_candidates() -> Vec<(String, u64)> {
    let d = crate::sys::data_dir();
    let mut v = Vec::new();
    for p in [
        format!("{d}/cache"),
        format!("{d}/thumbs"),
        format!("{d}/tmp"),
        format!("{d}/screen.png"),
        format!("{d}/shell.gz"),
        format!("{d}/shell2.gz"),
        format!("{d}/net.gz"),
        "/data/tmp".into(),
    ] {
        let s = dir_size(std::path::Path::new(&p));
        if s > 0 {
            v.push((p, s));
        }
    }
    for p in [
        "/usr/bin/ultar-shell.old",
        "/usr/bin/ultar-shell.old2",
        "/usr/bin/ultar-net.old",
    ] {
        if let Ok(m) = std::fs::metadata(p) {
            v.push((p.to_string(), m.len()));
        }
    }
    v
}

pub fn cleanup_run(list: &[(String, u64)]) -> u64 {
    let mut freed = 0;
    for (p, s) in list {
        let path = std::path::Path::new(p);
        let ok = if path.is_dir() {
            std::fs::remove_dir_all(path).is_ok()
        } else {
            std::fs::remove_file(path).is_ok()
        };
        if ok {
            freed += s;
        }
    }
    freed
}

/// Kernel-Protokoll lesen (klogctl, ohne dmesg-Programm). Neueste Zeilen zuletzt.
pub fn kernel_log(max_lines: usize) -> Vec<String> {
    use nix::libc;
    let mut buf = vec![0u8; 256 * 1024];
    let n = unsafe { libc::klogctl(3, buf.as_mut_ptr() as *mut libc::c_char, buf.len() as i32) };
    if n <= 0 {
        return vec![std::io::Error::last_os_error().to_string()];
    }
    let s = String::from_utf8_lossy(&buf[..n as usize]);
    let lines: Vec<String> = s
        .lines()
        .map(|l| {
            l.strip_prefix('<')
                .and_then(|x| x.split_once('>'))
                .map(|(_, r)| r)
                .unwrap_or(l)
                .to_string()
        })
        .collect();
    lines[lines.len().saturating_sub(max_lines)..].to_vec()
}

/// Protokoll der Ultar-Dienste (Ausgaben, die ultar-init sammelt), falls vorhanden.
pub fn service_log(max_lines: usize) -> Vec<String> {
    for p in [
        "/run/ultar/init.log",
        "/run/ultar/log",
        "/data/ultar/init.log",
    ] {
        if let Ok(s) = std::fs::read_to_string(p) {
            let l: Vec<String> = s.lines().map(String::from).collect();
            return l[l.len().saturating_sub(max_lines)..].to_vec();
        }
    }
    Vec::new()
}

/// Neustart in einen besonderen Modus (Samsung: „download" = Odin-Modus, „recovery").
pub fn reboot_to(mode: &str) -> Result<(), String> {
    use nix::libc;
    unsafe { libc::sync() };
    let arg = std::ffi::CString::new(mode).map_err(|e| e.to_string())?;
    let r = unsafe {
        libc::syscall(
            libc::SYS_reboot,
            libc::LINUX_REBOOT_MAGIC1,
            libc::LINUX_REBOOT_MAGIC2,
            libc::LINUX_REBOOT_CMD_RESTART2,
            arg.as_ptr(),
        )
    };
    if r < 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}

/// Bild als PNG speichern (Bildschirmfoto). Pixel 0x00RRGGBB.
pub fn save_png(path: &str, w: u32, h: u32, px: &[u32]) -> Result<(), String> {
    let mut rgb = Vec::with_capacity((w * h * 3) as usize);
    for p in px {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, *p as u8]);
    }
    if let Some(dir) = std::path::Path::new(path).parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    image::save_buffer(path, &rgb, w, h, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ut(y: i64, mo: i64, d: i64, h: i64, mi: i64) -> i64 {
        crate::sys::to_unix(
            &crate::sys::DateTime {
                y,
                mo,
                d,
                h,
                mi,
                s: 0,
                wd: 0,
            },
            0,
        )
    }

    #[test]
    fn dst_rules() {
        // EU 2026: 29.03. 01:00 UTC bis 25.10. 01:00 UTC
        assert_eq!(offset_at("Europe/Berlin", ut(2026, 3, 29, 0, 59)), Some(60));
        assert_eq!(offset_at("Europe/Berlin", ut(2026, 3, 29, 1, 0)), Some(120));
        assert_eq!(
            offset_at("Europe/Berlin", ut(2026, 10, 25, 0, 59)),
            Some(120)
        );
        assert_eq!(offset_at("Europe/Berlin", ut(2026, 10, 25, 1, 0)), Some(60));
        assert_eq!(
            offset_at("Europe/Berlin", ut(2026, 11, 19, 12, 0)),
            Some(60)
        );
        // USA 2026: 08.03. 02:00 EST (07:00 UTC) bis 01.11. 02:00 EDT (06:00 UTC)
        assert_eq!(
            offset_at("America/New_York", ut(2026, 3, 8, 6, 59)),
            Some(-300)
        );
        assert_eq!(
            offset_at("America/New_York", ut(2026, 3, 8, 7, 0)),
            Some(-240)
        );
        assert_eq!(
            offset_at("America/New_York", ut(2026, 11, 1, 5, 59)),
            Some(-240)
        );
        assert_eq!(
            offset_at("America/New_York", ut(2026, 11, 1, 6, 0)),
            Some(-300)
        );
        // Damaskus ganzjährig UTC+3
        assert_eq!(offset_at("Asia/Damascus", ut(2026, 7, 1, 0, 0)), Some(180));
        assert_eq!(offset_at("Asia/Damascus", ut(2026, 1, 1, 0, 0)), Some(180));
        // Kairo 2026: Sommerzeit ab letztem Fr im April (24.04.)
        assert_eq!(offset_at("Africa/Cairo", ut(2026, 6, 1, 0, 0)), Some(180));
        assert_eq!(offset_at("Africa/Cairo", ut(2026, 12, 1, 0, 0)), Some(120));
        assert_eq!(fmt_offset(330), "UTC+5:30");
        assert_eq!(fmt_offset(-300), "UTC-5");
    }

    #[test]
    fn clock_formats() {
        assert_eq!(fmt_clock(0, 5, true), "12:05 AM");
        assert_eq!(fmt_clock(13, 30, true), "1:30 PM");
        assert_eq!(fmt_clock(9, 7, false), "09:07");
        assert!(in_window(23 * 60, 22 * 60, 7 * 60));
        assert!(in_window(6 * 60, 22 * 60, 7 * 60));
        assert!(!in_window(12 * 60, 22 * 60, 7 * 60));
        assert!(in_window(10 * 60, 9 * 60, 17 * 60));
    }

    #[test]
    fn sntp_roundtrip() {
        let mut b = [0u8; 48];
        b[0] = 0b00_100_100;
        b[1] = 2;
        let secs = (1_795_000_000i64 + NTP_EPOCH) as u32;
        b[40..44].copy_from_slice(&secs.to_be_bytes());
        b[44..48].copy_from_slice(&(1u32 << 31).to_be_bytes());
        assert_eq!(sntp_parse(&b), Some(1_795_000_000_500));
        b[1] = 0;
        assert_eq!(sntp_parse(&b), None);
        assert_eq!(sntp_request()[0], 0x23);
        // DNS: Anfrage + Antwort mit Namenskompression
        let q = dns_query(0x1234, "code-craft.tech");
        let mut r = q.clone();
        r[2] = 0x81;
        r[3] = 0x80;
        r[7] = 1;
        r.extend_from_slice(&[0xC0, 0x0C, 0, 1, 0, 1, 0, 0, 0, 60, 0, 4, 93, 184, 216, 34]);
        assert_eq!(
            dns_parse(&r, 0x1234),
            Some(std::net::Ipv4Addr::new(93, 184, 216, 34))
        );
        assert_eq!(dns_parse(&r, 0x9999), None);
        assert_eq!(
            parse_http_date("Thu, 19 Nov 2026 19:48:14 GMT"),
            Some(ut(2026, 11, 19, 19, 48) + 14)
        );
    }

    #[test]
    fn sha_and_pin() {
        assert_eq!(
            hex(&sha256(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&sha256(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let s = "00112233";
        assert_eq!(pin_hash("1234", s), pin_hash("1234", s));
        assert_ne!(pin_hash("1234", s), pin_hash("1235", s));
        assert!(pin_valid("1234") && !pin_valid("12a4") && !pin_valid("123"));
        assert_eq!(lockout_secs(4), 0);
        assert_eq!(lockout_secs(5), 30);
        assert_eq!(lockout_secs(6), 60);
        assert_eq!(lockout_secs(20), 900);
    }

    #[test]
    fn units_calendar_search() {
        assert!((convert(0, 3, 7, 1.609344) - 1.0).abs() < 1e-9);
        assert!((convert(2, 0, 1, 100.0) - 212.0).abs() < 1e-9);
        assert!((convert(2, 1, 2, 32.0) - 273.15).abs() < 1e-9);
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(1000.0), "1000");
        // November 2026 beginnt an einem Sonntag (Montag=0 -> 6), 30 Tage
        assert_eq!(month_grid(2026, 11), (6, 30));
        assert_eq!(month_grid(2026, 2), (6, 28));
        let mut cal = Calendar::default();
        cal.events.push(Event {
            y: 2026,
            mo: 11,
            d: 20,
            at: 9 * 60,
            title: "Termin".into(),
            remind: 15,
            notified: false,
        });
        let start = cal.events[0].start_unix(60);
        assert!(due_reminders(&cal, start - 16 * 60, 60).is_empty());
        assert_eq!(due_reminders(&cal, start - 10 * 60, 60), vec![0]);
        assert!(score("set", "Settings").unwrap() > score("set", "Reset").unwrap_or(0));
        assert!(score("Einst", "Einstellungen").is_some());
        assert!(score("ملاح", "الملاحظات").is_some());
        assert!(score("xyz", "Kamera").is_none());
        assert!(score("gal", "Galerie") > score("gal", "Wallpaper gal"));
    }

    #[test]
    fn battery_and_display() {
        use BattEvent::*;
        assert_eq!(
            battery_events(Some((16, false)), (15, false), true, 15, false),
            vec![Low(15), SaverOn]
        );
        assert_eq!(
            battery_events(Some((6, false)), (5, false), true, 15, true),
            vec![Low(5)]
        );
        assert_eq!(
            battery_events(Some((40, false)), (40, true), true, 15, true),
            vec![ChargingStarted, SaverOff]
        );
        assert_eq!(
            battery_events(Some((99, true)), (100, true), false, 15, false),
            vec![Full]
        );
        assert!(battery_events(None, (5, false), true, 15, false).is_empty());
        let mut px = [0x00FF_FFFFu32, 0x0000_00FF];
        night_tint(&mut px, 1.0);
        assert_eq!(px[0] >> 16, 0xFF);
        assert!((px[0] & 0xFF) < 0x60 && ((px[0] >> 8) & 0xFF) > 0xB0);
        assert!(
            lux_to_brightness(0.5) < lux_to_brightness(100.0) && lux_to_brightness(100000.0) == 1.0
        );
        let av = vec![
            "schedutil".to_string(),
            "powersave".to_string(),
            "performance".into(),
        ];
        assert_eq!(governor_for("power", &av).as_deref(), Some("powersave"));
        assert_eq!(governor_for("balanced", &av).as_deref(), Some("schedutil"));
        let mut d = std::collections::BTreeMap::new();
        d.insert("2026-11-01".to_string(), (100u64, 50u64));
        d.insert("2026-10-31".to_string(), (999u64, 1u64));
        assert_eq!(month_usage(&d, 2026, 11), 150);
    }

    #[test]
    fn backup_roundtrip() {
        let base = std::env::temp_dir().join(format!("ultar-bk-{}", std::process::id()));
        let data = base.join("data");
        std::fs::create_dir_all(&data).unwrap();
        let (ds, bk) = (
            data.to_str().unwrap().to_string(),
            base.join("bk").to_str().unwrap().to_string(),
        );
        std::fs::write(data.join("notes.toml"), "a=1").unwrap();
        std::fs::write(data.join("wifi.conf"), "geheim").unwrap();
        let (dir, n) = backup_create_in(&ds, &bk, "20261119-200000").unwrap();
        assert_eq!(n, 1);
        assert!(!std::path::Path::new(&dir).join("wifi.conf").exists());
        std::fs::write(data.join("notes.toml"), "a=2").unwrap();
        assert_eq!(
            backup_list_in(&bk),
            vec!["ultar-20261119-200000".to_string()]
        );
        assert_eq!(
            backup_restore_in(&ds, &bk, "ultar-20261119-200000").unwrap(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(data.join("notes.toml")).unwrap(),
            "a=1"
        );
        assert!(backup_restore_in(&ds, &bk, "../x").is_err());
        std::fs::write(data.join("hw-profile.toml"), "x").unwrap();
        factory_reset_in(&ds, true);
        assert!(data.join("hw-profile.toml").exists() && !data.join("notes.toml").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
}
