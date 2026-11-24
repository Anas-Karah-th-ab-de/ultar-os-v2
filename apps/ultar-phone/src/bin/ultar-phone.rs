//! `ultar-phone` – the Phone app as a compositor client.
//!
//! ```text
//! ultar-phone [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR` or the shipped socket, `--bus` to
//! `$ULTAR_BUS_SOCKET`. Without a reachable call service (no bus, no modem, no right
//! `call.*`) the window still opens: the dial pad shows and every request answers that the
//! call service is not reachable (the app's own error texts), nothing is faked.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
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
/// How long one request to the call service may take.
const CALL: Duration = Duration::from_secs(5);

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

/// The call service over the bus, or nothing when the bus cannot be reached.
enum Port {
    Bus(BusPort),
    Unreachable,
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
    let bus = o
        .bus
        .as_deref()
        .and_then(|p| Bus::connect(p, ClientOptions::named(APP_ID)).ok());
    let (port, state, launcher) = match bus {
        Some(b) => {
            let port = BusPort::new(b.clone(), CALL);
            let state = port.snapshot().unwrap_or_default();
            (Port::Bus(port), state, Some(b))
        }
        None => (Port::Unreachable, CallState::default(), None),
    };
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
    let app = PhoneApp::new(look, state, port, host.clone()).map_err(|e| e.to_string())?;
    let client = Client::connect(&o.compositor, APP_ID).map_err(|e| e.to_string())?;
    let title = app.ui_ref().translator().t("phone.title").into_owned();
    let mut window = PhoneWindow::open(client, app, host, &title).map_err(|e| e.to_string())?;
    if let Some(b) = launcher {
        window = window.with_launcher(b);
    }
    loop {
        match window
            .step(Duration::from_millis(250), wall_s())
            .map_err(|e| e.to_string())?
        {
            Step::Closed => return Ok(()),
            Step::Running | Step::Forward(_) => {}
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
