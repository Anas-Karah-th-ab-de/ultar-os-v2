#![allow(dead_code)] // Kepler-Konstanten/ioctl-Weg bleiben als Referenz (Gerät nutzt Samsungs gpsd)
//! GNSS/GPS für Ultar OS (Samsung A30).
//!
//! Die A30 hat KEINEN externen NMEA-GPS-Chip (alle CONFIG_GPS_BCM* aus), sondern den
//! Samsung-SLSI-GNSS-Co-Prozessor „Kepler" über Shared Memory (CONFIG_GNSS_SHMEM_IF=y,
//! Treiber drivers/misc/gnss_if). Zugang über **/dev/gnss_ipc** (exynos_ipc-Frames).
//! Der Kepler muss – wie das Modem – zuerst mit seiner Firmware gebootet werden
//! (GNSS_IOCTL_LOAD_FIRMWARE + RELEASE_RESET); danach liefert er Positions-/NMEA-Daten
//! über den IPC-Kanal. Die Firmware-Boot-Schritte sind in A30-GNSS.md beschrieben und
//! am Gerät zu verifizieren (VERIFY-Markierungen unten).
//!
//! Dieses Modul enthält: einen vollständigen NMEA-0183-Parser (GGA/RMC/GSV), die
//! exynos_ipc-Entrahmung von /dev/gnss_ipc und eine saubere API `read_fix()`.
//! Der NMEA-Parser ist unabhängig nutzbar (auch falls später ein NMEA-tty dazukommt).

use std::fs::File;
use std::io::Read;
use std::os::fd::AsRawFd;

// ---------- Kernel-Konstanten (drivers/misc/gnss_if) ----------
pub const GNSS_DEV: &str = "/dev/gnss_ipc";
const GNSS_IOC_MAGIC: u32 = b'K' as u32;
const fn io(nr: u32) -> u64 {
    // _IO(magic, nr): Richtung 0, Größe 0
    (((GNSS_IOC_MAGIC) << 8) | nr) as u64
}
pub const GNSS_IOCTL_RESET: u64 = io(0x00);
pub const GNSS_IOCTL_LOAD_FIRMWARE: u64 = io(0x01);
pub const GNSS_IOCTL_REQ_BCMD: u64 = io(0x03);
pub const GNSS_IOCTL_RELEASE_RESET: u64 = io(0x08);

// ---------- exynos_ipc-Frame (include/exynos_ipc.h) ----------
const EXYNOS_START_MASK: u16 = 0xABCD;
const EXYNOS_HEADER_SIZE: usize = 12;
const EXYNOS_LEN_OFFSET: usize = 6; // u16: Gesamtlänge inkl. Header
const EXYNOS_CH_ID_OFFSET: usize = 8; // u8

// ---------- Positions-Datentyp ----------
#[derive(Clone, Debug, Default)]
pub struct GnssFix {
    pub lat: f64,
    pub lon: f64,
    pub alt_m: f64,
    /// 0 = kein Fix, 1 = GPS, 2 = DGPS
    pub fix_quality: u8,
    pub sats_used: u8,
    pub hdop: f32,
    /// UTC "hhmmss"
    pub utc: String,
    pub speed_kn: f32,
    pub course_deg: f32,
    /// Genauigkeit in Metern (aus dem Samsung-Dienst), < 0 = unbekannt
    pub acc_m: f32,
    pub valid: bool,
}

#[derive(Clone, Debug, Default)]
pub struct SatInfo {
    pub prn: u16,
    pub elevation: i16,
    pub azimuth: i16,
    pub snr: i16, // dBHz, -1 = nicht verfolgt
}

#[derive(Clone, Debug, Default)]
pub struct GnssState {
    pub fix: GnssFix,
    pub sats: Vec<SatInfo>,
    pub sats_in_view: u8,
    /// Satellitenliste kommt vom Samsung-Dienst ($ULSV) statt aus GSV
    pub sv_from_hal: bool,
    /// Zeitpunkt der letzten Position vom Dienst ($ULLOC)
    pub last_loc: Option<std::time::Instant>,
    /// Lese-Position in /run/ultar/gnss.nmea
    pub file_off: u64,
    /// Läuft der GNSS-Dienst (Datei vorhanden und aktuell)?
    pub service_up: bool,
}

impl GnssState {
    fn loc_fresh(&self) -> bool {
        self.last_loc
            .is_some_and(|t| t.elapsed() < std::time::Duration::from_secs(5))
    }
}

// ---------- NMEA-0183-Parser ----------
/// Prüft die NMEA-Prüfsumme (*HH am Ende). Fehlt sie, wird true geliefert (tolerant).
fn nmea_checksum_ok(s: &str) -> bool {
    let body = match (s.find('$'), s.find('*')) {
        (Some(d), Some(st)) if st > d + 1 => &s[d + 1..st],
        _ => return true,
    };
    let given = s.split('*').nth(1).map(|h| h.trim()).unwrap_or("");
    if given.len() < 2 {
        return true;
    }
    let want = u8::from_str_radix(&given[..2], 16).unwrap_or(0);
    let got = body.bytes().fold(0u8, |a, b| a ^ b);
    want == got
}

fn parse_coord(val: &str, hemi: &str) -> Option<f64> {
    // NMEA: ddmm.mmmm bzw. dddmm.mmmm
    if val.is_empty() {
        return None;
    }
    let dot = val.find('.')?;
    let deg_digits = if dot >= 4 { dot - 2 } else { return None };
    let deg: f64 = val[..deg_digits].parse().ok()?;
    let min: f64 = val[deg_digits..].parse().ok()?;
    let mut d = deg + min / 60.0;
    if hemi == "S" || hemi == "W" {
        d = -d;
    }
    Some(d)
}

/// Verarbeitet eine NMEA-Zeile und aktualisiert den Zustand. Liefert true bei Treffer.
pub fn feed_nmea_line(st: &mut GnssState, line: &str) -> bool {
    let line = line.trim();
    if !line.starts_with('$') || !nmea_checksum_ok(line) {
        return false;
    }
    let core = line.split('*').next().unwrap_or(line);
    let f: Vec<&str> = core.split(',').collect();
    if f.is_empty() {
        return false;
    }
    // Talker-ID ignorieren (GP/GN/GL/GA), nur den Satztyp (letzte 3 Zeichen) nehmen
    let typ = if f[0].len() >= 3 {
        &f[0][f[0].len() - 3..]
    } else {
        f[0]
    };
    match f[0] {
        // Eigene Zeilen von gnssclient (Samsung-GNSS-Dienst): Position und Satellitenliste
        "$ULLOC" if f.len() >= 7 => {
            let (Ok(la), Ok(lo)) = (f[1].parse::<f64>(), f[2].parse::<f64>()) else {
                return false;
            };
            st.fix.lat = la;
            st.fix.lon = lo;
            st.fix.alt_m = f[3].parse().unwrap_or(0.0);
            st.fix.acc_m = f[4].parse().unwrap_or(-1.0);
            st.fix.speed_kn = f[5].parse::<f32>().unwrap_or(0.0) * 1.943_844;
            st.fix.course_deg = f[6].parse().unwrap_or(0.0);
            if st.fix.fix_quality == 0 {
                st.fix.fix_quality = 1;
            }
            st.fix.valid = true;
            st.last_loc = Some(std::time::Instant::now());
            return true;
        }
        "$ULSV" if f.len() >= 3 => {
            st.sats_in_view = f[1].parse().unwrap_or(0);
            st.fix.sats_used = f[2].parse().unwrap_or(0);
            st.sats.clear();
            for e in &f[3..] {
                let v: Vec<&str> = e.split(':').collect();
                if v.len() >= 4 {
                    st.sats.push(SatInfo {
                        prn: v[0].parse().unwrap_or(0),
                        elevation: v[1].parse().unwrap_or(0),
                        azimuth: v[2].parse().unwrap_or(0),
                        snr: v[3].parse().unwrap_or(-1),
                    });
                }
            }
            st.sv_from_hal = true;
            return true;
        }
        _ => {}
    }
    match typ {
        "GGA" if f.len() >= 10 => {
            st.fix.utc = f[1].to_string();
            if let Some(la) = parse_coord(f[2], f[3]) {
                st.fix.lat = la;
            }
            if let Some(lo) = parse_coord(f[4], f[5]) {
                st.fix.lon = lo;
            }
            st.fix.fix_quality = f[6].parse().unwrap_or(0);
            if !st.sv_from_hal {
                st.fix.sats_used = f[7].parse().unwrap_or(0);
            }
            st.fix.hdop = f[8].parse().unwrap_or(0.0);
            if st.fix.fix_quality > 0 {
                st.fix.alt_m = f[9].parse().unwrap_or(0.0);
            }
            st.fix.valid = st.fix.fix_quality > 0 || st.loc_fresh();
            true
        }
        "RMC" if f.len() >= 9 => {
            st.fix.utc = f[1].to_string();
            st.fix.valid = f[2] == "A" || st.loc_fresh();
            if let Some(la) = parse_coord(f[3], f[4]) {
                st.fix.lat = la;
            }
            if let Some(lo) = parse_coord(f[5], f[6]) {
                st.fix.lon = lo;
            }
            st.fix.speed_kn = f[7].parse().unwrap_or(0.0);
            st.fix.course_deg = f[8].parse().unwrap_or(0.0);
            true
        }
        "GSV" if f.len() >= 4 && !st.sv_from_hal => {
            // Satelliten in Sicht; Einträge in 4er-Gruppen ab Feld 4
            st.sats_in_view = f[3].parse().unwrap_or(0);
            let msg_num: usize = f[2].parse().unwrap_or(1);
            if msg_num == 1 {
                st.sats.clear();
            }
            let mut i = 4;
            while i + 3 < f.len() {
                let prn = f[i].parse().unwrap_or(0);
                if prn != 0 {
                    st.sats.push(SatInfo {
                        prn,
                        elevation: f[i + 1].parse().unwrap_or(0),
                        azimuth: f[i + 2].parse().unwrap_or(0),
                        snr: f[i + 3].parse().unwrap_or(-1),
                    });
                }
                i += 4;
            }
            true
        }
        _ => false,
    }
}

// ---------- Quelle: /dev/gnss_ipc (exynos_ipc) ----------
fn u16le(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

/// Öffnet /dev/gnss_ipc nichtblockierend. VERIFY: funktioniert erst, wenn der Kepler
/// gebootet ist (siehe A30-GNSS.md). Ohne Boot bleibt das Device leer.
pub fn open_gnss() -> std::io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_o_nonblock())
        .open(GNSS_DEV)
}

fn libc_o_nonblock() -> i32 {
    0o4000 // O_NONBLOCK
}

/// Liest verfügbare exynos_ipc-Frames und extrahiert NMEA-Nutzlast (der GNSS-Kanal
/// transportiert die NMEA-Sätze als Rahmen-Payload). Aktualisiert den Zustand.
/// VERIFY am Gerät: exakter Kanal/Payload-Aufbau – hier wird jeder druckbare
/// NMEA-Textanteil geparst, was für reine NMEA-Payloads genügt.
pub fn poll_frames(f: &mut File, st: &mut GnssState) -> std::io::Result<usize> {
    let mut buf = [0u8; 8192];
    let n = match f.read(&mut buf) {
        Ok(n) => n,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(0),
        Err(e) => return Err(e),
    };
    if n == 0 {
        return Ok(0);
    }
    let data = &buf[..n];
    let mut hits = 0;
    // 1) Als exynos_ipc-Frames interpretieren und Payload parsen …
    let mut i = 0;
    while i + EXYNOS_HEADER_SIZE <= data.len() {
        if u16le(data, i) == EXYNOS_START_MASK {
            let flen = u16le(data, i + EXYNOS_LEN_OFFSET) as usize;
            let _ch = data[i + EXYNOS_CH_ID_OFFSET];
            if flen >= EXYNOS_HEADER_SIZE && i + flen <= data.len() {
                let payload = &data[i + EXYNOS_HEADER_SIZE..i + flen];
                hits += parse_payload_text(st, payload);
                i += flen;
                continue;
            }
        }
        i += 1;
    }
    // 2) … und zusätzlich den gesamten Puffer nach NMEA durchsuchen (falls roh/ohne Rahmen).
    hits += parse_payload_text(st, data);
    Ok(hits)
}

fn parse_payload_text(st: &mut GnssState, payload: &[u8]) -> usize {
    let text = String::from_utf8_lossy(payload);
    let mut hits = 0;
    for line in text.split(['\r', '\n']) {
        if line.contains('$') {
            // ab dem '$' parsen
            if let Some(d) = line.find('$') {
                if feed_nmea_line(st, &line[d..]) {
                    hits += 1;
                }
            }
        }
    }
    hits
}

/// Setzt den GNSS-Prozessor zurück (nach Fehlversuch). VERIFY am Gerät.
pub fn reset() -> std::io::Result<()> {
    extern "C" {
        fn ioctl(fd: i32, req: u64, ...) -> i32;
    }
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(GNSS_DEV)?;
    unsafe {
        ioctl(f.as_raw_fd(), GNSS_IOCTL_RESET);
    }
    Ok(())
}

// ---------- einmaliger Komfortaufruf ----------
/// Einmal lesen (nichtblockierend). Liefert den aktuellen Zustand, falls das Device
/// existiert; sonst None. Für die Oberfläche: regelmäßig aufrufen und `st` behalten.
pub fn read_fix(st: &mut GnssState) -> Option<&GnssFix> {
    // Der Kepler gehört Samsungs gpsd (siehe A30-GNSS.md); /dev/gnss_ipc darf hier NICHT geöffnet
    // werden. Der Dienst `gnss` (tools/android-gnss) schreibt NMEA + $ULLOC/$ULSV nach NMEA_FILE und
    // schaltet GNSS nur ein, solange WANT_FILE frisch ist. Höchstens 2× pro Sekunde lesen.
    use std::sync::Mutex;
    static LAST: Mutex<Option<std::time::Instant>> = Mutex::new(None);
    let due = {
        let mut l = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let due = l.map_or(true, |t| {
            t.elapsed() >= std::time::Duration::from_millis(500)
        });
        if due {
            *l = Some(std::time::Instant::now());
        }
        due
    };
    if due {
        want();
        let _ = poll_file(st);
        if !st.loc_fresh() && st.last_loc.is_some() && st.fix.fix_quality == 0 {
            st.fix.valid = false;
        }
    }
    if st.fix.valid {
        Some(&st.fix)
    } else {
        None
    }
}

pub const NMEA_FILE: &str = "/run/ultar/gnss.nmea";
pub const WANT_FILE: &str = "/run/ultar/gnss.want";

/// Meldet dem GNSS-Dienst, dass gerade ein Standort gebraucht wird (Datei berühren).
pub fn want() {
    let _ = std::fs::create_dir_all("/run/ultar");
    let _ = std::fs::write(WANT_FILE, b"1");
}

/// Liest neue Zeilen aus NMEA_FILE ab `st.file_off`. Schrumpft die Datei (Dienst beginnt neu), wird
/// von vorn gelesen. Liefert die Zahl erkannter Sätze.
pub fn poll_file(st: &mut GnssState) -> std::io::Result<usize> {
    use std::io::{Seek, SeekFrom};
    let mut f = match File::open(NMEA_FILE) {
        Ok(f) => f,
        Err(e) => {
            st.service_up = false;
            return Err(e);
        }
    };
    let meta = f.metadata()?;
    st.service_up = meta
        .modified()
        .ok()
        .and_then(|m| m.elapsed().ok())
        .is_some_and(|d| d.as_secs() < 15);
    let len = meta.len();
    if len < st.file_off {
        st.file_off = 0;
    }
    if len == st.file_off {
        return Ok(0);
    }
    // nicht mehr als die letzten 64 KB auf einmal
    if len - st.file_off > 65536 {
        st.file_off = len - 65536;
    }
    f.seek(SeekFrom::Start(st.file_off))?;
    let mut buf = Vec::with_capacity((len - st.file_off) as usize);
    f.take(len - st.file_off).read_to_end(&mut buf)?;
    // nur vollständige Zeilen verarbeiten
    let end = buf.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    st.file_off += end as u64;
    Ok(parse_payload_text(st, &buf[..end]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nmea_gga() {
        let mut st = GnssState::default();
        let l = "$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47";
        assert!(feed_nmea_line(&mut st, l));
        assert!((st.fix.lat - 48.1173).abs() < 0.001);
        assert!((st.fix.lon - 11.5167).abs() < 0.001);
        assert_eq!(st.fix.sats_used, 8);
        assert!(st.fix.valid);
    }
    #[test]
    fn ultar_lines() {
        let mut st = GnssState::default();
        assert!(feed_nmea_line(
            &mut st,
            "$ULLOC,36.2021000,37.1343000,410.0,8.5,0.00,0.0,1795173021509"
        ));
        assert!(st.fix.valid);
        assert!((st.fix.lat - 36.2021).abs() < 1e-6);
        assert!((st.fix.acc_m - 8.5).abs() < 1e-3);
        assert!(feed_nmea_line(
            &mut st,
            "$ULSV,3,2,5:40:120:32:1,12:10:300:18:1,70:5:20:9:0"
        ));
        assert_eq!(st.sats.len(), 3);
        assert_eq!(st.fix.sats_used, 2);
        assert_eq!(st.sats[1].snr, 18);
        // leere GGA vom Dienst darf die frische Position nicht ungültig machen
        assert!(feed_nmea_line(&mut st, "$GPGGA,,,,,,0,,,,,,,,*66"));
        assert!(st.fix.valid);
    }
    #[test]
    fn ioctl_nums() {
        assert_eq!(GNSS_IOCTL_LOAD_FIRMWARE, 0x4b01);
        assert_eq!(GNSS_IOCTL_RELEASE_RESET, 0x4b08);
    }
}

// ---------- Oberfläche: Standort/GPS-Screen ----------
use crate::gfx::{rgb, Rect};
use crate::ui::{Al, Ui};

/// Zeichnet den GPS-Zustand (Position, Fix, Satelliten). Der Koordinator ruft das aus
/// einem Settings-Screen `Screen::Gnss` auf (siehe GNSS_INTEGRATION.md). Rein lesend.
pub fn render_gnss(ui: &mut Ui, y: &mut f32, st: &GnssState, t: impl Fn(&'static str) -> String) {
    ui.section(y, &t("Position"));
    let fixtxt = match st.fix.fix_quality {
        0 => t("No fix"),
        2 => t("DGPS fix"),
        _ => t("GPS fix"),
    };
    let fixcol = if st.fix.valid { ui.th.ok } else { ui.th.sub };
    ui.row(y, None, &t("Fix"), None, Some(&fixtxt), false);
    // farbiger Punkt für Fix-Status
    {
        let dot = Rect::new(
            ui.w - ui.dp(34.0),
            *y - ui.dp(54.0) + ui.dp(18.0),
            ui.dp(12.0),
            ui.dp(12.0),
        );
        ui.cv.rrect(dot, ui.dp(6.0), fixcol);
    }
    if st.fix.valid {
        ui.row(
            y,
            None,
            &t("Latitude"),
            None,
            Some(&format!("{:.6}°", st.fix.lat)),
            false,
        );
        ui.row(
            y,
            None,
            &t("Longitude"),
            None,
            Some(&format!("{:.6}°", st.fix.lon)),
            false,
        );
        ui.row(
            y,
            None,
            &t("Altitude"),
            None,
            Some(&format!("{:.0} m", st.fix.alt_m)),
            false,
        );
        if st.fix.acc_m > 0.0 {
            ui.row(
                y,
                None,
                &t("Accuracy"),
                None,
                Some(&format!("± {:.0} m", st.fix.acc_m)),
                false,
            );
        } else {
            ui.row(
                y,
                None,
                &t("Accuracy (HDOP)"),
                None,
                Some(&format!("{:.1}", st.fix.hdop)),
                false,
            );
        }
        if st.fix.speed_kn > 0.5 {
            ui.row(
                y,
                None,
                &t("Speed"),
                None,
                Some(&format!("{:.0} km/h", st.fix.speed_kn * 1.852)),
                false,
            );
        }
        if !st.fix.utc.is_empty() {
            ui.row(y, None, &t("Time (UTC)"), None, Some(&st.fix.utc), false);
        }
    } else {
        let c = ui.th.sub;
        let msg = if st.service_up {
            t("Waiting for satellites…")
        } else {
            t("Starting GPS service…")
        };
        ui.label(
            Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(24.0)),
            &msg,
            14.0,
            c,
            false,
            Al::Start,
        );
        *y += ui.dp(30.0);
        if st.service_up {
            ui.label(
                Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(24.0)),
                &t("First fix outdoors can take 1–2 minutes"),
                12.0,
                c,
                false,
                Al::Start,
            );
            *y += ui.dp(26.0);
        }
    }

    ui.section(
        y,
        &format!(
            "{} ({}/{})",
            t("Satellites"),
            st.fix.sats_used,
            st.sats_in_view
        ),
    );
    // Satelliten-Balken (SNR 0..50 dBHz)
    let bw = (ui.w - ui.dp(24.0)) / 16.0;
    let mut n = 0;
    for s in &st.sats {
        if n >= 16 {
            break;
        }
        let snr = s.snr.clamp(0, 50) as f32;
        let h = ui.dp(6.0) + (snr / 50.0) * ui.dp(60.0);
        let x = ui.dp(12.0) + n as f32 * bw;
        let base = *y + ui.dp(74.0);
        let col = if s.snr >= 30 {
            ui.th.ok
        } else if s.snr >= 15 {
            ui.th.accent
        } else {
            ui.th.sub
        };
        ui.cv.rrect(
            Rect::new(x + ui.dp(2.0), base - h, bw - ui.dp(4.0), h),
            ui.dp(2.0),
            col,
        );
        let lc = ui.th.sub;
        ui.label(
            Rect::new(x, base + ui.dp(2.0), bw, ui.dp(16.0)),
            &format!("{}", s.prn),
            9.0,
            lc,
            false,
            Al::Center,
        );
        n += 1;
    }
    if st.sats.is_empty() {
        let c = ui.th.sub;
        ui.label(
            Rect::new(ui.dp(22.0), *y, ui.w - ui.dp(44.0), ui.dp(24.0)),
            &t("No satellites in view"),
            13.0,
            c,
            false,
            Al::Start,
        );
    }
    *y += ui.dp(100.0);
    let _ = rgb; // genutzt falls erweitert
}
