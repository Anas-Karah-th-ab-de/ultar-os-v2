//! ultar-shell – Oberfläche von Ultar OS (KM Code Craft).
//!
//! Zeichnet direkt über DRM/KMS (ohne X11/Wayland), Eingabe über evdev (Touch).
//! Gezeichnet wird nur bei Ereignissen – im Leerlauf keine CPU-Last.

mod decon;
mod icons_svg;
mod agent;
mod app;
mod apps;
mod audio;
mod brand;
mod camera;
mod display;
mod fern;
mod face;
mod fp;
mod gfx;
mod hw;
mod i18n;
mod gnss;
mod help;
mod input;
mod mjpeg;
mod notify;
mod privacy;
mod linkcheck;
mod mic;
mod mind;
mod voice;
mod quick;
mod scan;
mod selftest;
mod sys;
mod sysx;
mod sysy;
mod sysd;
mod text;
mod ui;
mod kbd;
mod wifi;
mod update;

use app::App;
use gfx::{rgb, Canvas};
use input::{Ev, Input};
use std::os::fd::AsFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ui::{Key, Ptr, Ui};

/// Wird per SIGUSR1 gesetzt: nächster Frame wird als PNG nach /data/ultar/screen.png gespeichert.
/// So kann der Bildschirm ohne Kamera vom PC aus angesehen werden (Live-Entwicklung über die Tür).
static SHOT: AtomicBool = AtomicBool::new(false);
static WAKE_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);
extern "C" fn on_sigusr1(_: i32) {
    SHOT.store(true, Ordering::SeqCst);
    // Poll aufwecken (write ist signal-sicher)
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let b = [1u8];
        unsafe { nix::libc::write(fd, b.as_ptr() as *const _, 1) };
    }
}

/// Aktuellen Bildschirminhalt (cv.px = 0x00RRGGBB) als PNG speichern.
fn save_screenshot(cv: &Canvas) {
    let (w, h) = (cv.w as u32, cv.h as u32);
    let mut buf = Vec::with_capacity((w * h * 3) as usize);
    for &p in &cv.px {
        buf.push((p >> 16) as u8);
        buf.push((p >> 8) as u8);
        buf.push(p as u8);
    }
    let path = "/data/ultar/screen.png";
    let _ = std::fs::create_dir_all("/data/ultar");
    if let Some(img) = image::RgbImage::from_raw(w, h, buf) {
        match img.save(path) {
            Ok(()) => eprintln!("[shot] gespeichert: {path} ({w}x{h})"),
            Err(e) => eprintln!("[shot] Fehler: {e}"),
        }
    }
}

/// Tasten, die aus dem Browser zurück zur Ultar-Oberfläche führen (Ein/Aus 116, Home 102/172, F12 88).
struct ExitKeys {
    devs: Vec<std::fs::File>,
    /// Desktop-Sitzung: nur die Ein/Aus-Taste (Pos1/F12 braucht man dort zum Arbeiten)
    power_only: bool,
}

fn web_exit_keys() -> ExitKeys {
    use std::os::unix::fs::OpenOptionsExt;
    let devs = std::fs::read_dir("/dev/input")
        .map(|d| {
            d.flatten()
                .filter(|e| e.file_name().to_string_lossy().starts_with("event"))
                .filter_map(|e| std::fs::OpenOptions::new().read(true).custom_flags(0o4000).open(e.path()).ok())
                .collect()
        })
        .unwrap_or_default();
    ExitKeys { devs, power_only: false }
}

impl ExitKeys {
    /// true, sobald eine der Rückkehr-Tasten gedrückt wurde. Liest nur mit, greift nicht in die Eingabe ein.
    fn pressed(&mut self) -> bool {
        use std::io::Read;
        let mut hit = false;
        let mut buf = [0u8; 24 * 32];
        for f in self.devs.iter_mut() {
            while let Ok(n) = f.read(&mut buf) {
                if n < 24 {
                    break;
                }
                for ev in buf[..n].chunks_exact(24) {
                    let (ty, code, val) = (u16::from_le_bytes([ev[16], ev[17]]), u16::from_le_bytes([ev[18], ev[19]]), i32::from_le_bytes([ev[20], ev[21], ev[22], ev[23]]));
                    if ty == 1 && val == 1 && (code == 116 || (!self.power_only && matches!(code, 102 | 172 | 88))) {
                        hit = true;
                    }
                }
            }
        }
        hit
    }
}

/// Signal an alle Prozesse mit einem der Namen schicken.
fn kill_named(names: &[&str], sig: i32) {
    let Ok(rd) = std::fs::read_dir("/proc") else { return };
    for e in rd.flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<i32>() else { continue };
        let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default();
        if names.iter().any(|n| comm.trim() == *n) {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), nix::sys::signal::Signal::try_from(sig).ok());
        }
    }
}

/// Ultar Web starten (Bündel unter /usr/lib/ultar/web). Liefert false, wenn es nicht installiert ist
/// oder es um ein Ubuntu-Programm geht – dann übernimmt der bisherige Weg (Weston + Epiphany).
fn run_ultar_web(url: &str) -> bool {
    const BIN: &str = "/usr/lib/ultar/web/bin/ultar-web";
    if url.starts_with("exec:") || !std::path::Path::new(BIN).exists() || std::path::Path::new("/data/ultar/web-alt").exists() {
        return false;
    }
    let _ = std::fs::create_dir_all("/run/ultar/log");
    let host = url.split("://").nth(1).unwrap_or(url).split('/').next().unwrap_or("").to_string();
    let app = ["whatsapp.com", "telegram.org", "youtube.com", "spotify.com"].iter().any(|d| host.contains(d));
    let mut cmd = std::process::Command::new(BIN);
    cmd.arg(url);
    if app {
        cmd.arg("--app");
    }
    if let Some(l) = std::fs::File::create("/run/ultar/log/web.log").ok() {
        if let Ok(l2) = l.try_clone() {
            cmd.stdout(l).stderr(l2);
        }
    }
    match cmd.spawn() {
        Ok(mut child) => {
            // Rückweg auch dann, wenn der Browser nicht mehr reagiert: Ein/Aus-Taste beendet ihn
            let mut keys = web_exit_keys();
            let mut pressed_at: Option<Instant> = None;
            loop {
                match child.try_wait() {
                    Ok(Some(s)) => {
                        println!("ultar-shell: Ultar Web beendet ({s})");
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
                if keys.pressed() && pressed_at.is_none() {
                    // erst dem Browser Zeit geben, sich selbst sauber zu beenden
                    pressed_at = Some(Instant::now());
                }
                if pressed_at.is_some_and(|t| t.elapsed() > Duration::from_millis(2500)) {
                    kill_named(&["ultar-web", "WPEWebProcess", "WPENetworkProcess", "WPEGPUProcess"], 9);
                    let _ = child.wait();
                    break;
                }
                alive();
                std::thread::sleep(Duration::from_millis(120));
            }
            true
        }
        Err(e) => {
            eprintln!("ultar-shell: Ultar Web nicht startbar: {e}");
            false
        }
    }
}

/// Browser-Sitzung: Bildschirm und Eingabe abgeben, Fenstersystem + Browser im Ubuntu-Bereich laufen lassen,
/// danach den Bildschirm wieder übernehmen. Blockiert, solange der Browser offen ist.
fn web_session(disp: display::Display, url: &str, scale: f32) -> display::Display {
    drop(disp); // gibt den Bildschirm frei (DRM-Master)
    // Ultar Web: eigener Browser mit Rust-Kern, ohne Ubuntu-Bereich und ohne Fenstersystem (falls installiert)
    if !run_ultar_web(url) {
    let script = "/run/ultar/web-session.sh";
    let _ = std::fs::create_dir_all("/run/ultar/log");
    // webapp.sh gibt jeder Web-App ein eigenes, dauerhaftes Login-Profil (kein erneutes Anmelden).
    // Rückfall auf die alte web-session.sh, falls webapp.sh (noch) nicht vorhanden ist.
    // "exec:<Befehl>" startet statt des Browsers ein grafisches Ubuntu-Programm im selben Fenstersystem.
    let app_cmd = url.strip_prefix("exec:");
    let desktop = url.starts_with("desktop:");
    if desktop {
        // Ultar-Desktop: eigenes Fenstersystem ultar-wm (Rust) mit Leiste, Startmenü und allen Programmen
        let _ = std::fs::copy("/usr/lib/ultar/desktop-session.sh", script);
    } else if app_cmd.is_some() {
        let _ = std::fs::copy("/usr/lib/ultar/app-session.sh", script);
    } else if std::fs::copy("/usr/lib/ultar/webapp.sh", script).is_err() {
        let _ = std::fs::copy("/usr/lib/ultar/web-session.sh", script);
    }
    let url = if desktop { "" } else { app_cmd.unwrap_or(url) };
    let log = std::fs::File::create("/run/ultar/log/web.log").ok();
    let mut cmd = std::process::Command::new(sys::COMPAT);
    // WhatsApp Web und Telegram Web brauchen die Fassung für große Bildschirme, alles andere läuft mobil
    let kind = if url.contains("whatsapp.com") || url.contains("telegram.org") { "desktop" } else { "mobil" };
    cmd.args(["run", "sh", script, url, &format!("{scale:.2}"), kind]);
    if let Some(l) = log {
        if let Ok(l2) = l.try_clone() {
            cmd.stdout(l).stderr(l2);
        }
    }
    match cmd.spawn() {
        Ok(mut child) => {
            // Sicherer Rückweg, unabhängig vom Browser: Ein/Aus-Taste (oder Home, F12) beendet die Sitzung.
            let mut keys = web_exit_keys();
            keys.power_only = desktop;
            loop {
                match child.try_wait() {
                    Ok(Some(s)) => {
                        println!("ultar-shell: Browser beendet ({s})");
                        break;
                    }
                    Ok(None) => {}
                    Err(_) => break,
                }
                if keys.pressed() {
                    println!("ultar-shell: Rückkehr-Taste gedrückt, Browser wird beendet");
                    kill_named(&["epiphany", "WebKitWebProcess", "WebKitNetworkProcess", "ultar-app", "weston", "ultar-wm"], 15);
                    std::thread::sleep(Duration::from_millis(1500));
                    kill_named(&["epiphany", "WebKitWebProcess", "WebKitNetworkProcess", "weston", "ultar-wm"], 9);
                    let _ = child.wait();
                    break;
                }
                alive();
                std::thread::sleep(Duration::from_millis(120));
            }
        }
        Err(e) => eprintln!("ultar-shell: Browser nicht startbar: {e}"),
    }
    }
    let t0 = Instant::now();
    loop {
        match display::Display::open() {
            Ok(d) => return d,
            Err(e) if t0.elapsed() > Duration::from_secs(15) => {
                eprintln!("ultar-shell: Bildschirm nach dem Browser nicht übernehmbar: {e}");
                std::process::exit(1); // der Dienst wird neu gestartet
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

fn frame(app: &mut App, cv: &mut Canvas, tx: &mut text::Text, p: &mut Ptr, _short_side: f32) {
    for pass in 0..2 {
        let (w, h) = (cv.w as f32, cv.h as f32);
        let mut ui = Ui { th: ui::theme(app.set.dark, app.set.accent), k: ui::scale_for(w, h, app.set.scale), w, h, cv, tx, p, rtl: false, enabled: true, focus: None, dirty: false, clip: None, links: Vec::new(), kb: Default::default(), seen: None };
        app.draw(&mut ui);
        if !ui.dirty || pass == 1 {
            break;
        }
        // zweiter Durchlauf zeigt den neuen Zustand; Eingabe nicht doppelt auswerten
        p.pressed = false;
        p.released = false;
        p.dy = 0.0;
        p.dx = 0.0;
    }
    p.rad = 0.0;
}

/// Lebenszeichen für den Wachhund in ultar-init (höchstens alle 5 s geschrieben). Bleibt es über 150 s aus,
/// beendet init die Oberfläche und startet sie neu (siehe crates/ultar-init/src/rescue.rs).
fn alive() {
    use std::sync::atomic::{AtomicU64, Ordering};
    static LAST: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if now.abs_diff(LAST.load(Ordering::Relaxed)) >= 5 {
        LAST.store(now, Ordering::Relaxed);
        let _ = std::fs::write("/run/ultar/ui.alive", now.to_string());
    }
}

fn main() {
    // Startbild für den Bootloader erzeugen: ultar-shell --splash <ar|en|de> <breite> <höhe> <datei.bmp>
    let a: Vec<String> = std::env::args().collect();
    // Link-Prüfung in der abgeschotteten Box (siehe linkcheck.rs): ultar-shell --linkprobe <adresse>
    if a.len() == 3 && a[1] == "--linkprobe" {
        std::process::exit(linkcheck::probe_child(&a[2]));
    }
    // Bildschirmfoto ohne Gerät (für Gestaltungsprüfung): ultar-shell --shot <breite> <höhe> <bildschirm> <sprache> <datei.bmp>
    if a.len() >= 3 && a[1] == "--mic" {
        std::process::exit(mic::test_cli(a[2].parse().unwrap_or(3000), a.get(3).map(|s| s.as_str())));
    }
    if a.len() == 7 && a[1] == "--shot" {
        std::env::set_var("ULTAR_DRY", "1");
        let (w, h): (i32, i32) = (a[2].parse().expect("Breite"), a[3].parse().expect("Höhe"));
        let (tx, rx) = std::sync::mpsc::channel();
        let (_rfd, wfd) = nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC).expect("pipe");
        let mut app = App::new(sys::Bg { tx, wake: Arc::new(wfd) }, (w as u32, h as u32, "shot".into()));
        // Helfer: ohne Gleiten sofort am Ziel, keine Vorstellung von selbst
        app.buddy.snap = true;
        app.debug_open(&a[4], &a[5]);
        let mut cv = Canvas::new(w, h);
        let mut text = text::Text::new();
        let mut ptr = Ptr::default();
        // ULTAR_SHOT_WAIT=<ms>: Hintergrundergebnisse (z. B. WLAN-Suche) abwarten, bevor das Foto entsteht
        let wait: u64 = std::env::var("ULTAR_SHOT_WAIT").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
        let t_end = Instant::now() + Duration::from_millis(wait);
        while Instant::now() < t_end {
            while let Ok(m) = rx.try_recv() {
                app.on_msg(m);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        for i in 0..80 {
            // mindestens 10 Durchläufe; länger nur, solange im Hintergrund noch geladen wird (Vorschaubilder)
            if i >= 10 && !app.shot_pending() {
                break;
            }
            while let Ok(m) = rx.try_recv() {
                app.on_msg(m);
            }
            app.tick();
            frame(&mut app, &mut cv, &mut text, &mut ptr, w.min(h) as f32);
            std::thread::sleep(Duration::from_millis(60));
        }
        brand::write_bmp(&cv, &a[6]).expect("BMP schreiben");
        return;
    }
    // Messung auf dem Gerät: ultar-shell --bench [breite höhe] -> Zeichenzeit je Bildschirm (ohne Anzeige)
    if a.len() >= 2 && a[1] == "--bench" {
        std::env::set_var("ULTAR_DRY", "1");
        let w: i32 = a.get(2).and_then(|v| v.parse().ok()).unwrap_or(1080);
        let h: i32 = a.get(3).and_then(|v| v.parse().ok()).unwrap_or(2340);
        let mut cv = Canvas::new(w, h);
        let mut text = text::Text::new();
        let mut out = vec![0u8; (w * h * 4) as usize];
        for scr in ["home", "settings", "contacts", "calc", "clock", "files", "gallery", "tools", "privacy", "monitor", "shade", "locknow", "kb-text", "helper"] {
            if std::env::var("ULTAR_BENCH_ONLY").is_ok_and(|o| o != scr) {
                continue;
            }
            let (tx, _rx) = std::sync::mpsc::channel();
            let (_rfd, wfd) = nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC).expect("pipe");
            let mut app = App::new(sys::Bg { tx, wake: Arc::new(wfd) }, (w as u32, h as u32, "bench".into()));
            app.buddy.snap = true;
            app.debug_open(scr, "de");
            let mut ptr = Ptr::default();
            let mut times = Vec::new();
            for i in 0..25 {
                let t0 = Instant::now();
                frame(&mut app, &mut cv, &mut text, &mut ptr, w.min(h) as f32);
                let t1 = Instant::now();
                // Kopie in den Anzeigepuffer wie im Betrieb
                out.copy_from_slice(unsafe { std::slice::from_raw_parts(cv.px.as_ptr() as *const u8, cv.px.len() * 4) });
                let t2 = Instant::now();
                if i > 0 {
                    times.push(((t1 - t0).as_secs_f32() * 1000.0, (t2 - t1).as_secs_f32() * 1000.0));
                }
            }
            let n = times.len() as f32;
            let d: f32 = times.iter().map(|t| t.0).sum::<f32>() / n;
            let mx = times.iter().map(|t| t.0).fold(0.0, f32::max);
            let c: f32 = times.iter().map(|t| t.1).sum::<f32>() / n;
            let first = 0.0;
            let _ = first;
            println!("{scr:10} zeichnen {d:6.1} ms (max {mx:6.1})  kopieren {c:5.1} ms");
        }
        return;
    }
    // Ton für den Ultar-Verbund (Mithören/Durchsage): 16 kHz, Mono, 16 Bit, roh über stdout/stdin
    if a.len() == 2 && a[1] == "--mic-roh" {
        std::process::exit(mic::roh_cli());
    }
    if a.len() == 2 && a[1] == "--ton-roh" {
        std::process::exit(audio::roh_cli());
    }
    if a.len() == 2 && a[1] == "--beep" {
        std::process::exit(audio::test_cli());
    }
    if a.len() == 6 && a[1] == "--splash" {
        let mut cv = Canvas::new(a[3].parse().expect("Breite"), a[4].parse().expect("Höhe"));
        brand::splash(&mut cv, &mut text::Text::new(), &a[2]);
        brand::write_bmp(&cv, &a[5]).expect("BMP schreiben");
        return;
    }
    let t0 = Instant::now();
    // --virtuell [BxH]: ohne Anzeigegerät zeichnen (Gerät ohne Bildschirm, bedient über den Ultar-Verbund)
    let virtuell = a.iter().position(|x| x == "--virtuell").map(|i| a.get(i + 1).filter(|v| !v.starts_with('-')).cloned());
    let mut disp = match &virtuell {
        Some(g) => {
            let (w, h) = fern::virtuell_groesse(g.as_deref());
            display::Display::virtuell(w, h)
        }
        None => loop {
            match display::Display::open() {
                Ok(d) => break d,
                Err(e) if t0.elapsed() > Duration::from_secs(15) => {
                    // kein Bildschirm: virtuell weiterlaufen – das Gerät bleibt über den Verbund sicht- und bedienbar
                    eprintln!("ultar-shell: kein Bildschirm ({e}) – virtueller Bildschirm");
                    let (w, h) = fern::virtuell_groesse(None);
                    break display::Display::virtuell(w, h);
                }
                Err(_) => std::thread::sleep(Duration::from_millis(15)),
            }
        },
    };
    let mut naechste_suche = Instant::now() + Duration::from_secs(30);
    let mut geste = fern::Geste::default();
    println!("ultar-shell: {} {}x{}", disp.name, disp.w, disp.h);

    let (tx, rx) = std::sync::mpsc::channel();
    let (rfd, wfd) = nix::unistd::pipe2(nix::fcntl::OFlag::O_NONBLOCK | nix::fcntl::OFlag::O_CLOEXEC).expect("pipe");
    // SIGUSR1 -> Bildschirmfoto (Live-Entwicklung ohne Kamera): `kill -USR1 $(pidof ultar-shell)`
    WAKE_FD.store(std::os::fd::AsRawFd::as_raw_fd(&wfd), Ordering::SeqCst);
    unsafe {
        nix::libc::signal(nix::libc::SIGUSR1, on_sigusr1 as usize);
    }
    fern::start(std::os::fd::AsRawFd::as_raw_fd(&wfd));
    let bg = sys::Bg { tx, wake: Arc::new(wfd) };
    // Wächter: neue Benachrichtigungen (auch bei Bildschirm aus) wecken die Schleife zügig
    {
        let wake_raw = WAKE_FD.load(Ordering::SeqCst);
        std::thread::spawn(move || {
            let mut last = 0u64;
            loop {
                let len = std::fs::metadata(ultar_notify::LOG).map(|m| m.len()).unwrap_or(0);
                if len != last {
                    last = len;
                    if wake_raw >= 0 {
                        let b = [1u8];
                        unsafe { nix::libc::write(wake_raw, b.as_ptr() as *const _, 1) };
                    }
                }
                std::thread::sleep(Duration::from_millis(400));
            }
        });
    }
    // Auf die (im Hintergrund eingebundene) /data-Partition warten, BEVOR die persistenten
    // Einstellungen geladen werden – sonst erscheint das Setup/die Sprachauswahl bei jedem Neustart.
    sys::wait_for_data();
    let mut app = App::new(bg, (disp.w, disp.h, disp.name.clone()));
    // Startbildschirm vorgeben (Kiosk, Tests): ultar-shell --open <bildschirm> [sprache], z. B. "office:/data/a.pdf de"
    if let Some(i) = a.iter().position(|x| x == "--open") {
        if let Some(n) = a.get(i + 1) {
            app.remote_open(n, a.get(i + 2).map(|s| s.as_str()));
        }
    }
    let mut text = text::Text::new();
    {
        // Startbild mit Logo, bis der erste Bildschirm gezeichnet ist
        let (lw, lh) = disp.logical(app.set.rotation);
        let mut cv = Canvas::new(lw, lh);
        brand::splash(&mut cv, &mut text, app.lang());
        disp.present(&cv.px, lw, app.set.rotation, 1.0);
    }
    let mut input = Input::new();
    let mut ptr = Ptr::default();
    let mut cv = Canvas::new(1, 1);
    let mut redraw = true;
    let mut asleep = false;
    let mut prox_blanked = false;
    let mut last_input = Instant::now();
    let mut last_min = -1;
    let mut last_voldown: Option<Instant> = None;
    // Desktop-/Laptop-Ausgabe: nach der Ersteinrichtung direkt in den Ultar-Desktop (ultar-wm), wenn er
    // installiert ist und ein Grafikgerät da ist. Rückkehr in die Oberfläche bleibt jederzeit möglich
    // (Startmenü → „Zurück zu Ultar“, Ein/Aus-Taste); danach kein erneuter Autostart bis zum Neustart.
    if ultar_edition::current() == ultar_edition::Edition::Desktop
        && app.set.setup_done
        && sys::has_drm()
        && ["/compat/usr/bin/ultar-wm", "/compat/usr/local/bin/ultar-wm", "/usr/lib/ultar/ultar-wm"].iter().any(|p| std::path::Path::new(p).exists())
        && std::fs::metadata("/run/ultar/desktop-autostarted").is_err()
    {
        let _ = std::fs::write("/run/ultar/desktop-autostarted", b"1");
        app.launch = Some("desktop:".into());
    }

    loop {
        if let Some(url) = app.launch.take() {
            if disp.ist_virtuell() {
                // ohne echten Bildschirm kein Browser – die Ansicht gehört auf ein Gerät mit Anzeige
                eprintln!("ultar-shell: virtueller Bildschirm – Browser ({url}) nicht geöffnet");
                continue;
            }
            let scale = ui::scale_for(disp.w as f32, disp.h as f32, app.set.scale);
            disp = web_session(disp, &url, scale);
            input = Input::new(); // aufgelaufene Berührungen verwerfen
            ptr = Ptr::default();
            last_input = Instant::now();
            redraw = true;
        }
        let rot = app.set.rotation;
        let (lw, lh) = disp.logical(rot);
        if cv.w != lw || cv.h != lh {
            cv = Canvas::new(lw, lh);
            redraw = true;
        }
        let short = lw.min(lh) as f32;
        let (pw, ph) = (disp.w as i32, disp.h as i32);
        let map = |u: f32, v: f32| {
            let (x, y) = display::phys_to_logical(rot, (u * (pw - 1) as f32) as i32, (v * (ph - 1) as f32) as i32, pw, ph);
            (x as f32, y as f32)
        };

        // Leerlauf: ohne laufende Anzeige bis zur nächsten vollen Minute (bzw. bis zum Bildschirm-Timeout) schlafen
        let wait_ms: u64 = if redraw {
            0
        } else if app.needs_tick() && !asleep {
            500
        } else {
            let mut ms = 60_000 - (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0) % 60_000) + 20;
            // Auch bei Bildschirm aus auf die volle Minute ausrichten, damit Wecker pünktlich klingeln
            if !asleep && app.set.timeout_s > 0 {
                ms = ms.min((app.set.timeout_s as u64 * 1000).saturating_sub(last_input.elapsed().as_millis() as u64) + 20);
            }
            ms
        };
        // Während eines Anrufs häufiger prüfen (Näherungssensor am Ohr reagiert flott)
        let wait_ms = if app.call_active() { wait_ms.min(150) } else { wait_ms };
        // Schwung/Zurückfedern beim Scrollen: ~60 Bilder/s, solange es läuft
        let wait_ms = if ui::scroll_animating() && !asleep { wait_ms.min(16) } else { wait_ms };
        // Kamerabild, Video, Selbstauslöser: zügig weiterzeichnen (~30 Bilder/s)
        let wait_ms = if !asleep && app.animating() { wait_ms.min(30) } else { wait_ms };
        let mut evs = input.wait(wait_ms.min(60_000) as u16, rfd.as_fd());
        alive();
        // Fernbedienung (Ultar-Verbund): Eingaben kommen logisch 0..1 → in physische Lage umrechnen
        for e in fern::eingaben() {
            let phys = |u: f32, v: f32| match rot {
                90 => (1.0 - v, u),
                180 => (1.0 - u, 1.0 - v),
                270 => (v, 1.0 - u),
                _ => (u, v),
            };
            evs.push(match e {
                Ev::Down(u, v) => {
                    let (a, b) = phys(u, v);
                    Ev::Down(a, b)
                }
                Ev::Move(u, v) => {
                    let (a, b) = phys(u, v);
                    Ev::Move(a, b)
                }
                x => x,
            });
        }
        for st in fern::steuerung() {
            match st {
                fern::Steuer::Hell(b) => {
                    app.set.brightness = b;
                    app.apply_brightness();
                }
                fern::Steuer::Drehen(r) => app.set.rotation = r,
                fern::Steuer::Bildschirm(an) => {
                    if an && asleep {
                        asleep = false;
                        app.apply_brightness();
                        last_input = Instant::now();
                    } else if !an && !asleep {
                        asleep = true;
                        app.on_sleep();
                        sys::set_backlight(0.0);
                        cv.clear(rgb(0));
                        disp.present(&cv.px, lw, rot, 1.0);
                    }
                }
                fern::Steuer::Oeffne(n, l) => app.remote_open(&n, l.as_deref()),
                fern::Steuer::Helfer(t) => app.remote_helfer(t),
                fern::Steuer::Datei(p) => app.remote_datei(p),
                fern::Steuer::Browser(u) => app.launch = Some(u),
            }
            redraw = true;
        }
        // virtueller Bildschirm: alle 30 s nach einem echten Anzeigegerät sehen (spät geladene Treiber)
        if disp.ist_virtuell() && virtuell.is_none() && Instant::now() > naechste_suche {
            naechste_suche = Instant::now() + Duration::from_secs(30);
            if let Ok(d) = display::Display::open() {
                println!("ultar-shell: Bildschirm gefunden: {} {}x{}", d.name, d.w, d.h);
                disp = d;
                redraw = true;
            }
        }
        let mut b = [0u8; 64];
        while nix::unistd::read(std::os::fd::AsRawFd::as_raw_fd(&rfd), &mut b).is_ok_and(|n| n > 0) {}
        while let Ok(m) = rx.try_recv() {
            app.on_msg(m);
            redraw = true;
        }

        // Näherungssensor während eines Anrufs: Handy am Ohr -> Bildschirm aus, Eingaben ignorieren
        let near = app.call_active() && sys::proximity_near() == Some(true);
        if near {
            if !prox_blanked {
                prox_blanked = true;
                sys::set_backlight(0.0);
                cv.clear(rgb(0));
                disp.present(&cv.px, lw, rot, 1.0);
            }
            // Benachrichtigungen trotzdem einsammeln (z. B. zweiter Anruf), aber nicht zeichnen
            let _ = app.poll_notifications(false);
            continue;
        } else if prox_blanked {
            prox_blanked = false;
            app.apply_brightness();
            redraw = true;
        }

        if !evs.is_empty() {
            fern::bedient();
        }
        let mut i = 0;
        while i < evs.len() {
            let ev = evs[i];
            i += 1;
            last_input = Instant::now();
            // Weitergeben-Geste (lange halten, dann nach oben wischen): Datei/App an ein anderes Gerät
            if !asleep {
                let logisch = match ev {
                    Ev::Down(u, v) => {
                        let (x, y) = map(u, v);
                        Ev::Down(x / lw as f32, y / lh as f32)
                    }
                    Ev::Move(u, v) => {
                        let (x, y) = map(u, v);
                        Ev::Move(x / lw as f32, y / lh as f32)
                    }
                    x => x,
                };
                if geste.ereignis(&logisch) {
                    ptr = Ptr::default();
                    app.weitergeben_oeffnen();
                    redraw = true;
                }
                if geste.ausgeloest && matches!(ev, Ev::Move(..) | Ev::Up) {
                    continue; // Rest dieser Berührung gehört der Geste
                }
            }
            if asleep {
                // erste Berührung weckt nur auf
                if !matches!(ev, Ev::Down(..) | Ev::Move(..)) {
                    asleep = false;
                    app.apply_brightness();
                    ptr = Ptr::default();
                    redraw = true;
                }
                continue;
            }
            match ev {
                Ev::Down(u, v) => {
                    let (x, y) = map(u, v);
                    ptr = Ptr { x, y, px: x, py: y, down: true, pressed: true, ..Default::default() };
                }
                Ev::Move(mut u, mut v) => {
                    while let Some(Ev::Move(a, b)) = evs.get(i) {
                        (u, v) = (*a, *b);
                        i += 1;
                    }
                    let (x, y) = map(u, v);
                    if ptr.down {
                        ptr.dy += y - ptr.y;
                        ptr.dx += x - ptr.x;
                        if (x - ptr.px).abs() + (y - ptr.py).abs() > short * 0.03 {
                            ptr.moved = true;
                        }
                    }
                    (ptr.x, ptr.y) = (x, y);
                    if !ptr.down {
                        redraw |= input.has_mouse || app.desk.an;
                        continue;
                    }
                }
                Ev::Up => {
                    if !ptr.down {
                        continue;
                    }
                    ptr.down = false;
                    ptr.released = true;
                }
                Ev::Char(c) => {
                    app.on_key(Key::Char(c));
                    redraw = true;
                    continue;
                }
                Ev::Backspace => {
                    app.on_key(Key::Backspace);
                    redraw = true;
                    continue;
                }
                Ev::Enter => {
                    app.on_key(Key::Enter);
                    redraw = true;
                    continue;
                }
                Ev::Rad(n) => {
                    ptr.rad -= n as f32 * short * 0.08;
                    redraw = true;
                    continue;
                }
                Ev::Back => {
                    app.back();
                    redraw = true;
                    continue;
                }
                Ev::VolUp => {
                    app.nudge_volume(0.1);
                    redraw = true;
                    continue;
                }
                Ev::VolDown => {
                    last_voldown = Some(Instant::now());
                    app.nudge_volume(-0.1);
                    redraw = true;
                    continue;
                }
                Ev::Power if last_voldown.is_some_and(|t| t.elapsed() < Duration::from_millis(900)) => {
                    // Leiser + Ein/Aus = Bildschirmfoto (Lautstärke-Schritt zurücknehmen)
                    last_voldown = None;
                    app.nudge_volume(0.1);
                    frame(&mut app, &mut cv, &mut text, &mut ptr, short);
                    let d = sys::from_unix(sys::unix_now(), app.set.tz_min);
                    let path = format!("/data/Pictures/Screenshots/Screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.png", d.y, d.mo, d.d, d.h, d.mi, d.s);
                    match sysx::save_png(&path, cv.w as u32, cv.h as u32, &cv.px) {
                        Ok(()) => ultar_notify::post("system", "screenshot", app.tr("Screenshot saved"), &path, ultar_notify::Urgency::Low),
                        Err(e) => eprintln!("Bildschirmfoto: {e}"),
                    }
                    sys::vibrate(40);
                    redraw = true;
                    continue;
                }
                Ev::Power => {
                    // Bildschirm aus-/einschalten
                    asleep = true;
                    app.on_sleep();
                    sys::set_backlight(0.0);
                    cv.clear(rgb(0));
                    disp.present(&cv.px, lw, rot, 1.0);
                    continue;
                }
            }
            let was_released = ptr.released;
            frame(&mut app, &mut cv, &mut text, &mut ptr, short);
            ptr.pressed = false;
            ptr.released = false;
            ptr.dy = 0.0;
            ptr.dx = 0.0;
            if was_released {
                ptr.capture = None;
                ptr.moved = false;
            }
            // Sofort darstellen statt nach der Schleife ein ZWEITES Mal das ganze Bild zu rendern:
            // frame() hat über seinen 2-Pass bereits den Endzustand gezeichnet (Navigation,
            // Umschalter, Scroll-Position). Das halbiert die Render-Last pro Touch-Event und macht
            // besonders langes Scrollen spürbar flinker. Bei geänderter Drehung die Schleife mit
            // neuer Geometrie neu zeichnen lassen.
            if app.set.rotation == rot {
                disp.present(&cv.px, lw, rot, if app.soft_bright { app.set.brightness } else { 1.0 });
                redraw = false;
            } else {
                redraw = true;
            }
        }

        // Benachrichtigungen einsammeln + alarmieren (auch wenn Bildschirm aus / in der Tasche)
        let (note_new, note_wake) = app.poll_notifications(asleep);
        if note_wake && asleep && !app.pocket_block() {
            asleep = false;
            app.apply_brightness();
        }
        if note_new || note_wake {
            redraw = true;
        }
        // Bildschirmfoto aus den Schnelleinstellungen (verzögert, damit die Leiste zu ist)
        if !asleep && app.take_shot() {
            frame(&mut app, &mut cv, &mut text, &mut ptr, short);
            let d = sys::from_unix(sys::unix_now(), app.set.tz_min);
            let path = format!("/data/Pictures/Screenshots/Screenshot_{:04}{:02}{:02}_{:02}{:02}{:02}.png", d.y, d.mo, d.d, d.h, d.mi, d.s);
            match sysx::save_png(&path, cv.w as u32, cv.h as u32, &cv.px) {
                Ok(()) => ultar_notify::post("system", "screenshot", app.tr("Screenshot saved"), &path, ultar_notify::Urgency::Low),
                Err(e) => eprintln!("Bildschirmfoto: {e}"),
            }
            sys::vibrate(40);
            redraw = true;
        }

        if asleep {
            // Wecker, Timer und Schlummern laufen auch bei ausgeschaltetem Bildschirm
            if app.alarm_tick() {
                asleep = false;
                app.apply_brightness();
                redraw = true;
            } else {
                continue;
            }
        }
        if app.set.timeout_s > 0 && last_input.elapsed().as_secs() >= app.set.timeout_s as u64 && !app.stay_awake() {
            asleep = true;
            app.on_sleep();
            sys::set_backlight(0.0);
            cv.clear(rgb(0));
            disp.present(&cv.px, lw, rot, 1.0);
            continue;
        }
        let now = sys::from_unix(sys::unix_now(), app.set.tz_min);
        if now.mi != last_min {
            last_min = now.mi;
            redraw = true;
        }
        redraw |= app.tick();
        redraw |= ui::scroll_animating();
        if redraw && app.set.rotation == rot {
            let tf = Instant::now();
            frame(&mut app, &mut cv, &mut text, &mut ptr, short);
            let ms = tf.elapsed().as_secs_f32() * 1000.0;
            app.perf.1 = ms;
            app.perf.2 = if app.perf.2 == 0.0 { ms } else { app.perf.2 * 0.8 + ms * 0.2 };
            if app.perf.0 == 0.0 {
                app.perf.0 = t0.elapsed().as_secs_f32() * 1000.0;
            }
            if app.desk.an {
                // Desktop: Pfeil als Mauszeiger (absolute Maus: Zeigerposition; relative: eigener Zeiger)
                let (x, y) = if input.has_mouse && !input.has_touch { map(input.cursor.0, input.cursor.1) } else { (ptr.x, ptr.y) };
                ultar_shell_zeiger(&mut cv, x, y, (short / 800.0).max(1.0));
            } else if input.has_mouse && !input.has_touch {
                let (x, y) = map(input.cursor.0, input.cursor.1);
                cv.set_clip(None);
                cv.circle(x, y, short * 0.014, rgb(0xFFFFFF));
                cv.circle(x, y, short * 0.009, rgb(0x2F6FED));
            }
            disp.present(&cv.px, lw, rot, if app.soft_bright { app.set.brightness } else { 1.0 });
            redraw = false;
        }
        if SHOT.swap(false, Ordering::SeqCst) {
            // Fern-Navigation für Tests über die Tür: /run/ultar/shell.goto = "<bildschirm> [sprache]"
            // (Namen wie bei --shot, z. B. "cells", "radio ar", "shade", "home"); danach Bildschirmfoto.
            if let Ok(req) = std::fs::read_to_string("/run/ultar/shell.goto") {
                let _ = std::fs::remove_file("/run/ultar/shell.goto");
                let mut it = req.split_whitespace();
                if let Some(name) = it.next() {
                    app.remote_open(name, it.next());
                    for _ in 0..2 {
                        app.tick();
                        frame(&mut app, &mut cv, &mut text, &mut ptr, short);
                        std::thread::sleep(Duration::from_millis(80));
                    }
                }
            }
            // frisch zeichnen und als PNG ablegen
            frame(&mut app, &mut cv, &mut text, &mut ptr, short);
            save_screenshot(&cv);
            redraw = true;
        }
    }
}

fn ultar_shell_zeiger(cv: &mut Canvas, x: f32, y: f32, s: f32) {
    app::app_desktop::zeiger(cv, x, y, s);
}
