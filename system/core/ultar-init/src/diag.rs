//! Diagnose über USB für Geräte ohne Konsole (Handy).
//!
//! Meldet das Gerät am PC als serielle Schnittstelle „Ultar OS“ an (USB-Gadget, Funktion ACM) und schreibt
//! dort alle paar Sekunden einen Bericht hinein: Protokoll von ultar-init, Bildschirm- und Eingabegeräte,
//! Kernel-Meldungen, Protokoll der Oberfläche. Ultar Flash liest ihn mit „Diagnose vom Handy lesen“.
//! So lässt sich auch bei dunklem Bildschirm feststellen, ob und wie weit Ultar OS gestartet ist.

use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Mutex;

use crate::hw::{self, Case};

pub const VID: &str = "0x1209";
pub const PID: &str = "0x0001";
static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// Zeile des init-Protokolls merken (wird im Bericht mitgeschickt).
pub fn remember(line: &str) {
    if let Ok(mut l) = LOG.lock() {
        if l.len() < 400 {
            l.push(line.trim_end().to_string());
        }
    }
}

fn w(path: &str, v: &str) -> bool {
    fs::write(path, v).is_ok()
}

/// USB-Fall „configfs“: neuere Kernel. Meldet das Gerät als serielle Schnittstelle (ACM) an.
fn setup_configfs() -> bool {
    let _ = fs::create_dir_all("/sys/kernel/config");
    let _ = nix::mount::mount(
        Some("configfs"),
        "/sys/kernel/config",
        Some("configfs"),
        nix::mount::MsFlags::empty(),
        None::<&str>,
    );
    let g = "/sys/kernel/config/usb_gadget/ultar";
    if fs::create_dir_all(format!("{g}/strings/0x409")).is_err() {
        return false; // kein USB-Gadget im Kernel
    }
    w(&format!("{g}/idVendor"), VID);
    w(&format!("{g}/idProduct"), PID);
    w(&format!("{g}/strings/0x409/manufacturer"), "KM Code Craft");
    w(&format!("{g}/strings/0x409/product"), "Ultar OS");
    w(&format!("{g}/strings/0x409/serialnumber"), "ultar-diag");
    let _ = fs::create_dir_all(format!("{g}/configs/c.1/strings/0x409"));
    w(
        &format!("{g}/configs/c.1/strings/0x409/configuration"),
        "diag",
    );
    if fs::create_dir_all(format!("{g}/functions/acm.usb0")).is_err() {
        return false; // Funktion ACM fehlt
    }
    let _ = std::os::unix::fs::symlink(
        format!("{g}/functions/acm.usb0"),
        format!("{g}/configs/c.1/acm.usb0"),
    );
    // Schon aktiv (z. B. gemerkter Fall wird nach einem Dienst-Neustart erneut geprüft)?
    if !fs::read_to_string(format!("{g}/UDC"))
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        return true;
    }
    // Jeden vorhandenen USB-Controller versuchen, der erste freie gilt
    let mut udcs: Vec<String> = fs::read_dir("/sys/class/udc")
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    udcs.sort();
    udcs.iter().any(|u| w(&format!("{g}/UDC"), u))
}

/// USB-Fall „android_usb“: ältere Hersteller-Kernel mit Android-Gadget. Bei neueren gibt es android0 nur als
/// Statusanzeige ohne „enable“ – dann passt dieser Fall nicht.
fn setup_android_usb() -> bool {
    let a = "/sys/class/android_usb/android0";
    if fs::metadata(format!("{a}/enable")).is_err() {
        return false;
    }
    w(&format!("{a}/enable"), "0");
    w(&format!("{a}/idVendor"), &VID[2..]);
    w(&format!("{a}/idProduct"), &PID[2..]);
    w(&format!("{a}/iManufacturer"), "KM Code Craft");
    w(&format!("{a}/iProduct"), "Ultar OS");
    w(&format!("{a}/f_acm/instances"), "1");
    w(&format!("{a}/functions"), "acm");
    w(&format!("{a}/enable"), "1")
}

fn read(p: &str) -> String {
    fs::read_to_string(p).unwrap_or_default().trim().to_string()
}

// Kernel-Ringpuffer über den syslog-Systemaufruf lesen (zuverlässiger als /dev/kmsg bei console=ram).
fn kernel_log_raw() -> String {
    extern "C" {
        fn klogctl(typ: i32, buf: *mut u8, len: i32) -> i32;
    }
    let mut buf = vec![0u8; 1 << 20]; // 1 MiB
    let n = unsafe {
        klogctl(3 /* READ_ALL */, buf.as_mut_ptr(), buf.len() as i32)
    };
    if n <= 0 {
        // Rückfall auf /dev/kmsg
        let mut out = String::new();
        if let Ok(mut f) = fs::OpenOptions::new()
            .read(true)
            .custom_flags(0o4000)
            .open("/dev/kmsg")
        {
            let mut b = [0u8; 8192];
            while let Ok(k) = f.read(&mut b) {
                if k == 0 {
                    break;
                }
                let l = String::from_utf8_lossy(&b[..k]);
                out.push_str(l.split_once(';').map(|x| x.1).unwrap_or(&l).trim_end());
                out.push('\n');
            }
        }
        return out;
    }
    String::from_utf8_lossy(&buf[..n as usize]).into_owned()
}

// Zeilen, die die Anzeige betreffen (DECON/DSIM/Panel) – für die Fehlersuche.
fn kernel_log_display() -> Vec<String> {
    let log = kernel_log_raw();
    let keys = [
        "decon",
        "dsim",
        "panel",
        "lcd",
        "ea8076",
        "disp",
        "fb",
        "te_",
        "regulator",
        "backlight",
    ];
    log.lines()
        .filter(|l| {
            let lc = l.to_ascii_lowercase();
            keys.iter().any(|k| lc.contains(k))
        })
        .map(|l| l.trim_end().to_string())
        .collect()
}

fn kmsg_tail(max: usize) -> Vec<String> {
    let log = kernel_log_raw();
    let all: Vec<String> = log.lines().map(|l| l.trim_end().to_string()).collect();
    let skip = all.len().saturating_sub(max);
    all[skip..].to_vec()
}

fn report(gadget: &str) -> String {
    let mut s = String::from("\r\n===== ULTAR-DIAGNOSE ANFANG =====\r\n");
    let mut add = |k: &str, v: String| s.push_str(&format!("{k}: {v}\r\n"));
    add("version", env!("CARGO_PKG_VERSION").into());
    add("uptime", read("/proc/uptime"));
    add("kernel", read("/proc/sys/kernel/osrelease"));
    add("cmdline", read("/proc/cmdline"));
    add("usb", gadget.into());
    for i in 0..3 {
        let b = format!("/sys/class/graphics/fb{i}");
        if fs::metadata(&b).is_ok() {
            add(
                &format!("fb{i}"),
                format!(
                    "name={} size={} bpp={} stride={} blank={}",
                    read(&format!("{b}/name")),
                    read(&format!("{b}/virtual_size")),
                    read(&format!("{b}/bits_per_pixel")),
                    read(&format!("{b}/stride")),
                    read(&format!("{b}/blank"))
                ),
            );
        }
    }
    let ls = |d: &str| {
        fs::read_dir(d)
            .map(|r| {
                r.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_else(|_| "-".into())
    };
    add("dev/dri", ls("/dev/dri"));
    add("dev/input", ls("/dev/input"));
    add("backlight", ls("/sys/class/backlight"));
    add("power_supply", ls("/sys/class/power_supply"));
    add("net", ls("/sys/class/net"));
    add(
        "meminfo",
        read("/proc/meminfo")
            .lines()
            .take(3)
            .collect::<Vec<_>>()
            .join(" | "),
    );
    add(
        "geraeteprofil",
        read(hw::RUNTIME)
            .lines()
            .filter(|l| !l.starts_with('#'))
            .collect::<Vec<_>>()
            .join("; "),
    );
    s.push_str("--- ultar-init ---\r\n");
    if let Ok(l) = LOG.lock() {
        for x in l.iter() {
            s.push_str(x);
            s.push_str("\r\n");
        }
    }
    for svc in ["ui", "net", "drivers"] {
        let t = read(&format!("/run/ultar/log/{svc}.log"));
        if !t.is_empty() {
            s.push_str(&format!("--- dienst {svc} ---\r\n"));
            for x in t
                .lines()
                .rev()
                .take(25)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                s.push_str(x);
                s.push_str("\r\n");
            }
        }
    }
    s.push_str("--- kernel: anzeige (decon/dsim/panel) ---\r\n");
    for x in kernel_log_display() {
        s.push_str(&x);
        s.push_str("\r\n");
    }
    s.push_str("--- kernel (letzte Meldungen) ---\r\n");
    for x in kmsg_tail(60) {
        s.push_str(&x);
        s.push_str("\r\n");
    }
    s.push_str("===== ULTAR-DIAGNOSE ENDE =====\r\n");
    s
}

/// Wachhund-Fall „dev_watchdog“. Viele Handy-Kernel (Samsung) starten den Hardware-Wachhund selbst und setzen das
/// Gerät zurück, wenn niemand /dev/watchdog bedient – unter Android macht das „watchdogd“. Liefert false, wenn
/// es kein solches Gerät gibt (dann ist nichts zu tun).
fn watchdog() -> bool {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.load(std::sync::atomic::Ordering::SeqCst) {
        return true;
    }
    let mut files: Vec<fs::File> = ["/dev/watchdog", "/dev/watchdog0", "/dev/watchdog1"]
        .iter()
        .filter_map(|p| fs::OpenOptions::new().write(true).open(p).ok())
        .collect();
    if files.is_empty() {
        return false;
    }
    STARTED.store(true, std::sync::atomic::Ordering::SeqCst);
    std::thread::spawn(move || loop {
        for f in files.iter_mut() {
            let _ = f.write_all(b"\0");
        }
        std::thread::sleep(std::time::Duration::from_secs(3));
    });
    true
}

/// Im Hintergrund starten (nur als PID 1). Kehrt sofort zurück.
/// Ablauf über den Geräteschalter: beim ersten Start alle Fälle durchprobieren und merken, danach nur den gemerkten.
pub fn start() {
    std::thread::spawn(|| {
        let mut p = hw::Profile::load();
        if p.known() {
            crate::log(&format!(
                "Geräteschalter: bekanntes Gerät ({}) – gemerkte Fälle werden direkt verwendet",
                p.device
            ));
        } else {
            crate::log(&format!("Geräteschalter: erster Start auf diesem Gerät ({}) – alle Fälle werden durchprobiert", p.device));
        }
        // 1. Wachhund zuerst: sonst setzt mancher Kernel das Gerät zurück, bevor der Rest fertig ist
        p.choose(
            "watchdog",
            &[Case {
                name: "dev_watchdog",
                probe: &watchdog,
            }],
        );
        // 2. Lebenszeichen ohne Bildschirm: dreimal kurz vibrieren (der erste Stoß ist zugleich die Probe)
        let vib = hw::pick_vibrator(&mut p);
        for _ in 0..2 {
            std::thread::sleep(std::time::Duration::from_millis(400));
            hw::vibrate(&vib, 150);
        }
        // Notausstieg (Laut+Leise halten) und Wachhund der Oberfläche – unabhängig von ultar-shell
        crate::rescue::start(vib.clone());
        // 3. USB-Diagnose
        let usb = p.choose(
            "usb",
            &[
                Case {
                    name: "configfs",
                    probe: &setup_configfs,
                },
                Case {
                    name: "android_usb",
                    probe: &setup_android_usb,
                },
            ],
        );
        // 4. Bildschirm und Hintergrundlicht (Feststellung; die Oberfläche liest das Ergebnis aus /run/ultar/hw.toml)
        hw::pick_display(&mut p);
        hw::pick_backlight(&mut p);
        p.save();
        cmd_server(&usb);
    });
}

/// Befehls-Server über die USB-Verbindung (/dev/ttyGS0): beim Verbinden einmal der Bericht,
/// danach Zeile für Zeile Befehle entgegennehmen, über die Shell ausführen und die Ausgabe
/// zurückschicken. So lässt sich Ultar vom PC aus live bearbeiten – ohne Neu-Flashen.
fn cmd_server(usb: &str) -> ! {
    use std::io::{BufRead, BufReader};
    loop {
        let port = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(0o400 /* O_NOCTTY */)
            .open("/dev/ttyGS0");
        let Ok(mut w) = port else {
            std::thread::sleep(std::time::Duration::from_millis(400));
            continue;
        };
        // Beim Verbinden einmal der volle Bericht (für den "Diagnose lesen"-Knopf)
        let _ = w.write_all(report(usb).as_bytes());
        let _ = w
            .write_all(b"\r\nULTAR-TUER bereit. Befehl eingeben (sh), 'report' fuer Diagnose.\r\n");
        let Ok(rclone) = w.try_clone() else {
            std::thread::sleep(std::time::Duration::from_millis(400));
            continue;
        };
        let mut reader = BufReader::new(rclone);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break, // Verbindung getrennt
                Ok(_) => {}
                Err(_) => break,
            }
            let cmd = line.trim();
            if cmd.is_empty() {
                continue;
            }
            if cmd == "report" {
                let _ = w.write_all(report(usb).as_bytes());
                continue;
            }
            // "@<sek> befehl" setzt das Zeitlimit (Standard 120 s): ein hängender Treiber blockiert sonst die Tür
            let (limit, cmd) = match cmd.strip_prefix('@').and_then(|r| r.split_once(' ')) {
                Some((n, rest)) if n.parse::<u64>().is_ok() => (n.parse().unwrap(), rest),
                _ => (120u64, cmd),
            };
            let _ = w.write_all(b"\x02"); // Start-Marke
            match run_limited(cmd, std::time::Duration::from_secs(limit)) {
                Ok((out, rc, timed_out)) => {
                    let _ = w.write_all(&out);
                    if timed_out {
                        let _ = w.write_all(format!("\r\nTUER: Zeitlimit {limit} s ueberschritten, Befehl abgebrochen\r\n").as_bytes());
                        for l in crate::rescue::stuck_report() {
                            let _ = w.write_all(format!("  {l}\r\n").as_bytes());
                        }
                    }
                    let _ = w.write_all(format!("\r\n\x03RC={rc}\r\n").as_bytes());
                }
                Err(e) => {
                    let _ = w.write_all(
                        format!("FEHLER beim Start der Shell: {e}\r\n\x03RC=-1\r\n").as_bytes(),
                    );
                }
            }
        }
    }
}

/// Shell-Befehl mit Zeitlimit: eigene Prozessgruppe, bei Überschreitung SIGKILL an die ganze Gruppe.
/// Ausgabe (stdout+stderr gemischt) über eine Pipe; liefert (Ausgabe, Rückgabewert, Zeitlimit erreicht).
fn run_limited(cmd: &str, limit: std::time::Duration) -> std::io::Result<(Vec<u8>, i32, bool)> {
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    let (r, wr) = nix::unistd::pipe().map_err(std::io::Error::from)?;
    let wr2 = wr.try_clone()?;
    let mut child = std::process::Command::new("/system/bin/sh")
        .arg("-c")
        .arg(cmd)
        .env("PATH", "/system/bin:/sbin:/usr/bin:/bin:/usr/lib/ultar")
        .stdin(Stdio::null())
        .stdout(Stdio::from(wr))
        .stderr(Stdio::from(wr2))
        .process_group(0)
        .spawn()?;
    let pgid = child.id() as i32;
    let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
    std::thread::spawn(move || {
        let mut f = fs::File::from(r);
        let mut buf = [0u8; 8192];
        loop {
            match std::io::Read::read(&mut f, &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    if tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    let t0 = std::time::Instant::now();
    let mut out = Vec::new();
    let mut rc = -1;
    let mut timed_out = false;
    loop {
        while let Ok(b) = rx.try_recv() {
            out.extend_from_slice(&b);
        }
        match child.try_wait() {
            Ok(Some(s)) => {
                rc = s.code().unwrap_or(-1);
                break;
            }
            Ok(None) => {}
            Err(_) => break, // schon von PID 1 (waitpid(-1)) eingesammelt
        }
        if t0.elapsed() > limit {
            timed_out = true;
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(pgid),
                nix::sys::signal::Signal::SIGKILL,
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    // Restausgabe einsammeln; Kindprozesse im Hintergrund (Pipe noch offen) blockieren höchstens 1 s
    let dl = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while let Ok(b) = rx.recv_timeout(dl.saturating_duration_since(std::time::Instant::now())) {
        out.extend_from_slice(&b);
    }
    Ok((out, rc, timed_out))
}
