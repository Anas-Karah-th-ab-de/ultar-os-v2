//! Shared test helpers: an in-process peer speaking the raw protocol, a
//! harness that runs client *processes* against an in-process compositor,
//! a fixed-seed random generator and snapshot comparison.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver};
use std::time::{Duration, Instant};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState, PointerButton};
use ultar_surface::proto::{
    BufferId, Configure, Event, PixelFormat, Rect, Request, Role, SurfaceId, PROTOCOL_VERSION,
};
use ultar_surface::wire::FrameReader;

/// Deterministic xorshift generator.
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

/// A compositor with headless outputs (cursor off for pixel tests).
pub fn compositor(config: Config, sizes: &[(u32, u32)]) -> Compositor {
    let config = Config {
        cursor: false,
        ..config
    };
    Compositor::new(
        config,
        Box::new(Headless::new(sizes)),
        Box::new(MonotonicClock::default()),
    )
}

/// English, inline transport (so tests need no shared memory), no cursor.
pub fn test_config() -> Config {
    Config {
        lang: ultar_ui::Lang::En,
        inline_only: true,
        cursor: false,
        ..Config::default()
    }
}

/// An in-process client using the raw protocol.
pub struct Peer {
    pub stream: UnixStream,
    out: Vec<u8>,
    buffers: Vec<u32>,
    reader: FrameReader,
    pub events: Vec<Event>,
    pub closed: bool,
}

impl Peer {
    pub fn connect(comp: &mut Compositor) -> Peer {
        let (a, b) = UnixStream::pair().unwrap();
        comp.add_stream(a).expect("client accepted");
        b.set_nonblocking(true).unwrap();
        Peer {
            stream: b,
            out: Vec::new(),
            buffers: Vec::new(),
            reader: FrameReader::new(),
            events: Vec::new(),
            closed: false,
        }
    }

    pub fn hello(comp: &mut Compositor) -> Peer {
        let mut p = Peer::connect(comp);
        p.send(&Request::Hello {
            version: PROTOCOL_VERSION,
            app_id: "test".into(),
            shared_memory: false,
        });
        p.pump(comp);
        assert!(p.events.iter().any(|e| matches!(e, Event::Welcome { .. })));
        p
    }

    pub fn send(&mut self, r: &Request) {
        let frame = r.encode().unwrap();
        self.raw(&frame);
    }

    /// Queues bytes; [`Peer::pump`] writes them while the compositor runs
    /// (both ends live in this thread, so a blocking write would deadlock).
    pub fn raw(&mut self, bytes: &[u8]) {
        self.out.extend_from_slice(bytes);
        self.write_some();
    }

    fn write_some(&mut self) {
        while !self.out.is_empty() {
            match self.stream.write(&self.out) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    self.out.drain(..n);
                }
            }
        }
    }

    /// Lets the compositor handle what was sent, then reads its answers.
    pub fn pump(&mut self, comp: &mut Compositor) {
        let (mut idle, mut stalls) = (0, 0);
        while idle < 3 && stalls < 1000 {
            let before = self.out.len();
            self.write_some();
            comp.dispatch(Some(Duration::ZERO)).unwrap();
            self.read();
            if self.closed {
                self.out.clear();
            }
            if self.out.is_empty() {
                idle += 1;
            } else if self.out.len() == before {
                stalls += 1;
            }
        }
    }

    pub fn read(&mut self) {
        let mut buf = vec![0u8; 1 << 16];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => self.reader.push(&buf[..n]),
                Err(_) => break,
            }
        }
        while let Ok(Some((op, body))) = self.reader.next_frame() {
            if let Ok(e) = Event::decode(op, &body) {
                if let Event::Ping { serial } = e {
                    self.send(&Request::Pong { serial });
                }
                self.events.push(e);
            }
        }
    }

    pub fn take(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn last_configure(&self, surface: SurfaceId) -> Option<Configure> {
        self.events.iter().rev().find_map(|e| match e {
            Event::Configure(c) if c.surface == surface => Some(*c),
            _ => None,
        })
    }

    pub fn error(&self) -> Option<ultar_surface::proto::ErrorCode> {
        self.events.iter().find_map(|e| match e {
            Event::Error { code } => Some(*code),
            _ => None,
        })
    }

    /// Creates a toplevel and draws it in one colour at its configured size.
    pub fn window(&mut self, comp: &mut Compositor, id: u32, color: u32) -> SurfaceId {
        let s = SurfaceId(id);
        self.send(&Request::CreateSurface {
            surface: s,
            role: Role::Toplevel,
        });
        self.pump(comp);
        self.draw(comp, s, color);
        s
    }

    /// Draws the surface at its last configured size.
    pub fn draw(&mut self, comp: &mut Compositor, s: SurfaceId, color: u32) {
        let c = self.last_configure(s).expect("configured");
        let scale = f64::from(c.scale_milli) / 1000.0;
        let (w, h) = (
            (f64::from(c.width) * scale).round() as u32,
            (f64::from(c.height) * scale).round() as u32,
        );
        self.fill(comp, s, c.serial, (w, h), color);
    }

    pub fn fill(
        &mut self,
        comp: &mut Compositor,
        s: SurfaceId,
        serial: u32,
        (w, h): (u32, u32),
        color: u32,
    ) {
        let b = BufferId(1000 + s.0);
        if self.buffers.contains(&b.0) {
            self.send(&Request::DestroyBuffer { buffer: b });
        }
        self.buffers.push(b.0);
        self.send(&Request::CreateInlineBuffer {
            buffer: b,
            width: w,
            height: h,
            format: PixelFormat::Xrgb8888,
        });
        let px = (color | 0xff00_0000).to_le_bytes();
        let rows_per = ((1 << 20) / (w as usize * 4)).max(1);
        let mut y = 0;
        while y < h as usize {
            let n = rows_per.min(h as usize - y);
            let data: Vec<u8> = px
                .iter()
                .copied()
                .cycle()
                .take(w as usize * n * 4)
                .collect();
            self.send(&Request::WritePixels {
                buffer: b,
                rect: Rect::new(0, y as i32, w, n as u32),
                data,
            });
            y += n;
        }
        self.send(&Request::Attach {
            surface: s,
            buffer: b,
        });
        self.send(&Request::Commit {
            surface: s,
            configure: serial,
        });
        self.pump(comp);
    }
}

pub fn ev(kind: InputEventKind) -> InputEvent {
    InputEvent {
        device: InputDeviceId(0),
        time_us: 0,
        kind,
    }
}

/// Pointer to an absolute logical position on a single 1:1 output of
/// `size`, then optionally a click.
pub fn move_to(comp: &mut Compositor, x: f64, y: f64) {
    let (px, py) = (comp.state.pointer.x, comp.state.pointer.y);
    comp.inject(&[ev(InputEventKind::PointerMotion {
        dx: (x - px) as f32,
        dy: (y - py) as f32,
    })]);
}

pub fn click(comp: &mut Compositor, x: f64, y: f64) {
    move_to(comp, x, y);
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: true,
    })]);
    comp.inject(&[ev(InputEventKind::PointerButton {
        button: PointerButton::Primary,
        pressed: false,
    })]);
}

pub fn key(comp: &mut Compositor, code: u16, pressed: bool) {
    let state = if pressed {
        KeyState::Pressed
    } else {
        KeyState::Released
    };
    comp.inject(&[ev(InputEventKind::Key { code, state })]);
}

/// A client process with its stdout lines.
pub struct Proc {
    pub child: Child,
    lines: Receiver<String>,
    pub seen: Vec<String>,
}

pub fn testclient() -> &'static str {
    env!("CARGO_BIN_EXE_ultar-compositor-testclient")
}

pub fn spawn(program: &str, args: &[&str]) -> Proc {
    let mut child = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let out = child.stdout.take().unwrap();
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        for line in BufReader::new(out).lines().map_while(Result::ok) {
            if tx.send(line).is_err() {
                break;
            }
        }
    });
    Proc {
        child,
        lines: rx,
        seen: Vec::new(),
    }
}

impl Proc {
    /// Drives the compositor until the process printed a line starting with
    /// `prefix` (true) or `timeout` passed (false).
    pub fn wait_line(&mut self, comp: &mut Compositor, prefix: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            while let Ok(line) = self.lines.try_recv() {
                self.seen.push(line);
            }
            if self.seen.iter().any(|l| l.starts_with(prefix)) {
                self.seen.retain(|l| !l.starts_with(prefix));
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            comp.dispatch(Some(Duration::from_millis(5))).unwrap();
        }
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A fresh directory for sockets.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-comp-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Listens on `dir/socket`.
pub fn listen(comp: &mut Compositor, dir: &Path) -> PathBuf {
    let path = dir.join("socket");
    let listener = UnixListener::bind(&path).unwrap();
    comp.set_listener(listener).unwrap();
    path
}

/// Drives the compositor until `cond` holds or `timeout` passes.
pub fn pump_until(
    comp: &mut Compositor,
    timeout: Duration,
    mut cond: impl FnMut(&Compositor) -> bool,
) -> bool {
    let deadline = Instant::now() + timeout;
    while !cond(comp) {
        if Instant::now() >= deadline {
            return false;
        }
        comp.dispatch(Some(Duration::from_millis(5))).unwrap();
    }
    true
}

/// Compares an image with `tests/snapshots/<name>.png`
/// (`ULTAR_UPDATE_SNAPSHOTS=1` writes it).
pub fn assert_snapshot(name: &str, image: &ultar_compositor::pixels::Image) {
    let rgba: Vec<u8> = image
        .data()
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, 255])
        .collect();
    let snap = ultar_ui::snapshot::Image {
        width: image.width(),
        height: image.height(),
        rgba,
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(format!("{name}.png"));
    if std::env::var("ULTAR_UPDATE_SNAPSHOTS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, snap.encode_png().unwrap()).unwrap();
        return;
    }
    let bytes =
        std::fs::read(&path).unwrap_or_else(|_| panic!("missing snapshot {}", path.display()));
    let reference = ultar_ui::snapshot::Image::decode_png(&bytes, &path).unwrap();
    let (differ, _) = snap.diff(&reference);
    assert_eq!(differ, 0, "{name}: {differ} pixels differ");
}
