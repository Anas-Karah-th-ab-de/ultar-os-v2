//! Test harness: an in-process compositor (headless display), the bridge as
//! a real process, a raw Wayland client and a raw native client.
#![allow(dead_code)]

pub mod app;
pub mod native;
pub mod wl;

use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState, PointerButton};
use ultar_surface::proto::OutputId;

/// Screen size of the tests.
pub const SCREEN: (u32, u32) = (1280, 800);

/// Compositor + bridge.
pub struct Harness {
    pub comp: Compositor,
    pub bridge: Child,
    pub dir: PathBuf,
    pub socket: PathBuf,
}

impl Harness {
    /// Starts the compositor and the bridge running `program`.
    pub fn start(name: &str, program: &[&str]) -> Harness {
        Harness::start_with(name, program, &[])
    }

    /// Same, with extra environment for the program.
    pub fn start_with(name: &str, program: &[&str], env: &[(&str, &str)]) -> Harness {
        let config = Config {
            lang: ultar_ui::Lang::En,
            cursor: false,
            ..Config::default()
        };
        Harness::launch(name, program, env, config)
    }

    /// Same, with the English layout first (typing ASCII into real apps).
    pub fn start_english(name: &str, program: &[&str], env: &[(&str, &str)]) -> Harness {
        use ultar_compositor::keymap::Layout;
        let layouts = vec![Layout::English, Layout::Arabic, Layout::German];
        let config = Config {
            lang: ultar_ui::Lang::En,
            cursor: false,
            layouts,
            ..Config::default()
        };
        Harness::launch(name, program, env, config)
    }

    fn launch(name: &str, program: &[&str], env: &[(&str, &str)], config: Config) -> Harness {
        let dir = std::env::temp_dir().join(format!("uwl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("run")).unwrap();
        let mut comp = Compositor::new(
            config,
            Box::new(Headless::new(&[SCREEN])),
            Box::new(MonotonicClock::default()),
        );
        let sock = dir.join("compositor");
        comp.set_listener(UnixListener::bind(&sock).unwrap())
            .unwrap();
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ultar-wayland"));
        cmd.args([
            "--compositor",
            sock.to_str().unwrap(),
            "--runtime-dir",
            dir.join("run").to_str().unwrap(),
            "--app-id",
            "test.app",
            "--",
        ])
        .args(program)
        .env("ULTAR_LANG", "en")
        .stdout(Stdio::null());
        for (k, v) in env {
            cmd.env(k, v);
        }
        let bridge = cmd.spawn().unwrap();
        let socket = dir.join("run/wayland-0");
        let mut h = Harness {
            comp,
            bridge,
            dir,
            socket,
        };
        let ok = h.pump_until(Duration::from_secs(10), |h| h.socket.exists());
        assert!(ok, "the bridge made its socket");
        h
    }

    /// Runs the compositor for `ms`.
    pub fn pump(&mut self, ms: u64) {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            self.comp.dispatch(Some(Duration::from_millis(2))).unwrap();
        }
    }

    /// Runs the compositor until `cond`.
    pub fn pump_until(
        &mut self,
        timeout: Duration,
        mut cond: impl FnMut(&mut Harness) -> bool,
    ) -> bool {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if cond(self) {
                return true;
            }
            self.comp.dispatch(Some(Duration::from_millis(2))).unwrap();
        }
        cond(self)
    }

    /// The screen.
    pub fn frame(&self) -> &ultar_compositor::pixels::Image {
        self.comp.frame(OutputId(0)).expect("output 0")
    }

    /// Pixels of exactly `color` on screen and their bounding box.
    pub fn find(&self, color: u32) -> (usize, Option<(u32, u32, u32, u32)>) {
        let f = self.frame();
        let (w, mut n) = (f.width(), 0);
        let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
        for (i, p) in f.data().iter().enumerate() {
            if *p & 0x00FF_FFFF == color & 0x00FF_FFFF {
                n += 1;
                let (x, y) = (i as u32 % w, i as u32 / w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
        }
        (n, (n > 0).then_some((x0, y0, x1, y1)))
    }

    /// A point inside a visible 9×9 square of `color` (to click on).
    pub fn spot(&self, color: u32) -> Option<(f64, f64)> {
        let f = self.frame();
        let (w, h) = (f.width() as i64, f.height() as i64);
        let is = |x: i64, y: i64| {
            x >= 0
                && y >= 0
                && x < w
                && y < h
                && f.data()[(y * w + x) as usize] & 0x00FF_FFFF == color & 0x00FF_FFFF
        };
        for y in (4..h - 4).step_by(3) {
            for x in (4..w - 4).step_by(3) {
                if (-4..=4).all(|d| is(x + d, y) && is(x, y + d) && is(x + d, y + d)) {
                    return Some((x as f64 + 0.5, y as f64 + 0.5));
                }
            }
        }
        None
    }

    /// Saves the screen as PNG under `ULTAR_WAYLAND_SHOTS` (if set).
    pub fn shot(&self, name: &str) {
        let Ok(dir) = std::env::var("ULTAR_WAYLAND_SHOTS") else {
            return;
        };
        let f = self.frame();
        let rgba: Vec<u8> = f
            .data()
            .iter()
            .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255])
            .collect();
        let img = ultar_ui::snapshot::Image {
            width: f.width(),
            height: f.height(),
            rgba,
        };
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            Path::new(&dir).join(format!("{name}.png")),
            img.encode_png().unwrap(),
        );
    }

    pub fn inject(&mut self, kind: InputEventKind) {
        self.comp.inject(&[InputEvent {
            device: InputDeviceId(0),
            time_us: 0,
            kind,
        }]);
        self.pump(10);
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        let (px, py) = (self.comp.state.pointer.x, self.comp.state.pointer.y);
        self.inject(InputEventKind::PointerMotion {
            dx: (x - px) as f32,
            dy: (y - py) as f32,
        });
    }

    pub fn click(&mut self, x: f64, y: f64) {
        self.move_to(x, y);
        self.inject(InputEventKind::PointerButton {
            button: PointerButton::Primary,
            pressed: true,
        });
        self.inject(InputEventKind::PointerButton {
            button: PointerButton::Primary,
            pressed: false,
        });
    }

    pub fn key(&mut self, code: u16, pressed: bool) {
        let state = if pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        };
        self.inject(InputEventKind::Key { code, state });
    }

    pub fn tap(&mut self, code: u16) {
        self.key(code, true);
        self.key(code, false);
    }

    /// The bridge's exit code within `timeout`.
    pub fn exit_code(&mut self, timeout: Duration) -> Option<i32> {
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            if let Ok(Some(s)) = self.bridge.try_wait() {
                return s.code();
            }
            self.pump(10);
        }
        None
    }

    pub fn alive(&mut self) -> bool {
        matches!(self.bridge.try_wait(), Ok(None))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = self.bridge.kill();
        let _ = self.bridge.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Deterministic xorshift.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// Waits until a window of `color` shows with at least `at_least` pixels;
/// its bounding box.
pub fn mapped(
    h: &mut Harness,
    app: &mut app::App,
    color: u32,
    at_least: usize,
) -> (u32, u32, u32, u32) {
    assert!(app.roundtrip(h), "the bridge answers");
    let found = h.pump_until(Duration::from_secs(5), |h| h.find(color).0 >= at_least);
    assert!(
        found,
        "window with colour {color:08x} on screen ({} pixels)",
        h.find(color).0
    );
    h.find(color).1.unwrap()
}

/// Alt+Tab: the compositor focuses the other window.
pub fn alt_tab(h: &mut Harness) {
    h.key(56, true);
    h.tap(15);
    h.key(56, false);
}
