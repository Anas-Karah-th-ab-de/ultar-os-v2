//! Ultar Web – Browser mit Rust-Kern für Ultar OS, ohne Ubuntu-Bereich, ohne Fenstersystem.
//! Aufbau wie Chrome: dieser Prozess ist der Browser-Prozess (Leisten, Tabs, Planung), WebKit startet je
//! Website einen eigenen Seitenprozess und einen Netzwerkprozess; der Compositor-Thread gibt das Bild aus.
//!
//! Aufruf: ultar-web [Adresse] [--profil NAME] [--app] [--desktop] [--shot DATEI.bmp [--warten S] [--groesse BxH]]
#![allow(dead_code)]

#[cfg(ultar_engine)]
mod browser;
#[cfg(ultar_engine)]
mod engine;
#[cfg(ultar_engine)]
mod ffi;
#[cfg(ultar_engine)]
mod platform;

#[cfg(ultar_engine)]
mod compositor;
mod i18n;
mod sandbox;
mod sched;

#[path = "../../../platform/ultar-shell/src/decon.rs"]
mod decon;
#[path = "../../../platform/ultar-shell/src/display.rs"]
mod display;
#[path = "../../../platform/ultar-shell/src/fern.rs"]
mod fern;
#[path = "../../../platform/ultar-shell/src/icons_svg.rs"]
mod icons_svg;
/// Data directory lookup shared with the shell sources included above.
mod sys {
    pub fn data_dir() -> String {
        std::env::var("ULTAR_DATA").unwrap_or_else(|_| "/data/ultar".into())
    }
}
#[path = "../../../platform/ultar-shell/src/gfx.rs"]
mod gfx;
#[path = "../../../platform/ultar-shell/src/input.rs"]
mod input;
#[path = "../../../platform/ultar-shell/src/linkcheck.rs"]
mod linkcheck;
#[path = "../../../platform/ultar-shell/src/text.rs"]
mod text;
#[path = "../../../platform/ultar-shell/src/ui.rs"]
mod ui;

mod brand {
    /// Bild für gfx::Canvas::image (im Browser derzeit ungenutzt)
    pub struct Img {
        pub w: u32,
        pub h: u32,
        pub rgba: &'static [u8],
    }
}

struct Args {
    url: String,
    profile: String,
    app: bool,
    desktop: bool,
    shot: Option<String>,
    wait: u64,
    size: (i32, i32),
}

fn args() -> Args {
    let mut a = Args {
        url: String::new(),
        profile: String::new(),
        app: false,
        desktop: false,
        shot: None,
        wait: 8,
        size: (720, 1520),
    };
    let mut it = std::env::args().skip(1);
    while let Some(x) = it.next() {
        match x.as_str() {
            "--profil" | "--profile" => a.profile = it.next().unwrap_or_default(),
            "--app" => a.app = true,
            "--desktop" => a.desktop = true,
            "--mobil" => a.desktop = false,
            "--shot" => a.shot = it.next(),
            "--warten" | "--wait" => a.wait = it.next().and_then(|s| s.parse().ok()).unwrap_or(8),
            "--groesse" | "--size" => {
                if let Some((w, h)) = it.next().and_then(|s| {
                    s.split_once('x')
                        .map(|(a, b)| (a.parse().unwrap_or(720), b.parse().unwrap_or(1520)))
                }) {
                    a.size = (w, h);
                }
            }
            "--version" => {
                println!(
                    "ultar-web {} (Rust-Kern, Engine: WPE WebKit)",
                    env!("CARGO_PKG_VERSION")
                );
                std::process::exit(0);
            }
            s if !s.starts_with("--") => a.url = s.to_string(),
            _ => {}
        }
    }
    a
}

/// Einstellungen des Systems (Sprache, Farbschema, Akzent) aus /data/ultar/settings.toml
fn system_settings() -> (String, bool, usize) {
    let s = std::fs::read_to_string("/data/ultar/settings.toml").unwrap_or_default();
    let get = |k: &str| {
        s.lines().find_map(|l| {
            l.trim()
                .strip_prefix(k)
                .and_then(|r| r.trim_start().strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
        })
    };
    let lang = std::env::var("ULTAR_LANG")
        .ok()
        .or_else(|| get("lang"))
        .unwrap_or_else(|| "ar".into());
    let dark = get("dark").map(|v| v != "false").unwrap_or(true);
    let accent = get("accent").and_then(|v| v.parse().ok()).unwrap_or(0);
    (lang, dark, accent)
}

/// Profilname aus der Adresse (wie webapp.sh): web.whatsapp.com -> whatsapp
fn profile_for(url: &str, app: bool) -> String {
    if !app {
        return "browser".into();
    }
    let h = url
        .split_once("://")
        .map(|x| x.1)
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or("")
        .to_lowercase();
    for (k, v) in [
        ("whatsapp.com", "whatsapp"),
        ("telegram.org", "telegram"),
        ("youtube.com", "youtube"),
        ("spotify.com", "spotify"),
    ] {
        if h.contains(k) {
            return v.into();
        }
    }
    let s: String = h
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.is_empty() {
        "browser".into()
    } else {
        s
    }
}

#[cfg(not(ultar_engine))]
fn main() {
    let _ = args();
    eprintln!("ultar-web wurde ohne Web-Engine gebaut (ULTAR_WEB_SYSROOT fehlte beim Bauen).");
    std::process::exit(2);
}

#[cfg(ultar_engine)]
fn main() {
    use std::sync::mpsc::channel;
    use std::sync::Arc;
    let a = args();
    let t0 = std::time::Instant::now();
    let bundle = std::env::var("ULTAR_WEB_BUNDLE").unwrap_or_else(|_| "/usr/lib/ultar/web".into());
    let (lang, dark, accent) = system_settings();
    let app = a.app || std::env::var("ULTAR_WEB_APP").is_ok();
    let profile = if a.profile.is_empty() {
        profile_for(&a.url, app)
    } else {
        a.profile.clone()
    };
    let base = if std::path::Path::new("/data/ultar").is_dir() && a.shot.is_none() {
        "/data/ultar/web".to_string()
    } else {
        format!(
            "{}/.ultar-web",
            std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())
        )
    };
    let data_dir = format!("{base}/{profile}");
    let _ = std::fs::create_dir_all(&data_dir);
    let desktop = a.desktop || matches!(profile.as_str(), "whatsapp" | "telegram" | "spotify");

    // WebKit sucht seine Hilfsprozesse und das Injected Bundle an fest eingebauten Orten:
    // dorthin zeigen Verweise ins Bündel (werden beim Start als root angelegt, falls sie fehlen)
    link_fixed_paths(&bundle);
    // Laufzeit aus dem eigenen Bündel (kein Ubuntu-Bereich)
    let env = |k: &str, v: String| {
        if std::env::var(k).is_err() {
            std::env::set_var(k, v)
        }
    };
    env(
        "WEBKIT_EXEC_PATH",
        format!("{bundle}/libexec/wpe-webkit-2.0"),
    );
    env("GIO_MODULE_DIR", format!("{bundle}/lib/gio/modules"));
    env(
        "WEBKIT_INJECTED_BUNDLE_PATH",
        format!("{bundle}/lib/wpe-webkit-2.0/injected-bundle"),
    );
    env("GCONV_PATH", format!("{bundle}/lib/gconv"));
    env("LANG", "C.UTF-8".into());
    // Ohne GPU: Skia zeichnet auf der CPU direkt in Shared Memory (Patch 0002 in ultar-wpewebkit)
    if std::env::var("ULTAR_WEB_GPU").is_err() {
        env("WEBKIT_DISABLE_HARDWARE_ACCELERATION", "1".into());
    }
    // Hardware-Videodekoder nur zuschalten, wenn er sich ohne Hänger öffnen lässt (Exynos-MFC braucht Firmware)
    let hw = hw_decoder_ok();
    env(
        "GST_PLUGIN_SYSTEM_PATH_1_0",
        if hw {
            format!("{bundle}/lib/gstreamer-1.0:{bundle}/lib/gstreamer-hw")
        } else {
            format!("{bundle}/lib/gstreamer-1.0")
        },
    );
    env(
        "GST_PLUGIN_SCANNER_1_0",
        format!("{bundle}/libexec/gstreamer-1.0/gst-plugin-scanner"),
    );
    env("GST_REGISTRY_1_0", format!("{data_dir}/gst-registry.bin"));
    env("FONTCONFIG_FILE", format!("{bundle}/etc/fonts/fonts.conf"));
    env("XKB_CONFIG_ROOT", format!("{bundle}/share/X11/xkb"));
    env(
        "WEBKIT_TLS_CAFILE_PEM",
        format!("{bundle}/etc/ssl/certs/ca-certificates.crt"),
    );
    env(
        "SSL_CERT_FILE",
        format!("{bundle}/etc/ssl/certs/ca-certificates.crt"),
    );
    env("XDG_DATA_HOME", format!("{data_dir}/share"));
    env("XDG_CACHE_HOME", format!("{data_dir}/cache"));
    env("XDG_CONFIG_HOME", format!("{data_dir}/config"));
    env("XDG_RUNTIME_DIR", format!("{data_dir}/run"));
    let _ = std::fs::create_dir_all(format!("{data_dir}/run"));
    sched::configure_env();

    // Bildschirm und Eingabe (als root), danach Rechte abgeben
    let (scr, inp): (Box<dyn compositor::Screen>, Option<input::Input>) = if a.shot.is_some() {
        (Box::new(compositor::NullScreen(a.size.0, a.size.1)), None)
    } else {
        match display::Display::open() {
            Ok(d) => (Box::new(Disp(d)), Some(input::Input::new())),
            Err(e) => {
                eprintln!("ultar-web: kein Bildschirm: {e}");
                std::process::exit(1);
            }
        }
    };
    let (w, h) = scr.size();
    let mut inp = inp;
    if let Some(i) = inp.as_mut() {
        // Geräte jetzt öffnen, solange noch Rechte da sind
        let fd = eventfd();
        let _ = i.wait(0, unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) });
    }
    let (touch, keyboard) = inp
        .as_ref()
        .map(|i| (i.has_touch, true))
        .unwrap_or((true, true));
    let dropped = sandbox::drop_privileges(&data_dir, sched::topo().ram_mb);
    let _ = std::fs::create_dir_all(format!("{data_dir}/run"));
    eprintln!("ultar-web: {}x{} · {} Kerne (schnell {:?}) · Profil {profile} · Rechte abgegeben: {dropped}", w, h, sched::topo().ncpu, sched::topo().big);

    // Wecker für die GLib-Ereignisschleife (aus Eingabe- und Compositor-Thread)
    let wake_fd = eventfd();
    let quit_fd = eventfd();
    let wake = move || unsafe {
        let one: u64 = 1;
        libc::write(wake_fd, &one as *const u64 as *const _, 8);
    };
    let (ctx, crx) = channel::<compositor::Msg>();
    let (otx, orx) = channel::<compositor::Out>();
    let (itx, irx) = channel::<input::Ev>();
    let stats = Arc::new(compositor::Stats::default());
    let th = ui::theme(dark, accent);
    let bg = (th.bg.r as u32) << 16 | (th.bg.g as u32) << 8 | th.bg.b as u32;
    {
        let st = stats.clone();
        let wk = wake;
        std::thread::Builder::new()
            .name("compositor".into())
            .spawn(move || compositor::run(scr, crx, otx, wk, st, bg))
            .expect("Compositor-Thread");
    }
    if let Some(mut i) = inp {
        let wk = wake;
        std::thread::Builder::new()
            .name("eingabe".into())
            .spawn(move || {
                sched::pin_current_to_big();
                loop {
                    let evs = i.wait(10_000, unsafe {
                        std::os::fd::BorrowedFd::borrow_raw(quit_fd)
                    });
                    if evs.is_empty() {
                        continue;
                    }
                    for e in evs {
                        if itx.send(e).is_err() {
                            return;
                        }
                    }
                    wk();
                }
            })
            .expect("Eingabe-Thread");
    }

    unsafe {
        use ffi::*;
        let lp = g_main_loop_new(std::ptr::null_mut(), FALSE);
        unsafe extern "C" fn on_wake(
            fd: std::ffi::c_int,
            _c: std::ffi::c_uint,
            _d: gpointer,
        ) -> gboolean {
            let mut v = 0u64;
            libc::read(fd, &mut v as *mut u64 as *mut _, 8);
            browser::pump();
            TRUE
        }
        unsafe extern "C" fn on_tick(_d: gpointer) -> gboolean {
            browser::with(|b| b.tick());
            TRUE
        }
        g_unix_fd_add(wake_fd, 1, Some(on_wake), std::ptr::null_mut());
        g_timeout_add(10_000, Some(on_tick), std::ptr::null_mut());
        unsafe extern "C" fn on_usr1(_d: gpointer) -> gboolean {
            browser::with(|b| b.screenshot());
            TRUE
        }
        unsafe extern "C" fn on_term(_d: gpointer) -> gboolean {
            browser::with(|b| b.quit());
            TRUE
        }
        // SIGUSR1 gehört JavaScriptCore (Garbage Collector) – Bildschirmfoto daher auf SIGUSR2
        g_unix_signal_add(libc::SIGUSR2, Some(on_usr1), std::ptr::null_mut());
        g_unix_signal_add(libc::SIGTERM, Some(on_term), std::ptr::null_mut());
        let opts = browser::Opts {
            url: a.url.clone(),
            profile: profile.clone(),
            data_dir: data_dir.clone(),
            bundle: bundle.clone(),
            desktop,
            app,
            lang: i18n::Lang::from(&lang),
            dark,
            accent,
            shot: a.shot.clone(),
            shot_wait: a.wait,
        };
        let sink_tx = ctx.clone();
        platform::set_frame_sink(move |f| {
            if let Err(e) = sink_tx.send(compositor::Msg::Content(f)) {
                if let compositor::Msg::Content(f) = e.0 {
                    platform::frame_done(f.view, f.buffer);
                }
            }
        });
        platform::set_waker(browser::schedule);
        let b = browser::Browser::new(opts, w, h, touch, keyboard, ctx, orx, irx, stats, lp);
        browser::install(b);
        browser::with(|b| b.start());
        eprintln!("ultar-web: bereit nach {} ms", t0.elapsed().as_millis());
        g_main_loop_run(lp);
        let code = browser::with(|b| b.exit_code()).unwrap_or(0);
        let one: u64 = 1;
        libc::write(quit_fd, &one as *const u64 as *const _, 8);
        std::process::exit(code);
    }
}

/// Videodekoder-Knoten (v4l2-m2m, z. B. Exynos MFC) prüfen: in einem Kindprozess mit 1,5 s Zeitlimit öffnen.
/// Blockiert ein Knoten (fehlende Firmware), bleibt die Hardware-Dekodierung aus. Bei Erfolg bekommt die
/// Browsergruppe Zugriff nur auf die Dekoder-Knoten (nicht auf Kameras).
fn hw_decoder_ok() -> bool {
    let nodes: Vec<String> = std::fs::read_dir("/sys/class/video4linux")
        .map(|rd| {
            rd.flatten()
                .filter(|e| {
                    let n = std::fs::read_to_string(e.path().join("name"))
                        .unwrap_or_default()
                        .to_lowercase();
                    n.contains("mfc") || n.contains("dec")
                })
                .map(|e| format!("/dev/{}", e.file_name().to_string_lossy()))
                .collect()
        })
        .unwrap_or_default();
    if nodes.is_empty() {
        return false;
    }
    unsafe {
        let pid = libc::fork();
        if pid == 0 {
            for n in &nodes {
                if let Ok(c) = std::ffi::CString::new(n.as_str()) {
                    let fd = libc::open(c.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK);
                    if fd >= 0 {
                        libc::close(fd);
                    }
                }
            }
            libc::_exit(0);
        }
        if pid < 0 {
            return false;
        }
        let t0 = std::time::Instant::now();
        loop {
            let mut st = 0;
            if libc::waitpid(pid, &mut st, libc::WNOHANG) == pid {
                break;
            }
            if t0.elapsed().as_millis() > 1500 {
                // nicht warten: ein im Kernel hängendes open() endet erst nach dessen Zeitlimit
                libc::kill(pid, libc::SIGKILL);
                eprintln!("ultar-web: Videodekoder blockiert – Software-Dekodierung");
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        for n in &nodes {
            if let Ok(c) = std::ffi::CString::new(n.as_str()) {
                libc::chown(c.as_ptr(), 0, sandbox::WEB_GID);
                libc::chmod(c.as_ptr(), 0o660);
            }
        }
    }
    true
}

/// Feste Pfade von WPE WebKit (Release-Bau) auf das Bündel verweisen lassen.
pub fn link_fixed_paths(bundle: &str) {
    for (fixed, target) in [
        (
            "/usr/libexec/wpe-webkit-2.0",
            format!("{bundle}/libexec/wpe-webkit-2.0"),
        ),
        (
            "/usr/lib/wpe-webkit-2.0",
            format!("{bundle}/lib/wpe-webkit-2.0"),
        ),
        (
            "/usr/share/wpe-webkit-2.0",
            format!("{bundle}/share/wpe-webkit-2.0"),
        ),
    ] {
        let p = std::path::Path::new(fixed);
        if std::fs::read_link(p)
            .map(|t| t.to_string_lossy() == target)
            .unwrap_or(false)
            || !std::path::Path::new(&target).exists()
        {
            continue;
        }
        if p.symlink_metadata().is_ok() {
            if p.is_dir() && !p.is_symlink() {
                continue; // echter Ordner (z. B. anderes WebKit) bleibt unangetastet
            }
            let _ = std::fs::remove_file(p);
        }
        if let Some(parent) = p.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(e) = std::os::unix::fs::symlink(&target, p) {
            eprintln!("ultar-web: Verweis {fixed} -> {target} nicht anlegbar: {e}");
        }
    }
}

fn eventfd() -> i32 {
    unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) }
}

/// Bildschirm des Systems (DRM, DECON, fbdev) für den Compositor-Thread
#[cfg(ultar_engine)]
struct Disp(display::Display);
// Display gehört ab dem Start allein dem Compositor-Thread
#[cfg(ultar_engine)]
unsafe impl Send for Disp {}
#[cfg(ultar_engine)]
impl compositor::Screen for Disp {
    fn size(&self) -> (i32, i32) {
        (self.0.w as i32, self.0.h as i32)
    }
    fn present(&mut self, px: &[u32], w: i32) {
        self.0.present(px, w, 0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn profile() {
        assert_eq!(
            super::profile_for("https://web.whatsapp.com/", true),
            "whatsapp"
        );
        assert_eq!(
            super::profile_for("https://example.org/x", false),
            "browser"
        );
    }
}
