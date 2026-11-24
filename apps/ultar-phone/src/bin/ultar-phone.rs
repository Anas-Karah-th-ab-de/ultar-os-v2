//! `ultar-phone` – the Phone app as a compositor client.
//!
//! ```text
//! ultar-phone [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR` or the shipped socket, `--bus` to
//! `$ULTAR_BUS_SOCKET`. Without a reachable call service (no bus, no modem, no right
//! `call.*`) the window still opens: the dial pad shows and every request answers that the
//! call service is not reachable (the app's own error texts), nothing is faked.
//!
//! The window opens first, at once, showing "no modem" until the call service answered: the
//! bus (connect, `call.snapshot`, `call.log`, the `call.state` subscription) is reached on a
//! thread of its own, and every request of a click waits at most [`CALL`].
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use ultar_bus::messages::call::{CallState, LogEntry};
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_phone::port::{CallPort, PortError, Request};
use ultar_phone::{BusPort, Look, PhoneApp, PhoneWindow, QueuedHost, Step};
use ultar_surface::client::Client;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::Size;

/// App id towards compositor and bus.
const APP_ID: &str = "org.ultar.phone";
/// The shipped compositor socket.
const COMPOSITOR: &str = "/run/ultar/ui/compositor.sock";
/// How long one request of a click to the call service may take (it runs on the window's
/// thread; the service answers at once, a service that does not is reported as unreachable).
const CALL: Duration = Duration::from_millis(1500);
/// Limit for the bus handshake and for the first questions at start (bus thread).
const START: Duration = Duration::from_secs(3);

struct Options {
    compositor: PathBuf,
    bus: Option<PathBuf>,
    lang: Lang,
    light: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR").unwrap_or_else(|| PathBuf::from(COMPOSITOR)),
        bus: env("ULTAR_BUS_SOCKET"),
        lang: Lang::from_env(),
        light: false,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = PathBuf::from(args.next()?),
            "--bus" => o.bus = Some(PathBuf::from(args.next()?)),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            _ => return None,
        }
    }
    Some(o)
}

/// The call service over the bus, or nothing when the bus cannot be reached (also while
/// the bus thread has not answered yet).
enum Port {
    Bus(BusPort),
    Unreachable,
}

/// What the bus thread learnt.
enum FromBus {
    /// Connected; the service answered (`state`, `log`) or not (`None`: no call service).
    Up {
        bus: Bus,
        service: Option<(CallState, Vec<LogEntry>)>,
    },
    /// A new `call.state`.
    State(CallState),
}

/// Connects to the bus and asks the call service, off the window's thread.
fn bus_thread(path: PathBuf, tx: Sender<FromBus>) {
    let _ = std::thread::Builder::new()
        .name("phone-bus".into())
        .spawn(move || bus_work(&path, &tx));
}

fn bus_work(path: &Path, tx: &Sender<FromBus>) {
    let options = ClientOptions {
        io_timeout: START,
        ..ClientOptions::named(APP_ID)
    };
    let Ok(bus) = Bus::connect(path, options) else {
        return;
    };
    let port = BusPort::new(bus.clone(), START);
    let service = match port.snapshot() {
        Ok(state) => {
            let mut port = port;
            let log = port.log(false).unwrap_or_default();
            Some((state, log))
        }
        Err(_) => None,
    };
    let live = service.is_some();
    if tx
        .send(FromBus::Up {
            bus: bus.clone(),
            service,
        })
        .is_err()
        || !live
    {
        return;
    }
    let Ok(states) = BusPort::new(bus, START).states() else {
        return;
    };
    while let Ok(state) = states.recv() {
        if tx.send(FromBus::State(state)).is_err() {
            return;
        }
    }
}

impl CallPort for Port {
    fn send(&mut self, request: Request) -> Result<Option<u32>, PortError> {
        match self {
            Port::Bus(p) => p.send(request),
            Port::Unreachable => Err(PortError::Unreachable),
        }
    }

    fn log(&mut self, missed_only: bool) -> Result<Vec<LogEntry>, PortError> {
        match self {
            Port::Bus(p) => p.log(missed_only),
            Port::Unreachable => Err(PortError::Unreachable),
        }
    }
}

fn wall_s() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0))
}

fn run(o: Options) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    if let Some(path) = o.bus.clone() {
        bus_thread(path, tx);
    }
    let look = Look {
        lang: o.lang,
        theme: if o.light {
            ThemeKind::Light
        } else {
            ThemeKind::Dark
        },
        mode: InputMode::Pointer,
        size: Size {
            w: 1024.0,
            h: 680.0,
        },
    };
    let host = QueuedHost::default();
    let app = PhoneApp::new(look, CallState::default(), Port::Unreachable, host.clone())
        .map_err(|e| e.to_string())?;
    let client = Client::connect(&o.compositor, APP_ID).map_err(|e| e.to_string())?;
    let title = app.ui_ref().translator().t("phone.title").into_owned();
    let mut window = PhoneWindow::open(client, app, host, &title).map_err(|e| e.to_string())?;
    let mut rx = Some(rx);
    loop {
        match window
            .step(Duration::from_millis(100), wall_s())
            .map_err(|e| e.to_string())?
        {
            Step::Closed => return Ok(()),
            Step::Running | Step::Forward(_) => {}
        }
        if let Some(r) = &rx {
            if !from_bus(&mut window, r) {
                rx = None;
            }
        }
    }
}

/// Takes what the bus thread learnt; false once it ended.
fn from_bus(window: &mut PhoneWindow<Port>, rx: &Receiver<FromBus>) -> bool {
    loop {
        let message = match rx.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => return true,
            Err(TryRecvError::Disconnected) => return false,
        };
        let now = window.now_ms();
        match message {
            FromBus::Up { bus, service } => {
                window.set_launcher(bus.clone());
                if let Some((state, log)) = service {
                    *window.app().port() = Port::Bus(BusPort::new(bus, CALL));
                    // A failed rebuild keeps the last tree on screen; the next change retries.
                    let _ = window.app().set_log(log);
                    let _ = window.app().set_state(state, now);
                }
            }
            FromBus::State(state) => {
                let _ = window.app().set_state(state, now);
            }
        }
    }
}

fn main() -> ExitCode {
    let Some(o) = parse(std::env::args().skip(1)) else {
        eprintln!(
            "usage: ultar-phone [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]"
        );
        return ExitCode::from(2);
    };
    match run(o) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("ultar-phone: {e}");
            ExitCode::FAILURE
        }
    }
}
