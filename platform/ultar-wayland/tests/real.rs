//! Real Ubuntu Wayland apps through the bridge (only with
//! `ULTAR_WAYLAND_REAL=1`, apps that are not installed are skipped).
//! Each app runs with `WAYLAND_DEBUG=1`: its own log shows which events
//! reached it. Pictures go to `ULTAR_WAYLAND_SHOTS`.

mod common;

use common::Harness;
use std::path::{Path, PathBuf};
use std::time::Duration;

const LONG: Duration = Duration::from_secs(25);

/// One app and what it can show.
struct App {
    name: &'static str,
    program: Vec<String>,
    env: Vec<(&'static str, &'static str)>,
    /// A shell runs inside (typed commands can be checked).
    terminal: bool,
    /// Built on libwayland: `WAYLAND_DEBUG` shows the events it got.
    logs: bool,
}

fn installed(program: &str) -> bool {
    program.starts_with('/') && Path::new(program).exists()
        || std::env::var_os("PATH")
            .is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(program).exists()))
}

const QT_APP: &str = "import sys\nfrom PyQt6.QtWidgets import QApplication, QLineEdit\napp = QApplication(sys.argv)\nw = QLineEdit('Ultar Qt6 مرحبا')\nw.setWindowTitle('Qt6 on Ultar')\nw.resize(420, 120)\nw.show()\nsys.exit(app.exec())\n";

fn pyqt6() -> bool {
    std::process::Command::new("python3")
        .args(["-c", "import PyQt6.QtWidgets"])
        .status()
        .is_ok_and(|s| s.success())
}

fn app(
    name: &'static str,
    program: &[&str],
    env: Vec<(&'static str, &'static str)>,
    terminal: bool,
    logs: bool,
) -> App {
    let program = program.iter().map(|x| (*x).to_owned()).collect();
    App {
        name,
        program,
        env,
        terminal,
        logs,
    }
}

fn apps(dir: &Path) -> Vec<App> {
    let qt = dir.join("qt6.py");
    let _ = std::fs::write(&qt, QT_APP);
    let qt = qt.display().to_string();
    let python = if pyqt6() {
        "python3"
    } else {
        "PyQt6-not-installed"
    };
    let demo = std::env::var("UWL_DEMO").unwrap_or_else(|_| "uwl-demo".into());
    vec![
        app("uwl-demo", &[&demo], vec![], false, false),
        app(
            "weston-simple-shm",
            &["weston-simple-shm"],
            vec![],
            false,
            true,
        ),
        app(
            "weston-terminal",
            &["weston-terminal", "--shell=/bin/sh"],
            vec![],
            true,
            true,
        ),
        app("foot", &["foot", "/bin/sh"], vec![], true, true),
        app(
            "gtk4-demo",
            &["gtk4-demo"],
            vec![("GSK_RENDERER", "cairo")],
            false,
            true,
        ),
        app("qt6", &[python, &qt], vec![], false, true),
        app(
            "sdl3",
            &["/usr/libexec/installed-tests/SDL3/testsprite"],
            vec![],
            false,
            true,
        ),
    ]
}

/// US layout: key code and shift for an ASCII character.
fn key_of(c: char) -> Option<(u16, bool)> {
    const ROW: [(&str, &str, u16); 4] = [
        ("1234567890-=", "!@#$%^&*()_+", 2),
        ("qwertyuiop[]", "QWERTYUIOP{}", 16),
        ("asdfghjkl;'`", "ASDFGHJKL:\"~", 30),
        ("zxcvbnm,./", "ZXCVBNM<>?", 44),
    ];
    match c {
        ' ' => return Some((57, false)),
        '\n' => return Some((28, false)),
        '\\' => return Some((43, false)),
        '|' => return Some((43, true)),
        _ => {}
    }
    ROW.iter().find_map(|(plain, shifted, first)| {
        let at = |s: &str| s.chars().position(|x| x == c).map(|i| first + i as u16);
        at(plain)
            .map(|k| (k, false))
            .or_else(|| at(shifted).map(|k| (k, true)))
    })
}

fn type_text(h: &mut Harness, text: &str) {
    for c in text.chars() {
        let Some((code, shift)) = key_of(c) else {
            continue;
        };
        if shift {
            h.key(42, true);
        }
        h.tap(code);
        if shift {
            h.key(42, false);
        }
        h.pump(15);
    }
}

/// Pixels that differ from the empty screen, and their box.
fn changed(h: &Harness, empty: &[u32]) -> (usize, (u32, u32, u32, u32)) {
    let f = h.frame();
    let w = f.width();
    let (mut n, mut b) = (0, (u32::MAX, u32::MAX, 0, 0));
    for (i, (p, e)) in f.data().iter().zip(empty).enumerate() {
        if p != e {
            let (x, y) = (i as u32 % w, i as u32 / w);
            n += 1;
            b = (b.0.min(x), b.1.min(y), b.2.max(x), b.3.max(y));
        }
    }
    (n, b)
}

/// Waits until the picture holds still (no change for half a second).
fn settle(h: &mut Harness, empty: &[u32]) -> (usize, (u32, u32, u32, u32)) {
    let mut last = changed(h, empty);
    for _ in 0..20 {
        h.pump(500);
        let now = changed(h, empty);
        if now == last {
            break;
        }
        last = now;
    }
    last
}

fn log_has(log: &Path, parts: &[&str]) -> bool {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines().any(|l| parts.iter().all(|p| l.contains(p)))
}

/// The widest toplevel configure the app got.
fn widest_configure(log: &Path) -> i32 {
    let text = std::fs::read_to_string(log).unwrap_or_default();
    text.lines()
        .filter(|l| l.contains("xdg_toplevel") && l.contains(".configure("))
        .filter_map(|l| {
            l.split(".configure(")
                .nth(1)?
                .split(',')
                .next()?
                .trim()
                .parse::<i32>()
                .ok()
        })
        .max()
        .unwrap_or(0)
}

/// What one app showed.
#[derive(Default)]
struct Outcome {
    window: bool,
    click: Option<bool>,
    key: Option<bool>,
    typed: Option<bool>,
    clipboard: Option<bool>,
    resize: bool,
    close: bool,
}

impl Outcome {
    fn ok(&self) -> bool {
        self.window
            && self.click != Some(false)
            && self.key != Some(false)
            && self.typed != Some(false)
            && self.clipboard != Some(false)
            && self.resize
            && self.close
    }

    fn line(&self, name: &str) -> String {
        let y = |b: bool| if b { "ok" } else { "FAILED" };
        let o = |b: Option<bool>| b.map_or("-", y);
        format!(
            "REAL {name}: window {} click {} key {} typed {} clipboard {} resize {} close {}",
            y(self.window),
            o(self.click),
            o(self.key),
            o(self.typed),
            o(self.clipboard),
            y(self.resize),
            y(self.close)
        )
    }
}

fn start(app: &App, dir: &Path, log: &Path) -> Harness {
    let mut program: Vec<&str> = vec!["sh", "-c", "exec \"$0\" \"$@\" 2>\"$UWL_LOG\""];
    program.extend(app.program.iter().map(String::as_str));
    let mut env = vec![
        ("WAYLAND_DEBUG", "1"),
        ("SHELL", "/bin/sh"),
        ("UWL_LOG", log.to_str().unwrap_or("/dev/null")),
    ];
    env.extend(app.env.iter().copied());
    let home = dir.to_str().unwrap_or("/tmp");
    env.push(("HOME", home));
    Harness::start_english(&format!("real-{}", app.name), &program, &env)
}

/// Commands typed into the shell of a terminal; their files prove it.
fn terminal_checks(h: &mut Harness, dir: &Path, out: &mut Outcome) {
    let typed = dir.join("typed");
    type_text(h, &format!("echo ultar-typed-ok > {}\n", typed.display()));
    h.pump(1500);
    out.typed = Some(std::fs::read_to_string(&typed).is_ok_and(|t| t.trim() == "ultar-typed-ok"));
    if !installed("wl-copy") {
        return;
    }
    let pasted = dir.join("pasted");
    type_text(h, "printf ULTAR-CLIP-OK | wl-copy\n");
    h.pump(2500);
    type_text(h, &format!("wl-paste -n > {}\n", pasted.display()));
    h.pump(2500);
    out.clipboard = Some(std::fs::read_to_string(&pasted).is_ok_and(|t| t == "ULTAR-CLIP-OK"));
}

fn run_app(app: &App, base: &Path) -> Outcome {
    let dir = base.join(app.name);
    let _ = std::fs::create_dir_all(&dir);
    let log = dir.join("wayland.log");
    let mut h = start(app, &dir, &log);
    let empty = h.frame().data().to_vec();
    let mut out = Outcome {
        window: h.pump_until(LONG, |h| changed(h, &empty).0 > 4000),
        ..Outcome::default()
    };
    let (n, b) = settle(&mut h, &empty);
    h.shot(&format!("{}-window", app.name));
    if !out.window {
        return out;
    }
    h.click(f64::from(b.0 + b.2) / 2.0, f64::from(b.1 + b.3) / 2.0);
    h.pump(800);
    out.click = app.logs.then(|| log_has(&log, &["wl_pointer", ".button("]));
    if app.terminal {
        terminal_checks(&mut h, &dir, &mut out);
    } else {
        h.tap(30);
        h.pump(800);
    }
    out.key = app.logs.then(|| log_has(&log, &["wl_keyboard", ".key("]));
    let before = widest_configure(&log);
    h.key(125, true);
    h.tap(103);
    h.key(125, false);
    let (m, _) = settle(&mut h, &empty);
    h.shot(&format!("{}-maximized", app.name));
    out.resize = m > n + n / 2 && (!app.logs || widest_configure(&log) > before.max(0));
    h.key(56, true);
    h.tap(62);
    h.key(56, false);
    out.close = h.exit_code(Duration::from_secs(15)).is_some();
    out
}

#[test]
fn real_apps() {
    if std::env::var_os("ULTAR_WAYLAND_REAL").is_none() {
        return;
    }
    let base = PathBuf::from(
        std::env::var("ULTAR_WAYLAND_OUT").unwrap_or_else(|_| "/tmp/uwl-real".into()),
    );
    let _ = std::fs::create_dir_all(&base);
    let (mut ran, mut good) = (0, 0);
    for app in apps(&base) {
        if !installed(&app.program[0]) {
            println!("REAL {}: not installed, skipped", app.name);
            continue;
        }
        let out = run_app(&app, &base);
        println!("{}", out.line(app.name));
        ran += 1;
        good += usize::from(out.ok());
    }
    println!("WAYLAND-REAL: {good}/{ran} apps OK");
    assert!(ran > 0 && good == ran, "every installed app passes");
}
