//! `ultar-remote` - the remote view window as a compositor client.
//!
//! ```text
//! ultar-remote [--compositor PATH] [--bus PATH] [--device ID] [--lang ar|en|de] [--light]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--bus` to `$ULTAR_BUS_SOCKET`. With
//! `--device` the session with that device starts at once.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_remote::app::{RemoteApp, Wish};
use ultar_remote::port::BusPort;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::{Request, WindowState};
use ultar_surface::ui::UiWindow;
use ultar_ui::event::{Action, Event, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::{Ui, UiError};
use ultar_ui::Size;

/// App id towards the compositor and the bus.
const APP_ID: &str = "org.ultar.remote";
/// How often the mesh is looked at while no event comes.
const POLL: Duration = Duration::from_millis(16);

/// F11 is the window's own key (full screen); everything else goes to the app.
fn shortcut(event: &Event) -> bool {
    matches!(
        event,
        Event::Key {
            key: Key::F(11),
            ..
        }
    )
}

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    device: Option<String>,
    lang: Lang,
    light: bool,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let mut o = Options {
        compositor: std::env::var_os("ULTAR_COMPOSITOR").map(PathBuf::from),
        bus: std::env::var_os("ULTAR_BUS_SOCKET").map(PathBuf::from),
        device: None,
        lang: Lang::from_env(),
        light: false,
    };
    let mut args = args;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--bus" => o.bus = Some(args.next()?.into()),
            "--device" => o.device = Some(args.next()?),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            _ => return None,
        }
    }
    Some(o)
}

/// Full screen on or off (the compositor does it).
fn set_fullscreen(window: &mut UiWindow, on: bool) -> Result<(), String> {
    let state = if on {
        WindowState::Fullscreen
    } else {
        WindowState::Normal
    };
    let surface = window.window().surface();
    window
        .client_mut()
        .send(&Request::SetState { surface, state })
        .map_err(|e| e.to_string())
}

/// The window and the app in it (their trees are swapped in and out around every turn).
fn open(o: &Options, bus: Bus) -> Result<(UiWindow, RemoteApp), String> {
    let socket = o.compositor.clone().ok_or("compositor")?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let env = PaintEnv::new(o.lang, theme);
    let tr = ultar_remote::i18n::translator;
    let ui = || {
        Ui::new(env, InputMode::Pointer, Size::new(1.0, 1.0), tr(o.lang)).map_err(|e| e.to_string())
    };
    let title = tr(o.lang).t("app.remote.name").into_owned();
    let client = Client::connect(&socket, APP_ID).map_err(|e| e.to_string())?;
    let mut window =
        UiWindow::new(client, &WindowConfig::toplevel(&title), ui()?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let tree = std::mem::replace(window.ui_mut(), ui()?);
    let mut app =
        RemoteApp::new(tree, Box::new(BusPort::new(bus))).map_err(|e| format!("{e:?}"))?;
    std::mem::swap(window.ui_mut(), &mut app.ui);
    Ok((window, app))
}

/// What one turn of the loop brings to the app.
struct Turn {
    actions: Vec<Action>,
    keys: Vec<Event>,
    pointer: Vec<Event>,
    now: Instant,
}

/// One turn inside the app: size change, mesh news, the wanted device, the window's events.
fn turn(
    app: &mut RemoteApp,
    size: &mut Size,
    device: &mut Option<String>,
    t: Turn,
) -> Result<(), UiError> {
    if app.ui.size() != *size {
        *size = app.ui.size();
        app.rebuild()?;
    }
    app.poll(t.now)?;
    if let Some(id) = device.take_if(|_| !app.candidates().is_empty()) {
        let at = (app.candidates().iter()).position(|p| p.id == id || p.name == id);
        if let Some(at) = at {
            app.connect(at, t.now)?;
        }
    }
    app.actions(t.actions, t.now)?;
    for e in t.pointer {
        app.pointer(e)?;
    }
    for k in t.keys {
        app.key(k)?;
    }
    Ok(())
}

fn run(o: Options) -> Result<(), String> {
    let bus = Bus::connect(&o.bus.clone().ok_or("bus")?, ClientOptions::named(APP_ID))
        .map_err(|e| e.to_string())?;
    let (mut window, mut app) = open(&o, bus)?;
    let mut size = Size::new(0.0, 0.0);
    let mut device = o.device;
    loop {
        let actions = window.dispatch(Some(POLL)).map_err(|e| e.to_string())?;
        std::mem::swap(window.ui_mut(), &mut app.ui);
        if window.window().close_requested() {
            // closing the window ends a running session
            let _ = app.stop();
            return Ok(());
        }
        let t = Turn {
            actions,
            keys: window.take_unhandled_keys(),
            pointer: window.take_pointer_events(),
            now: Instant::now(),
        };
        let result = turn(&mut app, &mut size, &mut device, t);
        let wish = app.take_wish();
        std::mem::swap(window.ui_mut(), &mut app.ui);
        result.map_err(|e| format!("{e:?}"))?;
        if let Some(Wish::Fullscreen(on)) = wish {
            set_fullscreen(&mut window, on)?;
        }
        window.draw().map_err(|e| e.to_string())?;
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Some(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ultar-remote: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!(
                "usage: ultar-remote [--compositor PATH] [--bus PATH] [--device ID] [--lang ar|en|de] [--light]"
            );
            ExitCode::from(2)
        }
    }
}
