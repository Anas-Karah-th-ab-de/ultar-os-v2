//! Brandmauer im Kern: von außen ist **nichts** erreichbar, solange es nicht ausdrücklich geöffnet ist.
//!
//! * Eingehende neue TCP-Verbindungen und UDP-Pakete von fremden Adressen werden still verworfen
//!   (kein RST, keine ICMP-Antwort – ein Scanner sieht nur „gefiltert“), außer
//!   – der Port ist geöffnet (`/proc/ultar/firewall`: `offen 22`, `zu 22`; beim Start aus `/etc/ultar/firewall`),
//!   – es ist die Antwort auf eine eigene Verbindung (bestehende TCP-Verbindung, UDP-Gegenstelle, an die gesendet wurde),
//!   – es sind die Verbund-Ports und der Verbund ist AN.
//! * Ping-Antworten nur, wenn `ping an`.
//! * Loopback (127.0.0.1) ist immer erlaubt.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::Mutex;

static OFFEN: Mutex<Vec<(u8, u16)>> = Mutex::new(Vec::new());
pub static PING: AtomicBool = AtomicBool::new(false);
pub static VERWORFEN: AtomicU64 = AtomicU64::new(0);

fn proto_name(p: u8) -> &'static str {
    if p == 6 {
        "tcp"
    } else {
        "udp"
    }
}

/// darf eine NEUE eingehende Verbindung / ein Paket an diesen lokalen Port?
pub fn eingehend_erlaubt(proto: u8, src: [u8; 4], dport: u16) -> bool {
    if src[0] == 127 {
        return true;
    }
    if crate::verbund::aktiv()
        && ((proto == 17 && dport == crate::verbund::UDP_PORT)
            || (proto == 6 && dport == crate::verbund::TCP_PORT))
    {
        return true;
    }
    if OFFEN
        .lock()
        .iter()
        .any(|&(p, port)| p == proto && port == dport)
    {
        return true;
    }
    VERWORFEN.fetch_add(1, Ordering::Relaxed);
    false
}

pub fn verworfen() {
    VERWORFEN.fetch_add(1, Ordering::Relaxed);
}

/// Befehl ausführen: „offen 22“, „offen udp 53“, „zu 22“, „ping an|aus“
pub fn befehl(z: &str) -> Result<String, String> {
    let w: Vec<&str> = z.split_whitespace().collect();
    let port = |s: &str| {
        s.parse::<u16>()
            .ok()
            .filter(|p| *p != 0)
            .ok_or_else(|| format!("Port? {s}"))
    };
    match w.as_slice() {
        ["ping", x] => {
            PING.store(*x == "an", Ordering::Relaxed);
            Ok(format!("ping {x}"))
        }
        ["offen", p] | ["offen", "tcp", p] => oeffnen(6, port(p)?),
        ["offen", "udp", p] => oeffnen(17, port(p)?),
        ["zu", p] | ["zu", "tcp", p] => schliessen(6, port(p)?),
        ["zu", "udp", p] => schliessen(17, port(p)?),
        [] => Ok(text()),
        _ => Err(String::from(
            "offen [tcp|udp] <port> | zu [tcp|udp] <port> | ping an|aus",
        )),
    }
}

fn oeffnen(p: u8, port: u16) -> Result<String, String> {
    let mut o = OFFEN.lock();
    if !o.contains(&(p, port)) {
        if o.len() >= 64 {
            return Err(String::from("zu viele offene Ports"));
        }
        o.push((p, port));
    }
    crate::kprintln!("[firewall] {} {port} offen", proto_name(p));
    Ok(format!("{} {port} offen", proto_name(p)))
}

fn schliessen(p: u8, port: u16) -> Result<String, String> {
    OFFEN.lock().retain(|&x| x != (p, port));
    Ok(format!("{} {port} zu", proto_name(p)))
}

pub fn text() -> String {
    let mut s = String::from("standard: alles zu\n");
    for &(p, port) in OFFEN.lock().iter() {
        s += &format!("offen {} {port}\n", proto_name(p));
    }
    if crate::verbund::aktiv() {
        s += &format!(
            "offen verbund udp {} tcp {}\n",
            crate::verbund::UDP_PORT,
            crate::verbund::TCP_PORT
        );
    }
    s += &format!(
        "ping {}\nverworfen {}\n",
        if PING.load(Ordering::Relaxed) {
            "an"
        } else {
            "aus"
        },
        VERWORFEN.load(Ordering::Relaxed)
    );
    s
}

/// /etc/ultar/firewall beim Start lesen (eine Regel je Zeile, # Kommentar)
pub fn laden() {
    if let Some(crate::fs::Node::File(d)) = crate::fs::open("/etc/ultar/firewall") {
        for z in core::str::from_utf8(&d).unwrap_or("").lines() {
            let z = z.split('#').next().unwrap_or("").trim();
            if !z.is_empty() {
                let _ = befehl(z);
            }
        }
    }
}
