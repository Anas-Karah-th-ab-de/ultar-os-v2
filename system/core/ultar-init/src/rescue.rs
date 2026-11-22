//! Notausstieg bei hängendem System – läuft in PID 1 und damit unabhängig von der Oberfläche.
//!
//! **Tasten (Laut + Leise gleichzeitig halten):**
//! | gehalten | Rückmeldung      | Aktion beim Loslassen |
//! |----------|------------------|------------------------|
//! | ≥ 2 s    | 1× kurz vibrieren | Stufe 1: Oberfläche (und Browser-Sitzung) hart beenden → init startet sie neu |
//! | ≥ 6 s    | 2× vibrieren      | Stufe 2: alle Dienste hart beenden → init startet sie neu |
//! | ≥ 10 s   | lang vibrieren    | Stufe 3 sofort: Daten sichern (sync) und Neustart |
//!
//! Zusätzlich (Hardware, ohne Ultar): Leiser + Ein/Aus ~10 s = Zwangsneustart durch Samsung-PMIC/Bootloader –
//! hilft auch, wenn der Kernel selbst steht.
//!
//! **Automatik (Wachhund der Oberfläche):** `ultar-shell` schreibt bei jedem Schleifendurchlauf
//! `/run/ultar/ui.alive`. Ist die Datei älter als [`UI_STALE`], gilt die Oberfläche als hängend und wird wie
//! Stufe 1 beendet. Hängende Kernel-Aufrufe (Zustand D, typisch Treiber/Firmware) werden protokolliert:
//! `/run/ultar/rescue.log` und dauerhaft `/data/ultar/rescue.log`.
//!
//! Steuerbar auch per `ultar-ctl rescue 1|2|3` bzw. über die USB-Tür.

use std::fs;
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::{Duration, Instant};

const KEY_VOLUMEDOWN: u16 = 114;
const KEY_VOLUMEUP: u16 = 115;
const L1: Duration = Duration::from_secs(2);
const L2: Duration = Duration::from_secs(6);
const L3: Duration = Duration::from_secs(10);
/// Oberfläche schläft im Leerlauf bis zu 60 s – doppelt so lang plus Reserve gilt als Hänger.
const UI_STALE: Duration = Duration::from_secs(150);
pub const UI_ALIVE: &str = "/run/ultar/ui.alive";

/// Programme der Vordergrund-Sitzung (comm, vom Kernel auf 15 Zeichen gekürzt).
const UI_PROCS: &[&str] = &["ultar-shell", "weston", "epiphany", "WebKitWebProces", "WebKitNetworkPr", "librespot"];

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Ui = 1,
    Services = 2,
    Reboot = 3,
}

impl Level {
    pub fn from_arg(s: &str) -> Option<Level> {
        match s {
            "1" | "ui" => Some(Level::Ui),
            "2" | "dienste" | "services" => Some(Level::Services),
            "3" | "reboot" => Some(Level::Reboot),
            _ => None,
        }
    }
}

/// Startet Tasten-Überwachung und Wachhund der Oberfläche (nur als PID 1 aufrufen).
pub fn start(vib: String) {
    let v = vib.clone();
    let _ = std::thread::Builder::new().name("rescue-keys".into()).spawn(move || keys_loop(&v));
    let _ = std::thread::Builder::new().name("ui-watchdog".into()).spawn(move || watchdog_loop(&vib));
}

// ------------------------------------------------------------------ Aktionen

/// Führt eine Rettungsstufe aus. Liefert eine Kurzmeldung für Protokoll/Steuerung.
pub fn act(level: Level, why: &str, vib: &str) -> String {
    let stuck = stuck_report();
    record(&format!("Stufe {} ({why})", level as u8), &stuck);
    match level {
        Level::Ui => {
            let n = kill_comm(UI_PROCS);
            notify(&texts().0, &texts().1);
            format!("Oberfläche beendet ({n} Prozesse), Neustart folgt\n")
        }
        Level::Services => {
            // trifft alle Prozesse außer PID 1 (und deren Threads wie Tür/Wachhund) – der Supervisor startet neu
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(-1), nix::sys::signal::Signal::SIGKILL);
            notify(&texts().2, &texts().3);
            "alle Dienste beendet, Neustart folgt\n".into()
        }
        Level::Reboot => {
            crate::hw::vibrate(vib, 500);
            hard_reboot();
        }
    }
}

/// sync mit Zeitlimit (hängende Speichertreiber dürfen den Neustart nicht verhindern), dann Neustart.
fn hard_reboot() -> ! {
    crate::log("Notausstieg: Neustart");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        nix::unistd::sync();
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_secs(4));
    // Notfall-Schreibschutz der Dateisysteme (falls Magic-SysRq im Kernel ist) – danach ist ext4 sauber
    let _ = fs::write("/proc/sysrq-trigger", "u");
    std::thread::sleep(Duration::from_millis(500));
    let _ = nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT);
    let _ = fs::write("/proc/sysrq-trigger", "b");
    loop {
        std::thread::sleep(Duration::from_secs(3600));
    }
}

fn kill_comm(names: &[&str]) -> usize {
    let mut n = 0;
    for (pid, comm) in procs() {
        if names.iter().any(|w| comm == *w) {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::SIGKILL);
            n += 1;
        }
    }
    n
}

fn procs() -> Vec<(i32, String)> {
    let Ok(rd) = fs::read_dir("/proc") else { return Vec::new() };
    rd.flatten()
        .filter_map(|e| {
            let pid: i32 = e.file_name().to_str()?.parse().ok()?;
            (pid > 1).then(|| (pid, fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().to_string()))
        })
        .collect()
}

/// Zustand eines Prozesses aus /proc/<pid>/stat (R, S, D, Z …).
fn state(pid: i32) -> char {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
    s.rfind(") ").and_then(|i| s[i + 2..].chars().next()).unwrap_or('?')
}

/// Alle Threads von Programmen (keine Kernel-Threads) im Zustand D (unterbrechungsfreies Warten im Kernel) mit Kernel-Funktion – zeigt, welcher
/// Treiber hängt. Solche Prozesse lassen sich auch mit SIGKILL erst beenden, wenn der Treiber zurückkehrt.
pub fn stuck_report() -> Vec<String> {
    let mut out = Vec::new();
    for (pid, comm) in procs() {
        // Kernel-Threads (leere cmdline) warten oft regulär im Zustand D – kein Hänger
        if fs::read(format!("/proc/{pid}/cmdline")).map_or(true, |c| c.is_empty()) {
            continue;
        }
        let Ok(tasks) = fs::read_dir(format!("/proc/{pid}/task")) else { continue };
        for t in tasks.flatten() {
            let Some(tid) = t.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
            let st = fs::read_to_string(format!("/proc/{pid}/task/{tid}/stat")).unwrap_or_default();
            let c = st.rfind(") ").and_then(|i| st[i + 2..].chars().next()).unwrap_or('?');
            if c == 'D' {
                let wchan = fs::read_to_string(format!("/proc/{pid}/task/{tid}/wchan")).unwrap_or_default();
                out.push(format!("{comm} pid {pid} tid {tid}: D in {}", wchan.trim()));
            }
        }
    }
    out
}

fn record(what: &str, stuck: &[String]) {
    let mut s = format!("[{:.1}] Notausstieg {what}\n", uptime());
    if stuck.is_empty() {
        s.push_str("  kein Prozess hängt im Kernel\n");
    }
    for l in stuck {
        s.push_str(&format!("  {l}\n"));
    }
    crate::log(&format!("Notausstieg {what}; {} hängende Kernel-Aufrufe", stuck.len()));
    for p in ["/run/ultar/rescue.log", "/data/ultar/rescue.log"] {
        if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = f.write_all(s.as_bytes());
        }
    }
}

fn uptime() -> f64 {
    fs::read_to_string("/proc/uptime").ok().and_then(|s| s.split_whitespace().next()?.parse().ok()).unwrap_or(0.0)
}

/// Texte in der Systemsprache (aus /data/ultar/settings.toml).
fn texts() -> (String, String, String, String) {
    let set = fs::read_to_string("/data/ultar/settings.toml").unwrap_or_default();
    let lang = set.lines().find_map(|l| l.trim().strip_prefix("lang")).map(|r| r.trim_start_matches([' ', '=']).trim_matches(['"', ' '])).unwrap_or("ar");
    let t = |a: &str, b: &str, c: &str, d: &str| (a.to_string(), b.to_string(), c.to_string(), d.to_string());
    match lang {
        "de" => t("Notausstieg", "Oberfläche wurde neu gestartet", "Notausstieg", "Alle Dienste wurden neu gestartet"),
        "en" => t("Emergency exit", "Interface was restarted", "Emergency exit", "All services were restarted"),
        _ => t("مخرج الطوارئ", "تمت إعادة تشغيل الواجهة", "مخرج الطوارئ", "تمت إعادة تشغيل جميع الخدمات"),
    }
}

fn notify(title: &str, body: &str) {
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let _ = fs::create_dir_all("/run/ultar");
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open("/run/ultar/notify.log") {
        let _ = f.write_all(format!("{ts}\tnormal\tsystem\tultar-init\t{title}\t{body}\n").as_bytes());
    }
}

// ------------------------------------------------------------------ Tasten

extern "C" {
    fn ioctl(fd: i32, req: u64, ...) -> i32;
}

/// Hat das Gerät eine der Lautstärketasten? (EVIOCGBIT(EV_KEY))
fn has_vol_keys(f: &fs::File) -> bool {
    let mut bits = [0u8; 96];
    let req = 0x8000_0000u64 | ((bits.len() as u64) << 16) | (0x45 << 8) | (0x20 + 1);
    let r = unsafe { ioctl(f.as_raw_fd(), req, bits.as_mut_ptr()) };
    let bit = |k: u16| bits[(k / 8) as usize] & (1 << (k % 8)) != 0;
    r >= 0 && (bit(KEY_VOLUMEUP) || bit(KEY_VOLUMEDOWN))
}

fn open_key_devs() -> Vec<fs::File> {
    let Ok(rd) = fs::read_dir("/dev/input") else { return Vec::new() };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
        .filter_map(|e| fs::OpenOptions::new().read(true).custom_flags(0o4000 | 0o2000000).open(e.path()).ok())
        .filter(has_vol_keys)
        .collect()
}

/// Zustandsautomat der Tastenkombination – getrennt von der Ein-/Ausgabe, damit testbar.
#[derive(Default)]
pub struct Combo {
    up: bool,
    down: bool,
    since: Option<Instant>,
    armed: u8,
    quiet_until: Option<Instant>,
}

pub enum Step {
    None,
    /// Schwelle erreicht → so oft vibrieren
    Armed(u8),
    Act(Level),
}

impl Combo {
    pub fn key(&mut self, code: u16, pressed: bool, now: Instant) -> Step {
        match code {
            KEY_VOLUMEUP => self.up = pressed,
            KEY_VOLUMEDOWN => self.down = pressed,
            _ => return Step::None,
        }
        if self.quiet_until.is_some_and(|q| now < q) {
            self.since = None;
            return Step::None;
        }
        if self.up && self.down {
            self.since.get_or_insert(now);
            return Step::None;
        }
        // eine Taste losgelassen → erreichte Stufe ausführen
        self.since = None;
        let a = std::mem::take(&mut self.armed);
        let lvl = match a {
            1 => Some(Level::Ui),
            2 => Some(Level::Services),
            _ => None,
        };
        match lvl {
            Some(l) => {
                self.quiet_until = Some(now + Duration::from_secs(3));
                Step::Act(l)
            }
            None => Step::None,
        }
    }

    pub fn tick(&mut self, now: Instant) -> Step {
        let Some(s) = self.since else { return Step::None };
        let held = now - s;
        if held >= L3 {
            self.since = None;
            self.armed = 0;
            return Step::Act(Level::Reboot);
        }
        let want = if held >= L2 { 2 } else if held >= L1 { 1 } else { 0 };
        if want > self.armed {
            self.armed = want;
            return Step::Armed(want);
        }
        Step::None
    }

    pub fn active(&self) -> bool {
        self.since.is_some()
    }
}

fn keys_loop(vib: &str) {
    use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
    use std::os::fd::AsFd;
    let mut devs = open_key_devs();
    let mut last_scan = Instant::now();
    let mut combo = Combo::default();
    crate::log(&format!("Notausstieg: {} Gerät(e) mit Lautstärketasten überwacht (Laut+Leise halten)", devs.len()));
    let mut buf = [0u8; 24 * 32];
    loop {
        if devs.is_empty() || last_scan.elapsed() > Duration::from_secs(if devs.is_empty() { 5 } else { 60 }) {
            if !combo.active() {
                devs = open_key_devs();
                last_scan = Instant::now();
            }
        }
        let timeout = if combo.active() { 100u16 } else { 5000 };
        if devs.is_empty() {
            std::thread::sleep(Duration::from_millis(timeout as u64));
            continue;
        }
        let mut fds: Vec<PollFd> = devs.iter().map(|d| PollFd::new(d.as_fd(), PollFlags::POLLIN)).collect();
        let _ = poll(&mut fds, PollTimeout::from(timeout));
        drop(fds);
        let now = Instant::now();
        let mut steps = Vec::new();
        for d in devs.iter_mut() {
            while let Ok(n) = d.read(&mut buf) {
                if n == 0 {
                    break;
                }
                for ev in buf[..n].chunks_exact(24) {
                    let ty = u16::from_ne_bytes([ev[16], ev[17]]);
                    let code = u16::from_ne_bytes([ev[18], ev[19]]);
                    let val = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    if ty == 1 && val != 2 {
                        steps.push(combo.key(code, val == 1, now));
                    }
                }
            }
        }
        steps.push(combo.tick(now));
        for s in steps {
            match s {
                Step::None => {}
                Step::Armed(n) => {
                    for i in 0..n {
                        if i > 0 {
                            std::thread::sleep(Duration::from_millis(180));
                        }
                        crate::hw::vibrate(vib, 90);
                    }
                }
                Step::Act(l) => {
                    let m = act(l, "Tasten Laut+Leise", vib);
                    crate::log(&format!("Notausstieg: {}", m.trim()));
                }
            }
        }
    }
}

// ------------------------------------------------------------------ Wachhund der Oberfläche

fn watchdog_loop(vib: &str) {
    loop {
        std::thread::sleep(Duration::from_secs(15));
        // Nur wenn die laufende Oberfläche das Lebenszeichen kennt (alte Fassungen schreiben keins)
        let Ok(m) = fs::metadata(UI_ALIVE).and_then(|m| m.modified()) else { continue };
        let age = m.elapsed().unwrap_or_default();
        if age < UI_STALE {
            continue;
        }
        let Some((pid, _)) = procs().into_iter().find(|(_, c)| c == "ultar-shell") else { continue };
        // Läuft die Oberfläche gerade erst? Dann ist die alte Datei vom Vorgänger.
        if proc_age(pid).is_some_and(|a| a < UI_STALE) {
            continue;
        }
        let st = state(pid);
        crate::log(&format!("Wachhund: Oberfläche (pid {pid}, Zustand {st}) seit {} s ohne Lebenszeichen", age.as_secs()));
        crate::hw::vibrate(vib, 60);
        act(Level::Ui, &format!("Wachhund, {} s ohne Lebenszeichen, Zustand {st}", age.as_secs()), vib);
        // Datei auffrischen, damit erst die neue Oberfläche wieder bewertet wird
        let _ = fs::write(UI_ALIVE, b"restart");
    }
}

/// Laufzeit eines Prozesses (Startzeit aus /proc/<pid>/stat, Feld 22, in Ticks).
fn proc_age(pid: i32) -> Option<Duration> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let rest = &s[s.rfind(") ")? + 2..];
    let start: f64 = rest.split_whitespace().nth(19)?.parse().ok()?;
    let hz = unsafe { nix::libc::sysconf(nix::libc::_SC_CLK_TCK) } as f64;
    Some(Duration::from_secs_f64((uptime() - start / hz.max(1.0)).max(0.0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combo_levels() {
        let t0 = Instant::now();
        let mut c = Combo::default();
        assert!(matches!(c.key(KEY_VOLUMEUP, true, t0), Step::None));
        assert!(matches!(c.key(KEY_VOLUMEDOWN, true, t0), Step::None));
        assert!(c.active());
        assert!(matches!(c.tick(t0 + Duration::from_millis(1500)), Step::None));
        assert!(matches!(c.tick(t0 + Duration::from_millis(2100)), Step::Armed(1)));
        assert!(matches!(c.tick(t0 + Duration::from_millis(2200)), Step::None));
        assert!(matches!(c.key(KEY_VOLUMEUP, false, t0 + Duration::from_secs(3)), Step::Act(Level::Ui)));
        // kurz danach: Sperrzeit, nichts passiert
        c.key(KEY_VOLUMEUP, true, t0 + Duration::from_secs(4));
        assert!(!c.active());
    }

    #[test]
    fn combo_services_and_reboot() {
        let t0 = Instant::now();
        let mut c = Combo::default();
        c.key(KEY_VOLUMEUP, true, t0);
        c.key(KEY_VOLUMEDOWN, true, t0);
        c.tick(t0 + Duration::from_secs(3));
        assert!(matches!(c.tick(t0 + Duration::from_secs(7)), Step::Armed(2)));
        assert!(matches!(c.key(KEY_VOLUMEDOWN, false, t0 + Duration::from_secs(8)), Step::Act(Level::Services)));
        let t1 = t0 + Duration::from_secs(20);
        c.key(KEY_VOLUMEUP, false, t1);
        c.key(KEY_VOLUMEUP, true, t1);
        c.key(KEY_VOLUMEDOWN, true, t1);
        assert!(matches!(c.tick(t1 + Duration::from_secs(10)), Step::Act(Level::Reboot)));
    }

    #[test]
    fn single_key_or_short_press_does_nothing() {
        let t0 = Instant::now();
        let mut c = Combo::default();
        c.key(KEY_VOLUMEUP, true, t0);
        assert!(matches!(c.tick(t0 + Duration::from_secs(12)), Step::None));
        c.key(KEY_VOLUMEDOWN, true, t0 + Duration::from_secs(12));
        assert!(matches!(c.key(KEY_VOLUMEDOWN, false, t0 + Duration::from_secs(13)), Step::None));
    }
}
