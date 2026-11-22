//! Systemzugriff: Einstellungen, Zeit, Speicher, Netzstatus, Dienste, Hintergrundaufträge.

use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::collections::HashSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::fd::OwnedFd;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::Arc;

pub const COMPAT: &str = "/usr/bin/ultar-compat";

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Settings {
    pub lang: String,
    pub dark: bool,
    pub accent: usize,
    pub scale: f32,
    pub rotation: u16,
    pub brightness: f32,
    pub timeout_s: u32,
    pub tz_min: i32,
    pub hostname: String,
    /// Ersteinrichtung abgeschlossen
    pub setup_done: bool,
    /// Werbe- und Tracker-Server systemweit sperren
    pub adblock: bool,
    /// Töne (Wecker, Hinweise) und ihre Lautstärke 0..1
    pub sound: bool,
    pub volume: f32,
    // ---- Datenschutz/Sicherheit: Kontrolle des Nutzers über die Fähigkeiten des Geräts ----
    /// Hauptschalter: aus = ALLES aus (Netz, Kamera, Mikro, Ort, Sensoren, Bluetooth). Standard: an.
    pub priv_all: bool,
    pub priv_net: bool,
    pub priv_cam: bool,
    pub priv_mic: bool,
    pub priv_loc: bool,
    pub priv_sensors: bool,
    pub priv_bt: bool,
    /// Hilfe-Knopf (ⓘ) in der Kopfzeile zeigen (abschaltbar unter Anzeige)
    pub help_btn: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { lang: "ar".into(), dark: true, accent: 0, scale: 1.0, rotation: 0, brightness: 1.0, timeout_s: 0, tz_min: 60, hostname: String::new(), setup_done: false, adblock: true, sound: true, volume: 0.7, priv_all: true, priv_net: true, priv_cam: true, priv_mic: true, priv_loc: true, priv_sensors: true, priv_bt: true, help_btn: true }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct NetCfg {
    pub dhcp: bool,
    pub address: String,
    pub prefix: u8,
    pub gateway: String,
    pub dns: String,
    /// Schnittstellen, die der Netzwerkdienst nicht anfassen soll
    pub ignore: Vec<String>,
}

impl Default for NetCfg {
    fn default() -> Self {
        Self { dhcp: true, address: String::new(), prefix: 24, gateway: String::new(), dns: String::new(), ignore: Vec::new() }
    }
}

pub fn data_dir() -> String {
    std::env::var("ULTAR_DATA").unwrap_or_else(|_| "/data/ultar".into())
}

pub fn load<T: Default + DeserializeOwned>(name: &str) -> T {
    std::fs::read_to_string(format!("{}/{name}", data_dir())).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default()
}

/// Atomar und stromausfallsicher speichern: temporäre Datei schreiben, auf den
/// Datenträger zwingen (fsync), umbenennen, Verzeichnis sichern.
pub fn save<T: Serialize>(name: &str, v: &T) -> Result<(), String> {
    let dir = data_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let s = toml::to_string(v).map_err(|e| e.to_string())?;
    let tmp = format!("{dir}/.{name}.tmp");
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(s.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, format!("{dir}/{name}")).map_err(|e| e.to_string())?;
    if let Ok(d) = std::fs::File::open(&dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

// ---------- Zeit ----------

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DateTime {
    pub y: i64,
    pub mo: i64,
    pub d: i64,
    pub h: i64,
    pub mi: i64,
    pub s: i64,
    pub wd: usize, // 0 = Sonntag
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub fn unix_now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn from_unix(t: i64, tz_min: i32) -> DateTime {
    let t = t + tz_min as i64 * 60;
    let days = t.div_euclid(86400);
    let rem = t.rem_euclid(86400);
    let (y, mo, d) = civil_from_days(days);
    DateTime { y, mo, d, h: rem / 3600, mi: rem % 3600 / 60, s: rem % 60, wd: (days + 4).rem_euclid(7) as usize }
}

pub fn to_unix(dt: &DateTime, tz_min: i32) -> i64 {
    days_from_civil(dt.y, dt.mo, dt.d) * 86400 + dt.h * 3600 + dt.mi * 60 + dt.s - tz_min as i64 * 60
}

pub fn days_in_month(y: i64, m: i64) -> i64 {
    days_from_civil(if m == 12 { y + 1 } else { y }, if m == 12 { 1 } else { m + 1 }, 1) - days_from_civil(y, m, 1)
}

/// Systemzeit und Hardware-Uhr (RTC, in UTC) setzen.
pub fn set_time(t: i64) -> Result<(), String> {
    use nix::sys::time::TimeSpec;
    nix::time::clock_settime(nix::time::ClockId::CLOCK_REALTIME, TimeSpec::new(t, 0)).map_err(|e| e.to_string())?;
    if let Ok(f) = std::fs::OpenOptions::new().write(true).open("/dev/rtc0") {
        let u = from_unix(t, 0);
        let tm: [i32; 9] = [u.s as i32, u.mi as i32, u.h as i32, u.d as i32, u.mo as i32 - 1, u.y as i32 - 1900, 0, 0, 0];
        extern "C" {
            fn ioctl(fd: i32, req: u64, ...) -> i32;
        }
        use std::os::fd::AsRawFd;
        unsafe { ioctl(f.as_raw_fd(), 0x4024_700a /* RTC_SET_TIME */, tm.as_ptr()) };
    }
    Ok(())
}

// ---------- Hardware / Status ----------

fn meminfo_raw() -> (u64, u64) {
    let s = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
    let get = |k: &str| s.lines().find(|l| l.starts_with(k)).and_then(|l| l.split_whitespace().nth(1)).and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    (get("MemTotal:") * 1024, get("MemAvailable:") * 1024)
}

pub fn is_mounted(path: &str) -> bool {
    path == "/" || std::fs::read_to_string("/proc/mounts").map(|m| m.lines().any(|l| l.split_whitespace().nth(1) == Some(path))).unwrap_or(false)
}

/// Wartet (begrenzt), bis /data eingebunden ist. ultar-init mountet die große userdata-ext4 im
/// HINTERGRUND, damit der Boot nicht blockiert – die Shell startet also u. U. vorher. Ohne dieses
/// Warten liest `load("settings.toml")` ins Leere → Default (setup_done=false) → das Setup erscheint
/// bei JEDEM Neustart. Darum vor dem ersten Laden der persistenten Einstellungen hier warten.
/// Bricht nach ~12 s ab (echtes Erstsystem ohne /data: dann ist Setup korrekt).
pub fn wait_for_data() {
    if std::env::var("ULTAR_DATA").is_ok() {
        return; // Test/Override: nicht auf /data warten
    }
    let t0 = std::time::Instant::now();
    while t0.elapsed() < std::time::Duration::from_secs(12) {
        // Fertig, sobald /data gemountet ist (oder die Einstellungen schon lesbar sind).
        if is_mounted("/data") || std::path::Path::new("/data/ultar/settings.toml").exists() {
            // kleine Nachlaufzeit, damit /data/ultar nach dem Mount sicher angelegt ist
            std::thread::sleep(std::time::Duration::from_millis(50));
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

/// (gesamt, frei) in Bytes
pub fn disk(path: &str) -> Option<(u64, u64)> {
    if !is_mounted(path) {
        return None;
    }
    let s = nix::sys::statvfs::statvfs(path).ok()?;
    Some((s.blocks() as u64 * s.fragment_size() as u64, s.blocks_available() as u64 * s.fragment_size() as u64))
}

pub fn human(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else {
        format!("{} MB", b >> 20)
    }
}

#[derive(Clone, Debug, Default)]
pub struct NetIf {
    pub name: String,
    pub wifi: bool,
    pub state: String,
    pub ip: String,
    pub gw: String,
    pub dns: String,
    pub mac: String,
}

pub fn net_status() -> Vec<NetIf> {
    memo("net_status", 1500, net_status_raw)
}
fn net_status_raw() -> Vec<NetIf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir("/run/ultar/net") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let mut n = NetIf { name, ..Default::default() };
            for l in std::fs::read_to_string(e.path()).unwrap_or_default().lines() {
                if let Some((k, val)) = l.split_once('=') {
                    match k {
                        "state" => n.state = val.into(),
                        "kind" => n.wifi = val == "wlan",
                        "ip" => n.ip = val.into(),
                        "gw" => n.gw = val.into(),
                        "dns" => n.dns = val.into(),
                        "mac" => n.mac = val.into(),
                        _ => {}
                    }
                }
            }
            v.push(n);
        }
    }
    v.sort_by(|a, b| a.name.cmp(&b.name));
    v
}

pub fn online() -> bool {
    net_status().iter().any(|n| n.state == "verbunden")
}

/// Akkustand in Prozent, falls ein Akku vorhanden ist.
pub fn battery() -> Option<(u8, bool)> {
    battery_info().map(|b| (b.percent, b.charging))
}

#[derive(Clone, Debug)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
    /// Rohstatus des Kernels: Charging, Discharging, Full, Not charging
    pub status: String,
    pub health: String,
    pub temp_c: Option<f32>,
    pub volt: Option<f32>,
}

/// Akku aus /sys/class/power_supply. Erkennt den Akku am Typ **oder** am Namen
/// (Samsung nutzt oft „battery"/„bms" und meldet den Typ nicht sauber).
pub fn battery_info() -> Option<Battery> {
    memo("battery_info", 4000, battery_info_raw)
}
fn battery_info_raw() -> Option<Battery> {
    let entries: Vec<_> = std::fs::read_dir("/sys/class/power_supply").ok()?.flatten().collect();
    // zuerst die klar als Akku benannten Knoten probieren
    let order = |e: &std::fs::DirEntry| -> i32 {
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n == "battery" { 0 } else if n.contains("battery") || n == "bms" { 1 } else { 2 }
    };
    let mut entries = entries;
    entries.sort_by_key(order);
    for e in entries {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_lowercase();
        let rd = |f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
        let is_batt = rd("type") == "Battery" || name == "battery" || name.contains("battery") || name == "bms";
        if !is_batt {
            continue;
        }
        // Prozent direkt oder aus Ladung/Energie berechnet (manche Treiber liefern kein "capacity")
        let pct = rd("capacity").parse::<f32>().ok().or_else(|| {
            let ratio = |now: &str, full: &str| Some(rd(now).parse::<f32>().ok()? / rd(full).parse::<f32>().ok().filter(|v| *v > 0.0)? * 100.0);
            ratio("charge_now", "charge_full").or_else(|| ratio("energy_now", "energy_full"))
        });
        let Some(pct) = pct else { continue };
        let status = rd("status");
        return Some(Battery {
            percent: pct.clamp(0.0, 100.0).round() as u8,
            charging: status == "Charging" || status == "Full",
            status,
            health: rd("health"),
            temp_c: rd("temp").parse::<f32>().ok().map(|t| t / 10.0),
            volt: rd("voltage_now").parse::<f32>().ok().map(|v| v / 1_000_000.0),
        });
    }
    None
}

/// Hintergrundbeleuchtung setzen. Liefert false, wenn keine steuerbar ist (dann Software-Abdunkelung).
pub fn set_backlight(v: f32) -> bool {
    let mut any = false;
    if let Ok(rd) = std::fs::read_dir("/sys/class/backlight") {
        for e in rd.flatten() {
            let p = e.path();
            if let Some(max) = std::fs::read_to_string(p.join("max_brightness")).ok().and_then(|s| s.trim().parse::<f32>().ok()) {
                let val = (max * v.clamp(0.0, 1.0)).round() as u32;
                any |= std::fs::write(p.join("brightness"), val.to_string()).is_ok();
            }
        }
    }
    any
}

pub fn read_trim(p: &str) -> String {
    std::fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

pub fn cpu() -> String {
    let s = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let n = s.lines().filter(|l| l.starts_with("processor")).count().max(1);
    let model = s.lines().find(|l| l.starts_with("model name") || l.starts_with("Hardware") || l.starts_with("Model")).and_then(|l| l.split_once(':')).map(|(_, v)| v.trim().to_string()).unwrap_or_else(|| std::env::consts::ARCH.to_string());
    format!("{n} × {model}")
}

pub fn uptime_s() -> u64 {
    read_trim("/proc/uptime").split('.').next().and_then(|v| v.parse().ok()).unwrap_or(0)
}

// ---------- ultar-init ----------

pub fn ctl(cmd: &str) -> String {
    let path = std::env::var("ULTAR_CTL_SOCK").unwrap_or_else(|_| "/run/ultar/ctl.sock".into());
    let Ok(mut s) = std::os::unix::net::UnixStream::connect(path) else {
        // ohne ultar-init (z. B. auf dem Ultar-Kern): Neustart/Ausschalten direkt über den Kern
        if matches!(cmd, "reboot" | "poweroff") && std::env::var("ULTAR_DRY").is_err() {
            nix::unistd::sync();
            let m = if cmd == "reboot" { nix::sys::reboot::RebootMode::RB_AUTOBOOT } else { nix::sys::reboot::RebootMode::RB_POWER_OFF };
            let _ = nix::sys::reboot::reboot(m);
        }
        return String::new();
    };
    let _ = s.set_read_timeout(Some(std::time::Duration::from_secs(2)));
    if s.write_all(format!("{cmd}\n").as_bytes()).is_err() {
        return String::new();
    }
    let _ = s.shutdown(std::net::Shutdown::Write);
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[derive(Clone, Debug)]
pub struct Svc {
    pub name: String,
    pub state: String,
    pub pid: String,
}

pub fn services() -> Vec<Svc> {
    ctl("status")
        .lines()
        .skip(1)
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (p.len() >= 4).then(|| Svc { name: p[0].into(), state: p[1].into(), pid: p[2].into() })
        })
        .collect()
}

// ---------- Hintergrundaufträge ----------

pub enum Msg {
    JobLine(String),
    JobDone(i32),
    Search(String, Vec<(String, String)>),
    Installed(HashSet<String>),
    TermLine(String),
    TermDone(i32),
    /// Hardware neu einlesen (nach der Treibersuche)
    HwChanged,
    /// Antwort des Sprachmodells bzw. Fehlertext
    AgentReply(Result<String, String>),
    /// Ergebnis eines länger laufenden Agenten-Werkzeugs
    AgentTool(String),
    /// Ergebnis mit Bild (Text, Bilddatei) – z. B. Kamerafoto für den Assistenten
    AgentToolImg(String, String),
    AgentUp(bool),
    /// Gefundene WLAN-Netze: (Name, Signal in dBm, verschlüsselt) und die gespeicherten Netze
    Wifi(Vec<(String, i32, bool)>, Vec<String>),
    /// Meldung des Hintergrunddienstes (Energiesparen, Zeitabgleich)
    Sys(crate::sysd::SysMsg),
    /// Ergebnis eines Verbindungsversuchs (Netzname, Ok oder Fehlerkennung)
    WifiDone(String, Result<(), String>),
    /// App-Katalog bzw. Installationsstand hat sich geändert
    AppsChanged,
    /// Ergebnis der Suche nach einem Systemupdate
    Update(crate::update::Release),
}

#[derive(Clone)]
pub struct Bg {
    pub tx: Sender<Msg>,
    pub wake: Arc<OwnedFd>,
}

impl Bg {
    pub fn send(&self, m: Msg) {
        let _ = self.tx.send(m);
        let _ = nix::unistd::write(&*self.wake, &[1u8]);
    }

    /// Oberfläche wecken (neu zeichnen), ohne eine Nachricht zu schicken.
    pub fn poke(&self) {
        let _ = nix::unistd::write(&*self.wake, &[1u8]);
    }

    /// Programm ausführen und warten; Ausgabe wird zeilenweise gemeldet. Liefert den Exit-Code.
    pub fn stream(&self, prog: &str, args: &[String], line: fn(String) -> Msg) -> i32 {
        let child = Command::new(prog).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let mut child = match child {
            Ok(c) => c,
            Err(e) => {
                self.send(line(format!("{prog}: {e}")));
                return 127;
            }
        };
        let pump = |r: Box<dyn Read + Send>, bg: Bg| {
            std::thread::spawn(move || {
                let mut r = BufReader::new(r);
                let mut buf = Vec::new();
                while r.read_until(b'\n', &mut buf).map(|n| n > 0).unwrap_or(false) {
                    let s = String::from_utf8_lossy(&buf);
                    // Fortschrittszeilen (\r) auf den letzten Stand kürzen
                    let s = s.trim_end().rsplit('\r').next().unwrap_or("").to_string();
                    if !s.is_empty() {
                        bg.send(line(s));
                    }
                    buf.clear();
                }
            })
        };
        let t1 = pump(Box::new(child.stdout.take().unwrap()), self.clone());
        let t2 = pump(Box::new(child.stderr.take().unwrap()), self.clone());
        let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
        let _ = t1.join();
        let _ = t2.join();
        code
    }

    /// Programm im Hintergrund starten, Ausgabe zeilenweise melden.
    pub fn run(&self, prog: &str, args: Vec<String>, line: fn(String) -> Msg, done: fn(i32) -> Msg) {
        let bg = self.clone();
        let prog = prog.to_string();
        std::thread::spawn(move || {
            let code = bg.stream(&prog, &args, line);
            bg.send(done(code));
        });
    }
}

pub fn capture(prog: &str, args: &[&str]) -> String {
    Command::new(prog).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().map(|o| String::from_utf8_lossy(&o.stdout).into_owned()).unwrap_or_default()
}

// ---------- WLAN ----------

pub const WIFI_CONF: &str = "/data/ultar/wifi.conf";

/// Native (statische) Ultar-WLAN-Programme, falls vorhanden – sonst Ubuntu-Bereich.
pub fn iw_bin() -> Option<String> {
    for p in ["/usr/lib/ultar/iw", "/sbin/iw", "/usr/sbin/iw"] {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

pub fn wpa_bin() -> Option<String> {
    for p in ["/usr/lib/ultar/wpa_supplicant", "/sbin/wpa_supplicant", "/usr/sbin/wpa_supplicant"] {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    None
}

pub fn wifi_tools() -> bool {
    // Entweder die nativen statischen Programme oder der Ubuntu-Bereich reichen.
    (wpa_bin().is_some() && iw_bin().is_some())
        || (std::path::Path::new("/compat/usr/sbin/wpa_supplicant").exists() && std::path::Path::new("/compat/usr/sbin/iw").exists())
}

/// WLAN-Hauptschnittstelle: bevorzugt wlan*, nie Nebenschnittstellen (p2p0 für Wi-Fi Direct,
/// swlan0 für Hotspot) – „p2p0“ stünde alphabetisch vor „wlan0“.
pub fn wifi_iface() -> Option<String> {
    let ignore = load::<NetCfg>("net.toml").ignore;
    let mut v: Vec<String> = std::fs::read_dir("/sys/class/net")
        .ok()?
        .flatten()
        .filter(|e| e.path().join("phy80211").exists())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| !ignore.contains(n) && !["p2p", "swlan", "ap", "aware", "nan"].iter().any(|p| n.starts_with(p)))
        .collect();
    v.sort_by_key(|n| (!n.starts_with("wlan"), n.clone()));
    v.into_iter().next()
}

/// Zustand laut wpa_supplicant (COMPLETED, ASSOCIATING, DISCONNECTED, INTERFACE_DISABLED …).
pub fn wifi_state() -> Option<String> {
    let st = crate::wifi::status(&wifi_iface()?)?;
    crate::wifi::get(&st, "wpa_state").map(str::to_string)
}

/// Netz, mit dem das Gerät verbunden ist bzw. sich gerade verbindet. Ohne Steuerschnittstelle:
/// das gespeicherte Netz aus der Konfiguration.
pub fn wifi_ssid() -> Option<String> {
    memo("wifi_ssid", 2000, wifi_ssid_raw)
}
fn wifi_ssid_raw() -> Option<String> {
    let ifc = wifi_iface()?;
    if let Some(st) = crate::wifi::status(&ifc) {
        return crate::wifi::get(&st, "ssid").map(crate::wifi::unescape);
    }
    let s = std::fs::read_to_string(WIFI_CONF).ok()?;
    let v = s.lines().find_map(|l| l.trim().strip_prefix("ssid="))?;
    if let Some(q) = v.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        return Some(q.to_string());
    }
    let bytes: Vec<u8> = (0..v.len() / 2).filter_map(|i| u8::from_str_radix(&v[2 * i..2 * i + 2], 16).ok()).collect();
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

/// Zugangsdaten als Datei speichern (Rückfallweg ohne Steuerschnittstelle; nur für root lesbar).
pub fn wifi_save(ssid: &str, password: &str) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt;
    crate::wifi::check_pw(password)?;
    let hex: String = ssid.bytes().map(|b| format!("{b:02x}")).collect();
    let auth = if password.is_empty() {
        "key_mgmt=NONE".to_string()
    } else if password.len() == 64 {
        format!("psk={password}")
    } else {
        format!("psk=\"{password}\"")
    };
    let conf = format!("ctrl_interface=DIR=/run/wpa_supplicant\nupdate_config=1\nnetwork={{\nssid={hex}\n{auth}\nscan_ssid=1\n}}\n");
    std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
    let tmp = format!("{WIFI_CONF}.tmp");
    let mut f = std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(&tmp).map_err(|e| e.to_string())?;
    f.write_all(conf.as_bytes()).and_then(|_| f.sync_all()).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, WIFI_CONF).map_err(|e| e.to_string())
}

/// Schnittstelle per ioctl einschalten (Suche auf ausgeschalteter Schnittstelle liefert nichts).
pub fn iface_up(name: &str, up: bool) -> bool {
    use nix::libc;
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC, 0);
        if fd < 0 {
            return false;
        }
        let mut r: libc::ifreq = std::mem::zeroed();
        for (i, c) in name.bytes().take(libc::IFNAMSIZ - 1).enumerate() {
            r.ifr_name[i] = c as libc::c_char;
        }
        let mut ok = libc::ioctl(fd, libc::SIOCGIFFLAGS, &mut r) >= 0;
        if ok && ((r.ifr_ifru.ifru_flags & libc::IFF_UP as i16) != 0) != up {
            if up {
                r.ifr_ifru.ifru_flags |= libc::IFF_UP as i16;
            } else {
                r.ifr_ifru.ifru_flags &= !(libc::IFF_UP as i16);
            }
            ok = libc::ioctl(fd, libc::SIOCSIFFLAGS, &r) >= 0;
        }
        libc::close(fd);
        ok
    }
}

/// Suche mit `iw` (Rückfallweg). „Device or resource busy“ = es läuft schon eine Suche → kurz warten.
fn iw_scan(ifc: &str) -> Vec<(String, i32, bool)> {
    let run = || match iw_bin() {
        Some(iw) => capture(&iw, &["dev", ifc, "scan"]),
        None => capture(COMPAT, &["run", "--", "/usr/sbin/iw", "dev", ifc, "scan"]),
    };
    let mut out = run();
    if out.trim().is_empty() {
        std::thread::sleep(std::time::Duration::from_millis(1500));
        out = run();
    }
    let mut nets: Vec<(String, i32, bool)> = Vec::new();
    let mut cur: Option<(String, i32, bool)> = None;
    for l in out.lines() {
        let t = l.trim();
        if l.starts_with("BSS ") {
            nets.extend(cur.take());
            cur = Some((String::new(), -100, false));
        } else if let Some(c) = cur.as_mut() {
            if let Some(v) = t.strip_prefix("SSID: ") {
                c.0 = crate::wifi::unescape(v);
            } else if let Some(v) = t.strip_prefix("signal: ") {
                c.1 = v.split('.').next().and_then(|x| x.trim().parse().ok()).unwrap_or(-100);
            } else if t.starts_with("RSN:") || t.starts_with("WPA:") || t.starts_with("capability:") && t.contains("Privacy") {
                c.2 = true;
            }
        }
    }
    nets.extend(cur);
    crate::wifi::tidy(nets)
}

pub fn wifi_scan(bg: &Bg) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let Some(ifc) = wifi_iface() else { return bg.send(Msg::Wifi(Vec::new(), Vec::new())) };
        iface_up(&ifc, true);
        let nets = crate::wifi::scan(&ifc).unwrap_or_else(|| iw_scan(&ifc));
        let saved = crate::wifi::saved(&ifc).into_iter().map(|(_, n)| n).collect();
        bg.send(Msg::Wifi(nets, saved));
    });
}

/// Mit einem Netz verbinden. Mit Kennwort: neu anlegen; ohne (und gespeichert): gespeichertes wählen.
/// Das Ergebnis kommt als Msg::WifiDone.
pub fn wifi_connect(bg: &Bg, ssid: String, password: Option<String>) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let Some(ifc) = wifi_iface() else { return bg.send(Msg::WifiDone(ssid, Err("no-device".into()))) };
        if !crate::wifi::available(&ifc) {
            // Rückfallweg: Datei schreiben, Netzdienst startet wpa_supplicant neu
            let r = wifi_save(&ssid, password.as_deref().unwrap_or("")).map(|_| {
                ctl("restart net");
            });
            return bg.send(Msg::WifiDone(ssid, r));
        }
        let r = match &password {
            Some(pw) => crate::wifi::connect(&ifc, &ssid, pw),
            None if crate::wifi::select(&ifc, &ssid) => Ok(()),
            None => crate::wifi::connect(&ifc, &ssid, ""),
        };
        let r = r.and_then(|_| crate::wifi::wait_connected(&ifc, &ssid, std::time::Duration::from_secs(25)));
        bg.send(Msg::WifiDone(ssid, r));
    });
}

/// Netz vergessen.
pub fn wifi_forget(ssid: &str) {
    match wifi_iface() {
        Some(ifc) if crate::wifi::available(&ifc) => {
            crate::wifi::forget(&ifc, ssid);
        }
        _ => {
            let _ = std::fs::remove_file(WIFI_CONF);
            ctl("restart net");
        }
    }
}

/// Kann der Ultar-Desktop hier laufen? Grafikchip mit DRM/KMS (GPU-Weg) oder Bildspeicher (fbdev/DECON, Pixman-Weg).
pub fn has_drm() -> bool {
    std::fs::read_dir("/dev/dri")
        .map(|d| d.flatten().any(|e| e.file_name().to_string_lossy().starts_with("card")))
        .unwrap_or(false)
        || ["/dev/fb0", "/dev/graphics/fb0"].iter().any(|p| std::path::Path::new(p).exists())
}

pub fn compat_ready() -> bool {
    std::path::Path::new("/compat/usr/bin/dpkg").exists()
}

pub fn valid_pkg(name: &str) -> bool {
    !name.is_empty() && name.len() < 100 && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "+.-".contains(c)) && !name.starts_with('-')
}

pub fn search_packages(bg: &Bg, query: String) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let q = query.trim().to_lowercase();
        let out = capture(COMPAT, &["run", "--", "/usr/bin/apt-cache", "search", "--names-only", "--", &q]);
        let mut res: Vec<(String, String)> = out.lines().filter_map(|l| l.split_once(" - ")).map(|(a, b)| (a.trim().to_string(), b.trim().to_string())).filter(|(n, _)| valid_pkg(n)).collect();
        // exakte Treffer und Präfixe zuerst
        res.sort_by_key(|(n, _)| (n != &q, !n.starts_with(&q), n.len(), n.clone()));
        res.truncate(80);
        bg.send(Msg::Search(query, res));
    });
}

pub fn load_installed(bg: &Bg) {
    let bg = bg.clone();
    std::thread::spawn(move || {
        let out = capture(COMPAT, &["run", "--", "/usr/bin/dpkg-query", "-W", "-f=${Package} ${db:Status-Abbrev}\\n"]);
        let set = out.lines().filter_map(|l| l.split_once(' ')).filter(|(_, st)| st.starts_with("ii")).map(|(n, _)| n.to_string()).collect();
        bg.send(Msg::Installed(set));
    });
}

/// Näherungssensor: Some(true)=verdeckt/nah (Handy am Ohr), Some(false)=frei, None=kein Sensor.
/// Best-effort über die üblichen Samsung-Sensorhub- und IIO-Knoten. Konvention: Wert > 0 = nah.
pub fn proximity_near() -> Option<bool> {
    // 1) Samsung-Sensorhub: /sys/class/sensors/*proximity*/{state,prox_raw,raw_data,adc}
    if let Ok(rd) = std::fs::read_dir("/sys/class/sensors") {
        for e in rd.flatten() {
            let name = std::fs::read_to_string(e.path().join("name")).unwrap_or_default();
            let n = name.to_lowercase();
            if n.contains("prox") {
                for f in ["state", "prox_raw", "raw_data", "adc", "value", "data"] {
                    if let Ok(s) = std::fs::read_to_string(e.path().join(f)) {
                        if let Some(v) = s.split_whitespace().next().and_then(|t| t.parse::<f32>().ok()) {
                            return Some(v > 0.0);
                        }
                    }
                }
            }
        }
    }
    // 2) IIO: /sys/bus/iio/devices/iio:deviceN/in_proximity_raw|in_proximity_input
    if let Ok(rd) = std::fs::read_dir("/sys/bus/iio/devices") {
        for e in rd.flatten() {
            for f in ["in_proximity_raw", "in_proximity_input"] {
                if let Ok(s) = std::fs::read_to_string(e.path().join(f)) {
                    if let Some(v) = s.split_whitespace().next().and_then(|t| t.parse::<f32>().ok()) {
                        return Some(v > 0.0);
                    }
                }
            }
        }
    }
    None
}

/// Kurzer Vibrationsimpuls (ms) über die üblichen Vibrationsknoten. Best-effort.
pub fn vibrate(ms: u32) {
    let v = ms.to_string();
    for p in ["/sys/class/timed_output/vibrator/enable", "/sys/class/leds/vibrator/duration"] {
        if std::fs::write(p, &v).is_ok() {
            // leds/vibrator braucht zusätzlich brightness/activate
            if p.contains("leds") {
                let base = "/sys/class/leds/vibrator";
                let _ = std::fs::write(format!("{base}/activate"), "1");
                let _ = std::fs::write(format!("{base}/brightness"), "255");
            }
            return;
        }
    }
}

// ------------------------------------------------------------ Zwischenspeicher für Systemabfragen
// Die Oberfläche fragt Akku, Netz und Funk in JEDEM Bild ab (Statusleiste, Schnellmenü). Auf dem Gerät
// sind das sysfs-Lesezugriffe (Akku über I²C!) und Steuer-Sockets – teuer und blockierend. Deshalb kurz merken.
thread_local! {
    static MEMO: std::cell::RefCell<std::collections::HashMap<&'static str, (std::time::Instant, Box<dyn std::any::Any>)>> = Default::default();
}

/// Ergebnis von `f` unter `key` höchstens `ttl_ms` lang wiederverwenden.
pub fn memo<T: Clone + 'static>(key: &'static str, ttl_ms: u64, f: impl FnOnce() -> T) -> T {
    let hit = MEMO.with(|m| {
        m.borrow().get(key).and_then(|(t, v)| if t.elapsed().as_millis() < ttl_ms as u128 { v.downcast_ref::<T>().cloned() } else { None })
    });
    if let Some(v) = hit {
        return v;
    }
    let v = f();
    MEMO.with(|m| m.borrow_mut().insert(key, (std::time::Instant::now(), Box::new(v.clone()))));
    v
}

/// Wert unter `key` ersetzen, ohne die Gültigkeitsdauer neu zu starten.
pub fn memo_set<T: 'static>(key: &'static str, v: T) {
    MEMO.with(|m| {
        if let Some(e) = m.borrow_mut().get_mut(key) {
            e.1 = Box::new(v);
        }
    });
}

/// Nach einer Änderung (Schalter umgelegt) sofort frisch lesen.
pub fn memo_clear() {
    MEMO.with(|m| m.borrow_mut().clear());
}

/// VPN-Zustand (startet Hilfsprogramme – daher höchstens alle 2 s).
pub fn vpn_status() -> ultar_vpn::VpnStatus {
    memo("vpn", 2000, ultar_vpn::status)
}

/// Alle Benachrichtigungen aus /run/ultar/notify.log und <data>/notify.log (höchstens 1 s alt).
pub fn notes_all() -> Vec<ultar_notify::Note> {
    memo("notes", 1000, || {
        let mut paths = vec!["/run/ultar/notify.log".to_string(), format!("{}/notify.log", data_dir())];
        paths.dedup();
        let mut v = Vec::new();
        for p in paths {
            if let Ok(t) = std::fs::read_to_string(&p) {
                v.extend(t.lines().filter_map(ultar_notify::parse_line));
            }
        }
        v
    })
}

/// Speicher (gesamt, frei) – höchstens 1 s alt.
pub fn meminfo() -> (u64, u64) {
    memo("meminfo", 1000, meminfo_raw)
}

/// Akkus angeschlossener Geräte (Linux: /sys/class/power_supply mit scope=Device – Bluetooth-/Funk-Mäuse,
/// Tastaturen, Kopfhörer, Controller): (Name, Prozent)
pub fn geraete_akkus() -> Vec<(String, u8)> {
    memo("geraete_akkus", 10_000, || {
        let mut v = Vec::new();
        let Ok(rd) = std::fs::read_dir("/sys/class/power_supply") else { return v };
        for e in rd.flatten() {
            let p = e.path();
            let lies = |f: &str| std::fs::read_to_string(p.join(f)).map(|s| s.trim().to_string()).unwrap_or_default();
            if lies("scope") != "Device" {
                continue;
            }
            let Ok(pct) = lies("capacity").parse::<u8>() else { continue };
            let mut name = lies("model_name");
            if name.is_empty() {
                name = e.file_name().to_string_lossy().into_owned();
            }
            v.push((name.chars().take(24).collect(), pct.min(100)));
        }
        v.sort();
        v
    })
}
