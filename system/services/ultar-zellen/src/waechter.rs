//! Protokoll: was eine Zelle (oder das ganze System) ins Netz schickt.
//!
//! In der Zelle leitet nftables jede TCP-Verbindung und jede DNS-Frage der App an den Wächter um (eigener
//! Prozess im Netz der Zelle, als root – sein eigener Verkehr wird nicht umgeleitet). Der Wächter verbindet
//! weiter zum eigentlichen Ziel und hält fest: Zeit, Ziel, Name (TLS-SNI, HTTP-Host, DNS), Bytes hoch/runter,
//! Dauer – bei `uploads = true` zusätzlich den Anfang unverschlüsselter HTTP-Anfragen. Andere UDP-Ziele
//! (z. B. QUIC) sperrt er, damit nichts am Protokoll vorbeigeht; Browser weichen dann auf TCP aus.
//! Verschlüsselte Inhalte (HTTPS) sieht niemand – auch der Wächter nicht; sichtbar sind Ziel, Name und Menge.
//!
//! Systemweit: `zelle systemprotokoll` notiert jede neue Verbindung jedes Prozesses (Programm, Ziel) aus /proc.

use crate::profil::{ordner, Profil};
use serde::Serialize;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TCP_PORT: u16 = 15001;
pub const DNS_PORT: u16 = 15053;

/// Umleitungsregeln im Netz der Zelle (nur der Verkehr der App, nicht der des Wächters)
pub fn regeln_setzen(p: &Profil) -> Result<(), String> {
    let uid = p.uid();
    let regel = format!(
        "table ip ultar_waechter {{\n chain umleiten {{ type nat hook output priority -100;\n  meta skuid {uid} ip daddr != 127.0.0.0/8 tcp dport 1-65535 redirect to :{TCP_PORT}\n  meta skuid {uid} udp dport 53 redirect to :{DNS_PORT}\n }}\n chain sperre {{ type filter hook output priority 0;\n  meta skuid {uid} ip daddr != 127.0.0.0/8 udp dport != 53 drop\n }}\n}}\n"
    );
    let ns = crate::netz::ns(&p.name);
    let _ = crate::befehl(
        "ip",
        &[
            "netns",
            "exec",
            &ns,
            "nft",
            "delete",
            "table",
            "ip",
            "ultar_waechter",
        ],
    );
    let mut k = std::process::Command::new("ip")
        .args(["netns", "exec", &ns, "nft", "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    k.stdin
        .take()
        .unwrap()
        .write_all(regel.as_bytes())
        .map_err(|e| e.to_string())?;
    let o = k.wait_with_output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!(
            "Wächter-Regeln: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        ));
    }
    Ok(())
}

#[derive(Serialize, Debug, Clone, Default)]
pub struct Eintrag {
    pub zeit: u64,
    pub zelle: String,
    pub art: String,
    pub ziel: String,
    pub name: String,
    pub hoch: u64,
    pub runter: u64,
    pub ms: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub upload: String,
}

struct Log {
    datei: Mutex<std::fs::File>,
}

impl Log {
    fn schreiben(&self, e: &Eintrag) {
        if let Ok(mut f) = self.datei.lock() {
            let _ = writeln!(f, "{}", serde_json::to_string(e).unwrap());
        }
    }
}

/// TLS-ClientHello: Servername (SNI) herauslesen
pub fn sni(b: &[u8]) -> Option<String> {
    if b.len() < 43 || b[0] != 0x16 || b[5] != 0x01 {
        return None;
    }
    let mut p = 5 + 4 + 2 + 32; // Satzkopf, Handshake-Kopf, Version, Zufall
    let sid = *b.get(p)? as usize;
    p += 1 + sid;
    let cs = u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]) as usize;
    p += 2 + cs;
    let cm = *b.get(p)? as usize;
    p += 1 + cm;
    let ext_ende = p + 2 + u16::from_be_bytes([*b.get(p)?, *b.get(p + 1)?]) as usize;
    p += 2;
    while p + 4 <= ext_ende.min(b.len()) {
        let typ = u16::from_be_bytes([b[p], b[p + 1]]);
        let len = u16::from_be_bytes([b[p + 2], b[p + 3]]) as usize;
        p += 4;
        if typ == 0 {
            // server_name: Listenlänge(2) Typ(1) Namenslänge(2) Name
            let n = u16::from_be_bytes([*b.get(p + 3)?, *b.get(p + 4)?]) as usize;
            return std::str::from_utf8(b.get(p + 5..p + 5 + n)?)
                .ok()
                .map(String::from);
        }
        p += len;
    }
    None
}

/// HTTP: Host-Zeile
pub fn http_host(b: &[u8]) -> Option<String> {
    let s = std::str::from_utf8(&b[..b.len().min(4096)]).ok()?;
    let erste = s.lines().next()?;
    if ![
        "GET ", "POST ", "PUT ", "PATCH ", "DELETE ", "HEAD ", "OPTIONS ",
    ]
    .iter()
    .any(|m| erste.starts_with(m))
    {
        return None;
    }
    s.lines().find_map(|l| {
        l.strip_prefix("Host:")
            .or_else(|| l.strip_prefix("host:"))
            .map(|h| h.trim().to_string())
    })
}

fn ursprungsziel(s: &TcpStream) -> Option<SocketAddrV4> {
    use std::os::fd::AsRawFd;
    let mut a: libc::sockaddr_in = unsafe { std::mem::zeroed() };
    let mut l = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
    // SO_ORIGINAL_DST = 80
    let r = unsafe {
        libc::getsockopt(
            s.as_raw_fd(),
            libc::SOL_IP,
            80,
            &mut a as *mut _ as *mut libc::c_void,
            &mut l,
        )
    };
    if r != 0 {
        return None;
    }
    Some(SocketAddrV4::new(
        Ipv4Addr::from(u32::from_be(a.sin_addr.s_addr)),
        u16::from_be(a.sin_port),
    ))
}

fn tcp_verbindung(c: TcpStream, log: Arc<Log>, p: Arc<Profil>) {
    let t = Instant::now();
    let Some(ziel) = ursprungsziel(&c) else {
        return;
    };
    let _ = c.set_read_timeout(Some(Duration::from_millis(1500)));
    // ersten Block lesen (für Name/Upload), dann weiterreichen
    let mut erst = vec![0u8; 16384];
    let n = (&c).read(&mut erst).unwrap_or(0);
    erst.truncate(n);
    let _ = c.set_read_timeout(None);
    let name = sni(&erst).or_else(|| http_host(&erst)).unwrap_or_default();
    let upload = if p.protokoll.uploads && http_host(&erst).is_some() {
        String::from_utf8_lossy(&erst[..erst.len().min(512)]).into_owned()
    } else {
        String::new()
    };
    let mut e = Eintrag {
        zeit: crate::jetzt(),
        zelle: p.name.clone(),
        art: "tcp".into(),
        ziel: ziel.to_string(),
        name,
        upload,
        ..Default::default()
    };
    let s = match TcpStream::connect_timeout(&SocketAddr::V4(ziel), Duration::from_secs(10)) {
        Ok(s) => s,
        Err(err) => {
            e.art = format!("tcp-fehler: {err}");
            log.schreiben(&e);
            return;
        }
    };
    let hoch = Arc::new(AtomicU64::new(0));
    let runter = Arc::new(AtomicU64::new(0));
    let (mut c1, mut s1) = (c.try_clone().unwrap(), s.try_clone().unwrap());
    let h2 = hoch.clone();
    let a = std::thread::spawn(move || {
        if !erst.is_empty() && s1.write_all(&erst).is_ok() {
            h2.fetch_add(erst.len() as u64, Ordering::Relaxed);
        }
        let mut b = [0u8; 16384];
        while let Ok(n) = c1.read(&mut b) {
            if n == 0 || s1.write_all(&b[..n]).is_err() {
                break;
            }
            h2.fetch_add(n as u64, Ordering::Relaxed);
        }
        let _ = s1.shutdown(std::net::Shutdown::Write);
    });
    let (mut s2, mut c2) = (s, c);
    let mut b = [0u8; 16384];
    while let Ok(n) = s2.read(&mut b) {
        if n == 0 || c2.write_all(&b[..n]).is_err() {
            break;
        }
        runter.fetch_add(n as u64, Ordering::Relaxed);
    }
    let _ = c2.shutdown(std::net::Shutdown::Write);
    let _ = a.join();
    e.hoch = hoch.load(Ordering::Relaxed);
    e.runter = runter.load(Ordering::Relaxed);
    e.ms = t.elapsed().as_millis() as u64;
    log.schreiben(&e);
}

/// Name aus einer DNS-Frage
pub fn dns_name(q: &[u8]) -> Option<String> {
    let mut p = 12;
    let mut teile = Vec::new();
    loop {
        let l = *q.get(p)? as usize;
        if l == 0 {
            break;
        }
        if l > 63 {
            return None;
        }
        teile.push(String::from_utf8_lossy(q.get(p + 1..p + 1 + l)?).into_owned());
        p += 1 + l;
        if teile.len() > 64 {
            return None;
        }
    }
    Some(teile.join("."))
}

fn dns_dienst(log: Arc<Log>, p: Arc<Profil>) {
    let Ok(s) = UdpSocket::bind(("127.0.0.1", DNS_PORT)) else {
        return;
    };
    let ober: Vec<String> = crate::netz::dns();
    let mut b = [0u8; 1500];
    while let Ok((n, von)) = s.recv_from(&mut b) {
        let q = b[..n].to_vec();
        let (s2, log, p, ober) = (s.try_clone().unwrap(), log.clone(), p.clone(), ober.clone());
        std::thread::spawn(move || {
            let name = dns_name(&q).unwrap_or_default();
            let u = UdpSocket::bind(("0.0.0.0", 0)).ok();
            let mut antwort = None;
            if let Some(u) = &u {
                let _ = u.set_read_timeout(Some(Duration::from_secs(3)));
                for o in &ober {
                    if u.send_to(&q, (o.as_str(), 53)).is_ok() {
                        let mut r = [0u8; 1500];
                        if let Ok((m, _)) = u.recv_from(&mut r) {
                            antwort = Some(r[..m].to_vec());
                            break;
                        }
                    }
                }
            }
            if let Some(a) = &antwort {
                let _ = s2.send_to(a, von);
            }
            log.schreiben(&Eintrag {
                zeit: crate::jetzt(),
                zelle: p.name.clone(),
                art: "dns".into(),
                name,
                ziel: ober.first().cloned().unwrap_or_default(),
                hoch: q.len() as u64,
                runter: antwort.map(|a| a.len() as u64).unwrap_or(0),
                ..Default::default()
            });
        });
    }
}

/// Wächter einer Zelle (läuft im Netz der Zelle)
pub fn laufen(name: &str) -> Result<(), String> {
    let p = Arc::new(Profil::laden(name)?);
    let datei = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(ordner(name).join("protokoll.jsonl"))
        .map_err(|e| e.to_string())?;
    let log = Arc::new(Log {
        datei: Mutex::new(datei),
    });
    let l = TcpListener::bind(("0.0.0.0", TCP_PORT)).map_err(|e| format!("Wächter: {e}"))?;
    {
        let (log, p) = (log.clone(), p.clone());
        std::thread::spawn(move || dns_dienst(log, p));
    }
    for c in l.incoming().flatten() {
        let (log, p) = (log.clone(), p.clone());
        std::thread::spawn(move || tcp_verbindung(c, log, p));
    }
    Ok(())
}

/// systemweit: neue Verbindungen aller Prozesse (aus /proc/net/tcp und den Dateideskriptoren)
pub fn system(intervall: Duration, runden: Option<u64>) -> Result<(), String> {
    let pfad = crate::wurzel().join("system-protokoll.jsonl");
    std::fs::create_dir_all(crate::wurzel()).map_err(|e| e.to_string())?;
    let log = Log {
        datei: Mutex::new(
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&pfad)
                .map_err(|e| e.to_string())?,
        ),
    };
    let mut gesehen = std::collections::HashSet::new();
    let mut r = 0;
    loop {
        // inode → Prozess
        let mut wem = std::collections::HashMap::new();
        for e in std::fs::read_dir("/proc")
            .map(|d| d.flatten().collect::<Vec<_>>())
            .unwrap_or_default()
        {
            let pid = e.file_name().to_string_lossy().into_owned();
            if !pid.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let prog = std::fs::read_to_string(e.path().join("comm"))
                .unwrap_or_default()
                .trim()
                .to_string();
            for fd in std::fs::read_dir(e.path().join("fd"))
                .map(|d| d.flatten().collect::<Vec<_>>())
                .unwrap_or_default()
            {
                if let Ok(z) = std::fs::read_link(fd.path()) {
                    if let Some(i) = z
                        .to_string_lossy()
                        .strip_prefix("socket:[")
                        .and_then(|x| x.strip_suffix(']'))
                    {
                        wem.insert(i.to_string(), format!("{prog} ({pid})"));
                    }
                }
            }
        }
        for (datei, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
            for z in std::fs::read_to_string(datei)
                .unwrap_or_default()
                .lines()
                .skip(1)
            {
                let f: Vec<&str> = z.split_whitespace().collect();
                if f.len() < 10 || f[3] != "01" {
                    continue; // nur bestehende Verbindungen
                }
                let ziel = adresse(f[2], v6);
                if ziel.starts_with("127.")
                    || ziel.starts_with("[::1]")
                    || ziel.starts_with("[::ffff:127.")
                {
                    continue;
                }
                let schluessel = format!("{}-{}", f[9], ziel);
                if gesehen.insert(schluessel) {
                    log.schreiben(&Eintrag {
                        zeit: crate::jetzt(),
                        zelle: wem.get(f[9]).cloned().unwrap_or_else(|| "?".into()),
                        art: "system".into(),
                        ziel,
                        ..Default::default()
                    });
                }
            }
        }
        r += 1;
        if runden.is_some_and(|n| r >= n) {
            return Ok(());
        }
        std::thread::sleep(intervall);
    }
}

fn adresse(hex: &str, v6: bool) -> String {
    let (a, p) = hex.split_once(':').unwrap_or((hex, "0"));
    let port = u16::from_str_radix(p, 16).unwrap_or(0);
    if !v6 {
        let n = u32::from_str_radix(a, 16).unwrap_or(0);
        return format!("{}:{port}", Ipv4Addr::from(n.swap_bytes()));
    }
    let mut b = [0u8; 16];
    for i in 0..4 {
        let w = u32::from_str_radix(a.get(i * 8..i * 8 + 8).unwrap_or("0"), 16)
            .unwrap_or(0)
            .swap_bytes();
        b[i * 4..i * 4 + 4].copy_from_slice(&w.to_be_bytes());
    }
    format!("[{}]:{port}", std::net::Ipv6Addr::from(b))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namen() {
        assert_eq!(
            http_host(b"POST /x HTTP/1.1\r\nHost: daten.example\r\n\r\nabc").as_deref(),
            Some("daten.example")
        );
        assert_eq!(http_host(b"\x16\x03\x01"), None);
        // DNS-Frage für www.example.org
        let mut q = vec![0u8; 12];
        for t in ["www", "example", "org"] {
            q.push(t.len() as u8);
            q.extend(t.as_bytes());
        }
        q.extend([0, 0, 1, 0, 1]);
        assert_eq!(dns_name(&q).as_deref(), Some("www.example.org"));
        assert_eq!(adresse("0100007F:0050", false), "127.0.0.1:80");
        // minimaler ClientHello mit SNI „ultar.code-craft.tech“
        let name = b"ultar.code-craft.tech";
        let mut daten = ((name.len() + 3) as u16).to_be_bytes().to_vec(); // Listenlänge
        daten.push(0); // Typ host_name
        daten.extend((name.len() as u16).to_be_bytes());
        daten.extend(name);
        let mut sn = vec![0, 0]; // Erweiterung server_name
        sn.extend((daten.len() as u16).to_be_bytes());
        sn.extend(daten);
        let ext = [(sn.len() as u16).to_be_bytes().to_vec(), sn].concat();
        let mut hello = vec![0x03, 0x03];
        hello.extend([0u8; 32]);
        hello.extend([0, 0, 2, 0x13, 0x01, 1, 0]);
        hello.extend(ext);
        let mut hs = vec![0x01, 0];
        hs.extend(((hello.len()) as u16).to_be_bytes());
        hs.extend(hello);
        let mut rec = vec![0x16, 0x03, 0x01];
        rec.extend((hs.len() as u16).to_be_bytes());
        rec.extend(hs);
        assert_eq!(sni(&rec).as_deref(), Some("ultar.code-craft.tech"));
    }
}
