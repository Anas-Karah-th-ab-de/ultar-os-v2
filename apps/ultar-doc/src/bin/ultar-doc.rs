//! `ultar-doc` – the word processor as a compositor client.
//!
//! ```text
//! ultar-doc [--compositor PATH] [--bus PATH] [--areas DIR] [--lang ar|en|de] [--light] [URI]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--areas` to `$ULTAR_AREAS` or `/areas`,
//! `--bus` to `$ULTAR_BUS_SOCKET`, the URI to `$ULTAR_OPEN_URI` (a `files://...` document, with an
//! optional `#b<block>` place).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use ultar_doc::service::Mesh;
use ultar_doc::{App, Out};
use ultar_i18n::Lang;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::Request;
use ultar_surface::ui::UiWindow;
use ultar_ui::event::{Event, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards the compositor and the bus.
const APP_ID: &str = "org.ultar.doc";
/// Longest wait for an event.
const POLL: Duration = Duration::from_millis(250);

/// The text has the keyboard.
static TEXT_FOCUSED: AtomicBool = AtomicBool::new(true);

fn shortcut(event: &Event) -> bool {
    TEXT_FOCUSED.load(Ordering::Relaxed)
        && matches!(event, Event::Key { key, modifiers } if *key != Key::Tab || modifiers.ctrl)
}

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    areas: PathBuf,
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
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            a if !a.starts_with('-') => o.uri = Some(a.to_owned()),
            _ => return None,
        }
    }
    Some(o)
}

fn block_fragment(uri: &str) -> Option<usize> {
    uri.split('#').nth(1)?.strip_prefix('b')?.parse().ok()
}

fn open(o: &Options) -> Result<(UiWindow, App, Option<ultar_bus::Client>), String> {
    let socket = o.compositor.clone().ok_or("compositor")?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let env = PaintEnv::new(o.lang, theme);
    let tr = ultar_doc::i18n::translator;
    let ui = |lang: Lang| {
        Ui::new(env, InputMode::Pointer, Size::new(1.0, 1.0), tr(lang)).map_err(|e| e.to_string())
    };
    let title = tr(o.lang).t("app.doc.name").into_owned();
    let client = Client::connect(&socket, APP_ID).map_err(|e| e.to_string())?;
    let config = WindowConfig::toplevel(&title);
    let mut window = UiWindow::new(client, &config, ui(o.lang)?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let uri = o.uri.as_deref();
    let (mut doc, failed) = App::open(uri, &o.areas);
    // Jump to the place a handoff gave (`#b<block>`).
    if let Some(place) = uri.and_then(block_fragment) {
        doc.cursor.block = place.min(doc.document.blocks.len().saturating_sub(1));
        doc.clamp();
    }
    let tree = std::mem::replace(window.ui_mut(), ui(o.lang)?);
    let mut app = App::new(tree, doc, o.areas.clone(), uri.map(str::to_owned))?;
    if let Some(e) = failed {
        app.open_failed(e).map_err(|e| format!("{e:?}"))?;
    }
    std::mem::swap(window.ui_mut(), &mut app.ui);
    let bus = o
        .bus
        .as_ref()
        .map(|p| ultar_bus::Client::connect(p, ultar_bus::ClientOptions::named(APP_ID)).ok());
    Ok((window, app, bus.flatten()))
}

fn turn(
    window: &mut UiWindow,
    app: &mut App,
    size: &mut Size,
    mesh: &mut Option<Mesh>,
    started: Instant,
) -> Result<bool, String> {
    let actions = window.dispatch(Some(POLL)).map_err(|e| e.to_string())?;
    let closing = window.window().close_requested();
    let keys = window.take_unhandled_keys();
    let pointer = window.take_pointer_events();
    let pasted = window.take_pasted();
    std::mem::swap(window.ui_mut(), &mut app.ui);
    let result = (|| {
        if app.ui.size() != *size {
            *size = app.ui.size();
            app.rebuild()?;
        }
        // Pointer first: a click that closes a dialog must not also land on the text below.
        for event in pointer {
            app.pointer(event)?;
        }
        app.actions(actions)?;
        for key in keys {
            app.key(key)?;
        }
        if let Some(text) = pasted {
            app.pasted(&text)?;
        }
        if closing {
            app.close_requested()?;
        }
        Ok::<(), ultar_ui::ui::UiError>(())
    })();
    TEXT_FOCUSED.store(app.text_focused(), Ordering::Relaxed);
    app.tick(started.elapsed());
    let out = app.take_out();
    std::mem::swap(window.ui_mut(), &mut app.ui);
    result.map_err(|e| format!("{e:?}"))?;
    for o in out {
        if !follow(window, o)? {
            return Ok(false);
        }
    }
    if let Some(m) = mesh {
        let title = app.title();
        m.report(APP_ID, &title, app.mesh_uri().as_deref(), None);
    }
    if closing {
        window.keep_open();
    }
    window.draw().map_err(|e| e.to_string())?;
    Ok(true)
}

fn follow(window: &mut UiWindow, o: Out) -> Result<bool, String> {
    match o {
        Out::Close => return Ok(false),
        Out::Copy(text) => window.offer_text(text).map_err(|e| e.to_string())?,
        Out::Paste => window.request_paste().map_err(|e| e.to_string())?,
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

fn run(o: Options) -> Result<(), String> {
    let (mut window, mut app, bus) = open(&o)?;
    let mut mesh = bus.map(Mesh::new);
    let mut size = Size::new(0.0, 0.0);
    let started = Instant::now();
    while turn(&mut window, &mut app, &mut size, &mut mesh, started)? {}
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Some(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ultar-doc: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: ultar-doc [--compositor PATH] [--bus PATH] [--areas DIR] [--lang ar|en|de] [--light] [URI]");
            ExitCode::from(2)
        }
    }
}
