//! `ultar-pdf` – the PDF reader as a compositor client.
//!
//! ```text
//! ultar-pdf [--compositor PATH] [--bus PATH] [--areas DIR] [--data DIR]
//!           [--lang ar|en|de] [--light] [URI]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--bus` to `$ULTAR_BUS_SOCKET`, `--areas` to
//! `$ULTAR_AREAS` or `/areas`, `--data` to `$ULTAR_APP_DATA` or `/data` (the recents list), the
//! URI (`files://documents/a.pdf[#page=N]`) to `$ULTAR_OPEN_URI` (set by the launcher when Files
//! opens a PDF).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_pdf::app::{App, Out};
use ultar_pdf::keys::{self, Context};
use ultar_pdf::service;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::Request;
use ultar_surface::ui::UiWindow;
use ultar_ui::event::Event;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// How long to wait for an event while the worker is busy, and while nothing is going on
/// (the window is driven by events; the longer wait only bounds the idle wake-ups).
const POLL_BUSY: Duration = Duration::from_millis(30);
const POLL_IDLE: Duration = Duration::from_millis(1000);

/// What the keyboard is doing, for the window's shortcut filter (set every turn).
static SEARCHING: AtomicU8 = AtomicU8::new(0);
static TYPING: AtomicU8 = AtomicU8::new(0);

fn shortcut(event: &Event) -> bool {
    let cx = Context {
        searching: SEARCHING.load(Ordering::Relaxed) != 0,
        typing: TYPING.load(Ordering::Relaxed) != 0,
    };
    matches!(event, Event::Key { key, modifiers } if keys::is_shortcut(*key, *modifiers, cx))
}

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    areas: PathBuf,
    data: PathBuf,
    lang: Lang,
    light: bool,
    uri: Option<String>,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        bus: env("ULTAR_BUS_SOCKET"),
        areas: env("ULTAR_AREAS").unwrap_or_else(|| PathBuf::from("/areas")),
        data: env("ULTAR_APP_DATA").unwrap_or_else(|| PathBuf::from("/data")),
        lang: Lang::from_env(),
        light: false,
        uri: std::env::var("ULTAR_OPEN_URI").ok(),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--bus" => o.bus = Some(args.next()?.into()),
            "--areas" => o.areas = args.next()?.into(),
            "--data" => o.data = args.next()?.into(),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            a if !a.starts_with('-') => o.uri = Some(a.to_owned()),
            _ => return None,
        }
    }
    Some(o)
}

/// The window and the reader in it; the bus is optional (without it the mesh is not told).
fn open(o: &Options) -> Result<(UiWindow, App, Option<Bus>), String> {
    let socket = o.compositor.as_deref().ok_or("compositor")?;
    let bus = o
        .bus
        .as_deref()
        .and_then(|b| Bus::connect(b, ClientOptions::named(service::APP_ID)).ok());
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let env = PaintEnv::new(o.lang, theme);
    let tr = ultar_pdf::i18n::translator;
    let ui = |lang| {
        Ui::new(env, InputMode::Pointer, Size::new(1.0, 1.0), tr(lang)).map_err(|e| e.to_string())
    };
    let title = tr(o.lang).t("app.pdf.name").into_owned();
    let client = Client::connect(socket, service::APP_ID).map_err(|e| e.to_string())?;
    let config = WindowConfig::toplevel(&title);
    let mut window = UiWindow::new(client, &config, ui(o.lang)?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let doc = App::open(o.uri.as_deref(), &o.areas);
    let tree = std::mem::replace(window.ui_mut(), ui(o.lang)?);
    let app = App::new(tree, doc, o.areas.clone(), o.data.clone()).map_err(|e| format!("{e:?}"))?;
    let mut app = app;
    std::mem::swap(window.ui_mut(), &mut app.ui);
    Ok((window, app, bus))
}

/// Does what the reader asked of the window; `false` for closing.
fn follow(window: &mut UiWindow, o: Out) -> Result<bool, String> {
    match o {
        Out::Close => return Ok(false),
        Out::Copy(text) => window.offer_text(text).map_err(|e| e.to_string())?,
        Out::Title(title) => {
            let surface = window.window().surface();
            window
                .client_mut()
                .send(&Request::SetTitle { surface, title })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

/// One turn: wait for an event, hand it to the reader, do what it asks.
fn turn(window: &mut UiWindow, app: &mut App, size: &mut Size) -> Result<bool, String> {
    let poll = if app.busy() { POLL_BUSY } else { POLL_IDLE };
    let actions = window.dispatch(Some(poll)).map_err(|e| e.to_string())?;
    let closing = window.window().close_requested();
    let keys = window.take_unhandled_keys();
    let pointer = window.take_pointer_events();
    let touch = window.take_touch_events();
    std::mem::swap(window.ui_mut(), &mut app.ui);
    let result = (|| {
        if app.ui.size() != *size {
            *size = app.ui.size();
            app.rebuild()?;
        }
        app.actions(actions)?;
        for event in pointer {
            app.pointer(event)?;
        }
        for event in touch {
            app.touch(event)?;
        }
        for event in keys {
            if let Event::Key { key, modifiers } = event {
                app.key(key, modifiers)?;
            }
        }
        if closing {
            app.close_requested();
        }
        app.poll()
    })();
    let cx = app.key_context();
    SEARCHING.store(u8::from(cx.searching), Ordering::Relaxed);
    TYPING.store(u8::from(cx.typing), Ordering::Relaxed);
    let out = app.take_out();
    std::mem::swap(window.ui_mut(), &mut app.ui);
    result.map_err(|e| format!("{e:?}"))?;
    for o in out {
        if !follow(window, o)? {
            return Ok(false);
        }
    }
    if closing {
        window.keep_open();
    }
    Ok(true)
}

fn run(o: Options) -> Result<(), String> {
    let (mut window, mut app, bus) = open(&o)?;
    let mut reported = None;
    let mut size = Size::new(0.0, 0.0);
    loop {
        if let (Some(bus), Some(uri)) = (&bus, app.activity_uri()) {
            service::report(bus, &uri, &app.title(), &mut reported);
        }
        if !turn(&mut window, &mut app, &mut size)? {
            return Ok(());
        }
        window.draw().map_err(|e| e.to_string())?;
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Some(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ultar-pdf: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: ultar-pdf [--compositor PATH] [--bus PATH] [--areas DIR] [--data DIR] [--lang ar|en|de] [--light] [URI]");
            ExitCode::from(2)
        }
    }
}
