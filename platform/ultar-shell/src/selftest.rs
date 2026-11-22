//! Hardware-Selbsttest – prüft die Subsysteme des Geräts auf einmal und meldet
//! pro Komponente OK / Warnung / Fehler mit Detailtext. Rein lesend über sysfs,
//! /proc, /dev und einige ioctls; verändert nichts am Gerät.
//!
//! Aufruf: `selftest::run()` liefert eine Liste von `Check`. Die Oberfläche (Screen::Hardware)
//! zeigt sie als Testbericht. Schlüssel der `comp`-Felder sind i18n-Schlüssel.

use nix::libc;
use std::fs;
use std::os::fd::AsRawFd;
use std::path::Path;

#[derive(Clone, Copy, PartialEq)]
pub enum Stat {
    Ok,
    Warn,
    Fail,
    Info,
}

#[derive(Clone)]
pub struct Check {
    /// i18n-Schlüssel der Komponente (siehe comp_label)
    pub comp: &'static str,
    pub stat: Stat,
    /// Kurzdetail (frei, nicht übersetzt – Werte/Namen vom Gerät)
    pub detail: String,
}

impl Check {
    fn ok(comp: &'static str, d: impl Into<String>) -> Self {
        Check { comp, stat: Stat::Ok, detail: d.into() }
    }
    fn warn(comp: &'static str, d: impl Into<String>) -> Self {
        Check { comp, stat: Stat::Warn, detail: d.into() }
    }
    fn fail(comp: &'static str, d: impl Into<String>) -> Self {
        Check { comp, stat: Stat::Fail, detail: d.into() }
    }
    fn info(comp: &'static str, d: impl Into<String>) -> Self {
        Check { comp, stat: Stat::Info, detail: d.into() }
    }
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

fn human(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < 4 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{} {}", bytes, U[0])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

// ---------- ioctl-Nummern (asm-generic) ----------
const fn ioc(dir: u32, typ: u32, nr: u32, size: u32) -> libc::c_ulong {
    ((dir << 30) | (size << 16) | (typ << 8) | nr) as libc::c_ulong
}
const IOC_READ: u32 = 2;

// ---------- Hauptlauf ----------
pub fn run() -> Vec<Check> {
    let mut v = Vec::new();
    v.push(storage());
    v.extend(display());
    v.push(keys());
    v.push(audio());
    v.push(cameras());
    v.push(modem_sim());
    v.push(wifi());
    v.push(bluetooth());
    v.push(sensors());
    v.push(battery());
    v.push(vibrator());
    v
}

// ---------- Speicher ----------
/// Interner Hauptspeicher (eMMC) – Gesamtgröße, Partitionen, /data-Einbindung.
fn storage() -> Check {
    // Blockgrößen: /sys/block/*/size in 512-Byte-Sektoren
    let mut total = 0u64;
    let mut emmc = None;
    if let Ok(rd) = fs::read_dir("/sys/block") {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with("mmcblk") && !name.contains('p') && !name.contains("boot") && !name.contains("rpmb") {
                let sect: u64 = read(&format!("/sys/block/{name}/size")).parse().unwrap_or(0);
                let bytes = sect * 512;
                if bytes > total {
                    total = bytes;
                    emmc = Some(name.clone());
                }
            }
        }
    }
    let Some(dev) = emmc else {
        return Check::fail("st_storage", "keine eMMC gefunden");
    };
    // größte Partition (userdata) suchen
    let mut biggest = (String::new(), 0u64);
    if let Ok(rd) = fs::read_dir(format!("/sys/block/{dev}")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if n.starts_with(&dev) && n.contains('p') {
                let sect: u64 = read(&format!("/sys/block/{dev}/{n}/size")).parse().unwrap_or(0);
                let b = sect * 512;
                if b > biggest.1 {
                    biggest = (n, b);
                }
            }
        }
    }
    // /data eingebunden?
    let mounts = read("/proc/mounts");
    let data_mounted = mounts.lines().any(|l| l.split_whitespace().nth(1) == Some("/data"));
    let mut detail = format!("{dev}: {} gesamt", human(total));
    if !biggest.0.is_empty() {
        detail += &format!(", größte Part. {} {}", biggest.0, human(biggest.1));
    }
    if data_mounted {
        if let Some((tot, free)) = disk_free("/data") {
            detail += &format!(", /data {} frei von {}", human(free), human(tot));
        } else {
            detail += ", /data eingebunden";
        }
        Check::ok("st_storage", detail)
    } else {
        detail += ", /data NICHT eingebunden";
        Check::warn("st_storage", detail)
    }
}

fn disk_free(path: &str) -> Option<(u64, u64)> {
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    let c = std::ffi::CString::new(path).ok()?;
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let bs = s.f_frsize as u64;
    Some((s.f_blocks as u64 * bs, s.f_bavail as u64 * bs))
}

// ---------- Display ----------
fn display() -> Vec<Check> {
    let mut v = Vec::new();
    if Path::new("/dev/fb0").exists() {
        // Auflösung aus /sys/class/graphics/fb0/virtual_size
        let res = read("/sys/class/graphics/fb0/virtual_size");
        let res = if res.is_empty() { "fb0".to_string() } else { res.replace(',', "x") };
        v.push(Check::ok("st_display", format!("fb0 {res}")));
    } else {
        v.push(Check::fail("st_display", "kein /dev/fb0"));
    }
    if Path::new("/dev/ion").exists() {
        v.push(Check::ok("st_gpu", "ION-Puffer vorhanden (DECON)"));
    } else {
        v.push(Check::warn("st_gpu", "kein /dev/ion – nur fbdev"));
    }
    v
}

// ---------- Tasten ----------
/// Eingabegeräte durchgehen und prüfen, ob Lautstärke- und Power-Taste gemeldet werden.
fn keys() -> Check {
    let mut names = Vec::new();
    let mut vol_up = false;
    let mut vol_dn = false;
    let mut power = false;
    let mut count = 0;
    if let Ok(rd) = fs::read_dir("/dev/input") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.starts_with("event") {
                continue;
            }
            count += 1;
            let Ok(f) = fs::File::open(e.path()) else { continue };
            let fd = f.as_raw_fd();
            // Name
            let mut buf = [0u8; 128];
            let r = unsafe { libc::ioctl(fd, ioc(IOC_READ, b'E' as u32, 0x06, 128), buf.as_mut_ptr()) };
            if r > 0 {
                let nm = String::from_utf8_lossy(&buf[..(r as usize).min(128)]).trim_end_matches('\0').trim().to_string();
                if !nm.is_empty() {
                    names.push(nm);
                }
            }
            // Key-Bitmap (EV_KEY=1 -> nr 0x20+1)
            let mut keymap = [0u8; 96]; // deckt bis KEY-Code ~767 ab
            let r = unsafe { libc::ioctl(fd, ioc(IOC_READ, b'E' as u32, 0x21, 96), keymap.as_mut_ptr()) };
            if r > 0 {
                let test = |code: usize| (keymap[code / 8] >> (code % 8)) & 1 == 1;
                if test(115) {
                    vol_up = true;
                }
                if test(114) {
                    vol_dn = true;
                }
                if test(116) {
                    power = true;
                }
            }
        }
    }
    if count == 0 {
        return Check::fail("st_keys", "keine /dev/input/event*");
    }
    let mut found = Vec::new();
    if vol_up {
        found.push("Lauter");
    }
    if vol_dn {
        found.push("Leiser");
    }
    if power {
        found.push("Power");
    }
    let detail = format!("{count} Eingabegeräte; Tasten: {}", if found.is_empty() { "keine erkannt".to_string() } else { found.join(", ") });
    if vol_up && vol_dn && power {
        Check::ok("st_keys", detail)
    } else if found.is_empty() {
        Check::fail("st_keys", format!("{detail} ({})", names.join(", ")))
    } else {
        Check::warn("st_keys", detail)
    }
}

// ---------- Audio ----------
fn audio() -> Check {
    let cards = read("/proc/asound/cards");
    let has_snd = Path::new("/dev/snd").exists();
    let pcm = fs::read_dir("/dev/snd").map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("pcmC")).count()).unwrap_or(0);
    if !has_snd || cards.is_empty() || cards.contains("no soundcards") {
        return Check::fail("st_audio", "keine Soundkarte (/proc/asound leer)");
    }
    let first = cards.lines().find(|l| l.contains('[')).map(|l| l.trim()).unwrap_or("Soundkarte");
    Check::ok("st_audio", format!("{first}; {pcm} PCM-Geräte"))
}

// ---------- Kameras ----------
fn cameras() -> Check {
    let mut cams = Vec::new();
    let mut blocked = 0u32;
    if let Ok(rd) = fs::read_dir("/dev") {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().into_owned();
            if !n.starts_with("video") {
                continue;
            }
            // WICHTIG: /dev/video*-Knoten auf Exynos (MFC-Codec, ISP) blockieren beim open()
            // teils 60 s im Kernel (request_firmware). Deshalb jeden Knoten in einem Kindprozess
            // mit kurzem Timeout prüfen – blockiert er, killen wir das Kind, der UI-Thread bleibt frei.
            match probe_video(&e.path()) {
                CamProbe::Capture(card) => cams.push(format!("{n}:{card}")),
                CamProbe::Blocked => blocked += 1,
                CamProbe::None => {}
            }
        }
    }
    if !cams.is_empty() {
        Check::ok("st_camera", format!("{} Kamera(s): {}", cams.len(), cams.join(", ")))
    } else if blocked > 0 {
        Check::warn("st_camera", format!("{blocked} Video-Knoten blockieren beim Öffnen (Firmware fehlt)"))
    } else {
        Check::fail("st_camera", "keine Kamera (/dev/video*) mit Capture")
    }
}

enum CamProbe {
    Capture(String),
    Blocked,
    None,
}

/// Einen Video-Knoten gefahrlos prüfen: open()+QUERYCAP in einem Kindprozess, höchstens ~1 s warten.
/// Hängt das Kind (Exynos-Firmware), wird es gekillt und der Knoten als „blockiert" gewertet.
fn probe_video(path: &std::path::Path) -> CamProbe {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return CamProbe::None };
    let mut fds = [0i32; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return CamProbe::None;
    }
    let (rd, wr) = (fds[0], fds[1]);
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        // Kind: nur async-signal-sichere Aufrufe, keine Rust-Allokation
        unsafe { libc::close(rd) };
        let fd = unsafe { libc::open(c.as_ptr(), libc::O_RDONLY | libc::O_NONBLOCK) };
        if fd >= 0 {
            let mut cap = [0u8; 104];
            let r = unsafe { libc::ioctl(fd, ioc(IOC_READ, b'V' as u32, 0, 104), cap.as_mut_ptr()) };
            if r == 0 {
                let caps = u32::from_ne_bytes([cap[84], cap[85], cap[86], cap[87]]);
                if caps & 0x1 != 0 {
                    // 1 Byte Marker + 32 Byte Kartenname
                    let mut out = [0u8; 33];
                    out[0] = 1;
                    out[1..].copy_from_slice(&cap[16..48]);
                    unsafe { libc::write(wr, out.as_ptr() as *const _, 33) };
                }
            }
            unsafe { libc::close(fd) };
        }
        unsafe { libc::_exit(0) };
    }
    if pid < 0 {
        unsafe {
            libc::close(rd);
            libc::close(wr);
        }
        return CamProbe::None;
    }
    unsafe { libc::close(wr) };
    let mut buf = [0u8; 33];
    let mut got = 0usize;
    let mut pfd = libc::pollfd { fd: rd, events: libc::POLLIN, revents: 0 };
    let mut waited = 0;
    let mut alive = true;
    while waited < 1000 {
        let pr = unsafe { libc::poll(&mut pfd, 1, 100) };
        if pr > 0 {
            let nread = unsafe { libc::read(rd, buf.as_mut_ptr() as *mut _, 33) };
            if nread > 0 {
                got = nread as usize;
            }
            alive = false;
            break;
        }
        let mut st = 0;
        if unsafe { libc::waitpid(pid, &mut st, libc::WNOHANG) } == pid {
            alive = false;
            break;
        }
        waited += 100;
    }
    unsafe { libc::close(rd) };
    if alive {
        unsafe {
            libc::kill(pid, libc::SIGKILL);
            let mut st = 0;
            libc::waitpid(pid, &mut st, 0);
        }
        return CamProbe::Blocked;
    }
    let mut st = 0;
    unsafe { libc::waitpid(pid, &mut st, 0) };
    if got >= 1 && buf[0] == 1 {
        let card = String::from_utf8_lossy(&buf[1..got]).trim_end_matches('\0').trim().to_string();
        CamProbe::Capture(card)
    } else {
        CamProbe::None
    }
}

// ---------- SIM / Modem ----------
fn modem_sim() -> Check {
    // Exynos/Shannon-Modem: /dev/umts_* ; cpif; RIL-Sockets ; rmnet-Netz
    let modem_nodes: Vec<String> = fs::read_dir("/dev")
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n.starts_with("umts_") || n.starts_with("cpif") || n.starts_with("rmnet") || n.contains("modem"))
                .collect()
        })
        .unwrap_or_default();
    let rmnet = fs::read_dir("/sys/class/net").map(|rd| rd.flatten().any(|e| e.file_name().to_string_lossy().starts_with("rmnet"))).unwrap_or(false);
    let ril = Path::new("/dev/socket/rild").exists() || Path::new("/dev/umts_ipc0").exists();
    if modem_nodes.is_empty() && !rmnet {
        return Check::fail("st_modem", "kein Modem-Knoten (umts_/cpif/rmnet) – Treiber fehlt");
    }
    let mut detail = format!("Knoten: {}", if modem_nodes.is_empty() { "–".to_string() } else { modem_nodes.join(", ") });
    if rmnet {
        detail += "; rmnet vorhanden";
    }
    if ril {
        detail += "; IPC bereit";
    }
    // SIM-Status kann erst der Modem-Dienst lesen; hier nur Hardware-Präsenz
    Check::warn("st_modem", format!("{detail} (SIM-Status braucht Modem-Dienst)"))
}

// ---------- WLAN ----------
fn wifi() -> Check {
    let Some(ifc) = crate::sys::wifi_iface() else {
        return Check::fail("st_wifi", "keine WLAN-Schnittstelle (phy80211)");
    };
    let mac = read(&format!("/sys/class/net/{ifc}/address"));
    let tools = crate::sys::wifi_tools();
    let valid_mac = !mac.is_empty() && mac != "00:00:00:00:00:00";
    let mut detail = format!("{ifc} MAC {}", if mac.is_empty() { "?".into() } else { mac });
    detail += if tools { "; Programme ✓" } else { "; Programme fehlen" };
    let state = crate::sys::wifi_state();
    match &state {
        Some(s) => detail += &format!("; wpa_supplicant {s}"),
        None => detail += "; wpa_supplicant nicht erreichbar",
    }
    if tools && valid_mac && state.is_some() {
        Check::ok("st_wifi", detail)
    } else {
        Check::warn("st_wifi", detail)
    }
}

// ---------- Bluetooth ----------
fn bluetooth() -> Check {
    let hci = fs::read_dir("/sys/class/bluetooth").map(|rd| rd.flatten().count()).unwrap_or(0);
    let scsc_bt = Path::new("/dev/scsc_bt").exists() || read("/proc/modules").contains("scsc_bt");
    if hci > 0 {
        Check::ok("st_bt", format!("{hci} HCI-Gerät(e)"))
    } else if scsc_bt {
        Check::warn("st_bt", "BT-Treiber geladen, aber kein HCI aktiv")
    } else {
        Check::fail("st_bt", "kein Bluetooth-Gerät")
    }
}

// ---------- Sensoren ----------
fn sensors() -> Check {
    let iio = fs::read_dir("/sys/bus/iio/devices").map(|rd| rd.flatten().filter(|e| e.file_name().to_string_lossy().starts_with("iio:device")).count()).unwrap_or(0);
    let sens = fs::read_dir("/sys/class/sensors").map(|rd| rd.flatten().count()).unwrap_or(0);
    let n = iio + sens;
    if n > 0 {
        Check::ok("st_sensors", format!("{n} Sensor(en) (iio:{iio}, class:{sens})"))
    } else {
        Check::warn("st_sensors", "keine Sensoren gefunden")
    }
}

// ---------- Akku ----------
fn battery() -> Check {
    for p in ["battery", "Battery"] {
        let base = format!("/sys/class/power_supply/{p}");
        if Path::new(&base).exists() {
            let cap = read(&format!("{base}/capacity"));
            let stat = read(&format!("{base}/status"));
            let volt: f64 = read(&format!("{base}/voltage_now")).parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
            return Check::ok("st_battery", format!("{cap}% {stat} {volt:.2}V"));
        }
    }
    Check::warn("st_battery", "kein Akku-Sensor")
}

// ---------- Vibration ----------
fn vibrator() -> Check {
    let paths = ["/sys/class/timed_output/vibrator", "/sys/class/leds/vibrator"];
    for p in paths {
        if Path::new(p).exists() {
            return Check::ok("st_vibra", "Vibrationsmotor vorhanden");
        }
    }
    Check::warn("st_vibra", "kein Vibrationsknoten")
}
