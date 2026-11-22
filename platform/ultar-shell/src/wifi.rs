//! WLAN über die Steuerschnittstelle von wpa_supplicant (/run/wpa_supplicant/<schnittstelle>).
//!
//! Suchen, Verbinden, gespeicherte Netze wählen und Vergessen laufen direkt über wpa_supplicant,
//! das `ultar-net` dauerhaft betreibt – ohne den Netzdienst neu zu starten. Mehrere Netze bleiben
//! gespeichert (SAVE_CONFIG in /data/ultar/wifi.conf). Fehlt die Schnittstelle (älteres Abbild,
//! Ubuntu-Bereich), fällt `sys` auf `iw` und das Schreiben der Konfigurationsdatei zurück.

use std::os::unix::net::UnixDatagram;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

pub const CTRL_DIR: &str = "/run/wpa_supplicant";
static SEQ: AtomicU32 = AtomicU32::new(0);

/// Gefundenes Netz: Name, Signal (dBm), verschlüsselt.
pub type Net = (String, i32, bool);

pub struct Ctrl {
    s: UnixDatagram,
    local: String,
}

impl Ctrl {
    pub fn open(iface: &str) -> Option<Ctrl> {
        let dst = format!("{CTRL_DIR}/{iface}");
        if !std::path::Path::new(&dst).exists() {
            return None;
        }
        let _ = std::fs::create_dir_all("/run/ultar");
        let local = format!("/run/ultar/wpa-{}-{}", std::process::id(), SEQ.fetch_add(1, Ordering::Relaxed));
        let _ = std::fs::remove_file(&local);
        let s = UnixDatagram::bind(&local).ok()?;
        let c = Ctrl { s, local };
        c.s.connect(&dst).ok()?;
        c.s.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
        Some(c)
    }

    /// Befehl senden und Antwort lesen (Ereignismeldungen „<n>…“ werden übersprungen).
    pub fn cmd(&self, c: &str) -> Option<String> {
        self.s.send(c.as_bytes()).ok()?;
        let mut buf = vec![0u8; 32 * 1024];
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end {
            let n = self.s.recv(&mut buf).ok()?;
            let r = String::from_utf8_lossy(&buf[..n]).into_owned();
            if !r.starts_with('<') {
                return Some(r);
            }
        }
        None
    }

    fn ok(&self, c: &str) -> bool {
        self.cmd(c).map(|r| r.trim() == "OK").unwrap_or(false)
    }

    /// Nach ATTACH auf ein Ereignis warten, das `pred` erfüllt.
    fn wait_event(&self, pred: impl Fn(&str) -> bool, timeout: Duration) -> Option<String> {
        let end = Instant::now() + timeout;
        let mut buf = vec![0u8; 4096];
        loop {
            let left = end.checked_duration_since(Instant::now())?;
            self.s.set_read_timeout(Some(left.max(Duration::from_millis(20)))).ok()?;
            let n = self.s.recv(&mut buf).ok()?;
            let r = String::from_utf8_lossy(&buf[..n]);
            if let Some(ev) = r.strip_prefix('<').and_then(|x| x.split_once('>')).map(|(_, e)| e) {
                if pred(ev) {
                    return Some(ev.to_string());
                }
            }
        }
    }
}

impl Drop for Ctrl {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.local);
    }
}

pub fn available(iface: &str) -> bool {
    std::path::Path::new(&format!("{CTRL_DIR}/{iface}")).exists()
}

/// Von wpa_supplicant maskierte Zeichen (\xNN, \\, \", \n …) zurückwandeln.
pub fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            match b[i + 1] {
                b'x' if i + 3 < b.len() => {
                    if let Ok(v) = u8::from_str_radix(&s[i + 2..i + 4], 16) {
                        out.push(v);
                        i += 4;
                        continue;
                    }
                    out.push(b'\\');
                    i += 1;
                }
                b'n' => { out.push(b'\n'); i += 2; }
                b'r' => { out.push(b'\r'); i += 2; }
                b't' => { out.push(b'\t'); i += 2; }
                b'e' => { out.push(0x1b); i += 2; }
                c => { out.push(c); i += 2; }
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Gleiches Netz mehrfach (2,4/5 GHz, Mesh) nur einmal, mit dem stärksten Signal; versteckte weglassen.
pub fn tidy(mut nets: Vec<Net>) -> Vec<Net> {
    nets.retain(|n| !n.0.is_empty() && !n.0.chars().all(|c| c == '\0'));
    nets.sort_by_key(|n| -n.1);
    let mut seen = std::collections::HashSet::new();
    nets.retain(|n| seen.insert(n.0.clone()));
    nets
}

/// SCAN_RESULTS: „bssid / frequency / signal level / flags / ssid“, tabgetrennt.
pub fn parse_results(s: &str) -> Vec<Net> {
    let nets = s
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(5, '\t').collect();
            if f.len() < 5 {
                return None;
            }
            let sig = f[2].trim().parse().unwrap_or(-100);
            let sec = ["WPA", "RSN", "WEP", "SAE", "OWE"].iter().any(|k| f[3].contains(k));
            Some((unescape(f[4]), sig, sec))
        })
        .collect();
    tidy(nets)
}

/// Netze suchen und auf das Ergebnis warten (höchstens ~12 s). None = Schnittstelle nicht erreichbar.
pub fn scan(iface: &str) -> Option<Vec<Net>> {
    let c = Ctrl::open(iface)?;
    let ev = Ctrl::open(iface).filter(|e| e.ok("ATTACH"));
    let r = c.cmd("SCAN")?;
    // FAIL-BUSY = es läuft schon eine Suche; dann einfach auf deren Ergebnis warten
    if r.trim() == "OK" || r.contains("BUSY") {
        if let Some(e) = &ev {
            e.wait_event(|x| x.starts_with("CTRL-EVENT-SCAN-RESULTS") || x.starts_with("CTRL-EVENT-SCAN-FAILED"), Duration::from_secs(12));
            let _ = e.cmd("DETACH");
        } else {
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    Some(parse_results(&c.cmd("SCAN_RESULTS").unwrap_or_default()))
}

/// STATUS als Schlüssel/Wert-Liste.
pub fn status(iface: &str) -> Option<Vec<(String, String)>> {
    let r = Ctrl::open(iface)?.cmd("STATUS")?;
    Some(r.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.to_string(), v.to_string())).collect())
}

pub fn get<'a>(st: &'a [(String, String)], k: &str) -> Option<&'a str> {
    st.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
}

/// Signal der bestehenden Verbindung: (RSSI dBm, Frequenz MHz).
pub fn signal(iface: &str) -> Option<(i32, i32)> {
    let r = Ctrl::open(iface)?.cmd("SIGNAL_POLL")?;
    let g = |k: &str| r.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| v.trim().parse::<i32>().ok());
    Some((g("RSSI=")?, g("FREQUENCY=").unwrap_or(0)))
}

/// Gespeicherte Netze: (id, Name).
pub fn saved(iface: &str) -> Vec<(u32, String)> {
    let Some(r) = Ctrl::open(iface).and_then(|c| c.cmd("LIST_NETWORKS")) else { return Vec::new() };
    r.lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            Some((f.first()?.parse().ok()?, unescape(f.get(1)?)))
        })
        .collect()
}

/// Kennwort prüfen: leer = offenes Netz, 8–63 Zeichen Passphrase oder 64 Hex-Zeichen.
pub fn check_pw(pw: &str) -> Result<(), String> {
    if pw.is_empty() || (pw.len() == 64 && pw.chars().all(|c| c.is_ascii_hexdigit())) {
        return Ok(());
    }
    if pw.len() < 8 || pw.len() > 63 || pw.contains(['\n', '\r']) {
        return Err("8–63".into());
    }
    Ok(())
}

/// Netz anlegen (ersetzt einen alten Eintrag gleichen Namens), auswählen und speichern.
pub fn connect(iface: &str, ssid: &str, pw: &str) -> Result<(), String> {
    check_pw(pw)?;
    let c = Ctrl::open(iface).ok_or("wpa_supplicant")?;
    for (id, n) in saved(iface) {
        if n == ssid {
            c.ok(&format!("REMOVE_NETWORK {id}"));
        }
    }
    let id: u32 = c.cmd("ADD_NETWORK").and_then(|r| r.trim().parse().ok()).ok_or("ADD_NETWORK")?;
    let hex: String = ssid.bytes().map(|b| format!("{b:02x}")).collect();
    let mut set = vec![format!("ssid {hex}"), "scan_ssid 1".to_string()];
    if pw.is_empty() {
        set.push("key_mgmt NONE".into());
    } else if pw.len() == 64 {
        set.push(format!("psk {pw}"));
    } else {
        set.push(format!("psk \"{pw}\""));
    }
    for s in set {
        if !c.ok(&format!("SET_NETWORK {id} {s}")) {
            c.ok(&format!("REMOVE_NETWORK {id}"));
            return Err(s.split(' ').next().unwrap_or("").to_string());
        }
    }
    if !c.ok(&format!("SELECT_NETWORK {id}")) {
        return Err("SELECT_NETWORK".into());
    }
    c.ok("SAVE_CONFIG");
    Ok(())
}

/// Gespeichertes Netz wieder wählen (ohne Kennwort-Eingabe).
pub fn select(iface: &str, ssid: &str) -> bool {
    let Some(c) = Ctrl::open(iface) else { return false };
    match saved(iface).into_iter().find(|(_, n)| n == ssid) {
        Some((id, _)) => c.ok(&format!("SELECT_NETWORK {id}")),
        None => false,
    }
}

/// Netz vergessen (alle Einträge mit diesem Namen) und speichern.
pub fn forget(iface: &str, ssid: &str) -> bool {
    let Some(c) = Ctrl::open(iface) else { return false };
    let mut any = false;
    for (id, n) in saved(iface) {
        if n == ssid {
            any |= c.ok(&format!("REMOVE_NETWORK {id}"));
        }
    }
    c.ok("SAVE_CONFIG");
    any
}

/// Auf das Ergebnis eines Verbindungsversuchs warten.
/// Err("wrong-key") bei falschem Kennwort, Err("timeout") ohne Verbindung.
pub fn wait_connected(iface: &str, ssid: &str, timeout: Duration) -> Result<(), String> {
    let ev = Ctrl::open(iface).filter(|e| e.ok("ATTACH")).ok_or("wpa_supplicant")?;
    let done = |st: Option<Vec<(String, String)>>| st.map(|s| get(&s, "wpa_state") == Some("COMPLETED") && get(&s, "ssid").map(unescape).as_deref() == Some(ssid)).unwrap_or(false);
    if done(status(iface)) {
        return Ok(());
    }
    let end = Instant::now() + timeout;
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        let e = ev.wait_event(|x| x.starts_with("CTRL-EVENT-CONNECTED") || x.starts_with("CTRL-EVENT-SSID-TEMP-DISABLED") || x.starts_with("CTRL-EVENT-ASSOC-REJECT") || x.contains("WRONG_KEY"), left);
        let Some(e) = e else { break };
        if e.contains("WRONG_KEY") {
            let _ = ev.cmd("DETACH");
            return Err("wrong-key".into());
        }
        if e.starts_with("CTRL-EVENT-CONNECTED") && done(status(iface)) {
            let _ = ev.cmd("DETACH");
            return Ok(());
        }
    }
    let _ = ev.cmd("DETACH");
    if done(status(iface)) { Ok(()) } else { Err("timeout".into()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn results_parse_dedup_unescape() {
        let s = "bssid / frequency / signal level / flags / ssid\n\
                 aa:bb:cc:00:00:01\t2412\t-70\t[WPA2-PSK-CCMP][ESS]\tHome\n\
                 aa:bb:cc:00:00:02\t5180\t-48\t[WPA2-PSK-CCMP][ESS]\tHome\n\
                 aa:bb:cc:00:00:03\t2437\t-60\t[ESS]\tCaf\\xc3\\xa9 Gast\n\
                 aa:bb:cc:00:00:04\t2437\t-55\t[WPA2-PSK-CCMP][ESS]\t\n";
        let n = parse_results(s);
        assert_eq!(n, vec![("Home".into(), -48, true), ("Café Gast".into(), -60, false)]);
    }

    #[test]
    fn pw_rules() {
        assert!(check_pw("").is_ok());
        assert!(check_pw("12345678").is_ok());
        assert!(check_pw("kurz").is_err());
        assert!(check_pw("mit \"Anführung\" ok").is_ok());
        assert!(check_pw(&"a".repeat(64)).is_ok());
        assert!(check_pw(&"z".repeat(64)).is_err());
    }
}

/// Gegen ein laufendes wpa_supplicant prüfen: ULTAR_WPA_IFACE=lo cargo test -- --ignored ctrl_roundtrip
/// (z. B. `wpa_supplicant -i lo -D wired -c w.conf -B` mit ctrl_interface + update_config).
#[cfg(test)]
mod live {
    use super::*;

    #[test]
    #[ignore]
    fn ctrl_roundtrip() {
        let ifc = std::env::var("ULTAR_WPA_IFACE").unwrap_or_else(|_| "lo".into());
        assert!(available(&ifc));
        assert!(status(&ifc).is_some());
        let _ = connect(&ifc, "Test Netz é", "geheim123");
        let _ = connect(&ifc, "Offen", "");
        let _ = connect(&ifc, "Test Netz é", "neues-kennwort"); // ersetzt, nicht doppelt
        let s: Vec<String> = saved(&ifc).into_iter().map(|(_, n)| n).collect();
        assert_eq!(s.iter().filter(|n| *n == "Test Netz é").count(), 1, "{s:?}");
        assert!(s.contains(&"Offen".to_string()));
        assert!(select(&ifc, "Offen"));
        assert!(forget(&ifc, "Test Netz é"));
        let s: Vec<String> = saved(&ifc).into_iter().map(|(_, n)| n).collect();
        assert!(!s.contains(&"Test Netz é".to_string()));
        assert!(forget(&ifc, "Offen"));
    }
}
