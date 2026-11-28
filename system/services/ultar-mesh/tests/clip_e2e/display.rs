//! A real compositor (headless, in a thread of the test) and an app window on it that
//! copies and pastes through the real protocol.

use std::collections::BTreeMap;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_compositor::backend::Headless;
use ultar_compositor::{Compositor, Config, MonotonicClock, RightsTable};
use ultar_hal::{InputDeviceId, InputEvent, InputEventKind, KeyState};
use ultar_surface::client::Client;
use ultar_surface::proto::{
    BufferId, Configure, Event, PixelFormat, Rect, Request, Rights, Role, SurfaceId, TransferId,
};

type Job = Box<dyn FnOnce(&mut Compositor) + Send>;

/// The compositor of one device.
pub struct Display {
    jobs: mpsc::Sender<Job>,
    stop: Arc<AtomicBool>,
    pub socket: PathBuf,
}

impl Display {
    /// Listens at `socket`; the user of this test holds the bridge right (the mesh service
    /// of the test runs as that user, as `833 = "ui.clipboard-bridge"` says for the device).
    pub fn start(socket: &Path) -> Display {
        let listener = UnixListener::bind(socket).unwrap();
        let (jobs, inbox) = mpsc::channel::<Job>();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        std::thread::spawn(move || {
            let mut rights = RightsTable::default();
            rights.grant(ultar_service::effective_uid(), Rights::CLIPBOARD_BRIDGE);
            let config = Config {
                rights,
                cursor: false,
                inline_only: true,
                ..Config::default()
            };
            let mut comp = Compositor::new(
                config,
                Box::new(Headless::new(&[(320, 200)])),
                Box::new(MonotonicClock::default()),
            );
            comp.set_listener(listener).unwrap();
            while !flag.load(Ordering::SeqCst) {
                while let Ok(job) = inbox.try_recv() {
                    job(&mut comp);
                }
                let _ = comp.dispatch(Some(Duration::from_millis(5)));
            }
        });
        Display {
            jobs,
            stop,
            socket: socket.to_owned(),
        }
    }

    /// Runs `work` inside the compositor's thread and returns its result.
    pub fn run<R: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Compositor) -> R + Send + 'static,
    ) -> R {
        let (tx, rx) = mpsc::channel();
        let job: Job = Box::new(move |comp| {
            let _ = tx.send(work(comp));
        });
        self.jobs.send(job).unwrap();
        rx.recv_timeout(Duration::from_secs(10)).unwrap()
    }

    /// The person presses and releases a key.
    fn press(&self, code: u16) {
        self.run(move |comp| {
            for state in [KeyState::Pressed, KeyState::Released] {
                comp.inject(&[InputEvent {
                    device: InputDeviceId(0),
                    time_us: 0,
                    kind: InputEventKind::Key { code, state },
                }]);
            }
        });
    }
}

impl Drop for Display {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

const WINDOW: SurfaceId = SurfaceId(1);
const CHUNK: usize = 256 * 1024;

/// Fills the window in one colour at its configured size and shows it.
fn draw(client: &mut Client, configure: &Configure) {
    let scale = f64::from(configure.scale_milli) / 1000.0;
    let width = (f64::from(configure.width) * scale).round() as u32;
    let height = (f64::from(configure.height) * scale).round() as u32;
    let buffer = BufferId(1000);
    let create = Request::CreateInlineBuffer {
        buffer,
        width,
        height,
        format: PixelFormat::Xrgb8888,
    };
    client.send(&create).unwrap();
    let rows = ((1 << 20) / (width as usize * 4)).max(1);
    let mut y = 0usize;
    while y < height as usize {
        let n = rows.min(height as usize - y);
        let rect = Rect::new(0, y as i32, width, n as u32);
        let data = vec![0xAA; width as usize * n * 4];
        client
            .send(&Request::WritePixels { buffer, rect, data })
            .unwrap();
        y += n;
    }
    let attach = Request::Attach {
        surface: WINDOW,
        buffer,
    };
    client.send(&attach).unwrap();
    let commit = Request::Commit {
        surface: WINDOW,
        configure: configure.serial,
    };
    client.send(&commit).unwrap();
}

/// An app window with a clipboard of its own.
pub struct App {
    client: Client,
    offer: Option<(String, Vec<u8>)>,
    served: BTreeMap<u32, usize>,
    done: bool,
}

impl App {
    /// Opens a window, draws it and gives it the keyboard focus.
    pub fn open(display: &Display) -> App {
        let mut client = Client::connect(&display.socket, "test.app").unwrap();
        client
            .send(&Request::CreateSurface {
                surface: WINDOW,
                role: Role::Toplevel,
            })
            .unwrap();
        let configure = loop {
            match client.next_event(Some(Duration::from_secs(10))).unwrap() {
                Some(Event::Configure(c)) if c.surface == WINDOW => break c,
                Some(_) => {}
                None => panic!("no configure"),
            }
        };
        draw(&mut client, &configure);
        let focused = ultar_svckit::testkit::wait_until(Duration::from_secs(10), || {
            display.run(|comp| {
                let first = comp.state.windows.first().map(|w| w.key);
                first.map(|key| comp.state.focus_window(key)).is_some()
            })
        });
        assert!(focused, "the window appeared");
        App {
            client,
            offer: None,
            served: BTreeMap::new(),
            done: false,
        }
    }

    /// The next event; a request for the data of the clipboard entry is answered here.
    fn next(&mut self, wait: Duration) -> Option<Event> {
        let event = self.client.next_event(Some(wait)).ok().flatten()?;
        if let Event::SendRequest { transfer, mime } = &event {
            self.serve(*transfer, mime);
            return Some(Event::Ping { serial: 0 });
        }
        Some(event)
    }

    fn serve(&mut self, transfer: TransferId, mime: &str) {
        let data: &[u8] = match &self.offer {
            Some((m, d)) if m == mime => d,
            _ => &[],
        };
        let start = (*self.served.entry(transfer.0).or_insert(0)).min(data.len());
        let end = (start + CHUNK).min(data.len());
        let last = end == data.len();
        self.served.insert(transfer.0, end);
        self.done |= last;
        let chunk = data[start..end].to_vec();
        let _ = self.client.send(&Request::SendData {
            transfer,
            data: chunk,
            last,
        });
    }

    /// A fresh serial of a user action (a key press in this window).
    fn serial(&mut self, display: &Display) -> u32 {
        display.press(47);
        let end = Instant::now() + Duration::from_secs(5);
        while Instant::now() < end {
            if let Some(Event::Key {
                serial, state: 1, ..
            }) = self.next(Duration::from_millis(200))
            {
                return serial;
            }
        }
        panic!("the key did not reach the window");
    }

    /// The person copies `data` of type `mime` in this window; waits until the compositor's
    /// bridge has read it (or a moment passed: there may be no reader).
    pub fn copy(&mut self, display: &Display, mime: &str, data: &[u8]) {
        let serial = self.serial(display);
        self.offer = Some((mime.to_owned(), data.to_vec()));
        self.served.clear();
        self.done = false;
        self.client
            .send(&Request::SetSelection {
                serial,
                mimes: vec![mime.to_owned()],
            })
            .unwrap();
        let end = Instant::now() + Duration::from_millis(1500);
        while !self.done && Instant::now() < end {
            let _ = self.next(Duration::from_millis(50));
        }
    }

    /// The person pastes in this window: the data of the clipboard entry of type `mime`
    /// (`None`: nothing of that type, or the transfer was cancelled).
    pub fn paste(&mut self, display: &Display, mime: &str) -> Option<Vec<u8>> {
        let serial = self.serial(display);
        self.client
            .send(&Request::ReadSelection {
                serial,
                mime: mime.to_owned(),
            })
            .unwrap();
        let mut got = Vec::new();
        let end = Instant::now() + Duration::from_secs(4);
        while Instant::now() < end {
            match self.next(Duration::from_millis(100)) {
                Some(Event::Data { data, last, .. }) => {
                    got.extend_from_slice(&data);
                    if last {
                        return Some(got);
                    }
                }
                Some(Event::TransferCancelled { .. }) => return None,
                _ => {}
            }
        }
        None
    }

    /// The window is in a password field (or leaves it).
    pub fn password_field(&mut self, on: bool) {
        self.client
            .send(&Request::TextInput {
                surface: WINDOW,
                enabled: on,
                purpose: 2,
            })
            .unwrap();
        std::thread::sleep(Duration::from_millis(100));
    }
}
