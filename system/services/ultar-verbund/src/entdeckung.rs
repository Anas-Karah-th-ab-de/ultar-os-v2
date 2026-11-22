//! Geräte im selben Netz finden: UDP-Rundruf auf Port 47800 alle 2 s.
//! Die Meldung ist öffentlich (Kennung, Schlüssel, Name, Port, freie Kerne) – vertraut wird erst nach Noise + Kopplung.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Meldung {
    pub v: u8,
    pub k: String,
    pub pk: String,
    pub n: String,
    pub a: String,
    pub p: u16,
    pub kerne: usize,
    pub frei: usize,
    #[serde(default)]
    pub aktiv: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gesehen {
    pub kennung: String,
    pub pk: String,
    pub name: String,
    pub ausgabe: String,
    pub adresse: String,
    pub kerne: usize,
    pub frei: usize,
    pub zuletzt_ms: u64,
    pub vertraut: bool,
    /// "netz" (direkt) oder "relais"
    pub weg: String,
}

pub type Tabelle = Arc<Mutex<HashMap<String, Gesehen>>>;

pub fn socket(port: u16) -> std::io::Result<UdpSocket> {
    let s = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::DGRAM,
        Some(socket2::Protocol::UDP),
    )?;
    s.set_reuse_address(true)?;
    #[cfg(unix)]
    let _ = s.set_reuse_port(true);
    s.set_broadcast(true)?;
    s.bind(&SocketAddr::from(([0, 0, 0, 0], port)).into())?;
    let u: UdpSocket = s.into();
    u.set_read_timeout(Some(Duration::from_millis(500)))?;
    Ok(u)
}

/// Rundruf-Ziele: 255.255.255.255 + Broadcast jeder Schnittstelle (aus /proc/net/route nicht ableitbar → feste Liste) + Extra-Ziele
pub fn ziele(port: u16, extra: &[String]) -> Vec<SocketAddr> {
    let mut v: Vec<SocketAddr> = vec![SocketAddr::from(([255, 255, 255, 255], port))];
    let mut env: Vec<String> = std::env::var("ULTAR_VERBUND_ZIELE")
        .map(|s| s.split(',').map(|x| x.trim().to_string()).collect())
        .unwrap_or_default();
    env.extend(extra.iter().cloned());
    for z in env {
        if z.is_empty() {
            continue;
        }
        let z = if z.contains(':') {
            z
        } else {
            format!("{z}:{port}")
        };
        if let Ok(a) = z.parse() {
            v.push(a);
        }
    }
    v
}

pub fn senden(u: &UdpSocket, m: &Meldung, ziele: &[SocketAddr]) {
    let b = serde_json::to_vec(m).unwrap();
    for z in ziele {
        let _ = u.send_to(&b, z);
    }
}

/// eine empfangene Meldung verarbeiten (eigene ignorieren)
pub fn verarbeiten(
    t: &Tabelle,
    eigen_pk: &str,
    b: &[u8],
    von: SocketAddr,
    vertraut: impl Fn(&str) -> bool,
) -> Option<Gesehen> {
    let m: Meldung = serde_json::from_slice(b).ok()?;
    if m.v != 1 || m.pk == eigen_pk || m.pk.len() != 64 || !m.aktiv {
        return None;
    }
    // Kennung muss zum Schlüssel passen
    if crate::ident::kennung(&crate::ident::unhex(&m.pk)?) != m.k {
        return None;
    }
    let g = Gesehen {
        kennung: m.k.clone(),
        pk: m.pk.clone(),
        name: m.n.chars().take(64).collect(),
        ausgabe: m.a.chars().take(16).collect(),
        adresse: SocketAddr::new(von.ip(), m.p).to_string(),
        kerne: m.kerne.min(4096),
        frei: m.frei.min(4096),
        zuletzt_ms: crate::jetzt_ms(),
        vertraut: vertraut(&m.pk),
        weg: "netz".into(),
    };
    t.lock().unwrap().insert(m.pk, g.clone());
    Some(g)
}

/// veraltete Einträge (> 10 s) entfernen
pub fn aufraeumen(t: &Tabelle) {
    let jetzt = crate::jetzt_ms();
    t.lock()
        .unwrap()
        .retain(|_, g| g.weg == "relais" || jetzt.saturating_sub(g.zuletzt_ms) < 10_000);
}
