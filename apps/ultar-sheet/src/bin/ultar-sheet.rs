//! `ultar-sheet` – the spreadsheet as a compositor client.
//!
//! ```text
//! ultar-sheet [--compositor PATH] [--bus PATH] [--areas DIR] [--recover DIR]
//!             [--lang ar|en|de] [--light] [--touch] [URI]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--bus` to `$ULTAR_BUS_SOCKET` (the mesh
//! is told which workbook, sheet and cell is open), `--areas` to `$ULTAR_AREAS` or `/areas`
//! (where the cell binds the user's places), `--recover` to `$ULTAR_SHEET_RECOVER` or a
//! folder under the user's data, the URI (`files://documents/a.xlsx#sheet=Sheet1&cell=B2`)
//! to `$ULTAR_OPEN_URI` (set by the launcher when Files opens a workbook).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::{Duration, Instant};
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_sheet::app::{Out, Report, SheetApp, State};
use ultar_sheet::book::Book;
use ultar_sheet::file::{self, DocError, Opened};
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::Request;
use ultar_surface::ui::UiWindow;
use ultar_ui::event::{Event, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards compositor and bus (the desktop's name of the sheet).
const APP_ID: &str = "org.ultar.sheet";
/// Longest wait for an event.
const POLL: Duration = Duration::from_millis(250);
/// Wait while a file is being read (the progress picture moves).
const POLL_LOADING: Duration = Duration::from_millis(40);

/// The grid has the keyboard: every key but Ctrl+Tab goes to the app before the widgets
/// (they would move the focus with the arrows and Tab).
static GRID_FOCUSED: AtomicBool = AtomicBool::new(true);

fn shortcut(event: &Event) -> bool {
    GRID_FOCUSED.load(Ordering::Relaxed)
        && matches!(event, Event::Key { key, modifiers } if *key != Key::Tab || !modifiers.ctrl)
}

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    areas: PathBuf,
    recover: PathBuf,
    lang: Lang,
    light: bool,
    touch: bool,
    uri: Option<String>,
}

fn default_recover() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("ultar-sheet").join("recover")
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        bus: env("ULTAR_BUS_SOCKET"),
        areas: env("ULTAR_AREAS").unwrap_or_else(|| PathBuf::from("/areas")),
        recover: env("ULTAR_SHEET_RECOVER").unwrap_or_else(default_recover),
        lang: Lang::from_env(),
        light: false,
        touch: false,
        uri: std::env::var("ULTAR_OPEN_URI").ok(),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--bus" => o.bus = Some(args.next()?.into()),
            "--areas" => o.areas = args.next()?.into(),
            "--recover" => o.recover = args.next()?.into(),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            "--touch" => o.touch = true,
            a if !a.starts_with('-') => o.uri = Some(a.to_owned()),
            _ => return None,
        }
    }
    Some(o)
}

type Loaded = (PathBuf, Result<Opened, DocError>);

/// The window, the app in it and the channel the file reader answers on.
struct Session {
    window: UiWindow,
    app: SheetApp,
    loaded: Receiver<Loaded>,
    reader: Sender<Loaded>,
    bus: Option<Bus>,
    reported: Option<Option<Report>>,
}

fn open(o: &Options) -> Result<Session, String> {
    let socket = o.compositor.clone().ok_or("compositor")?;
    let theme = if o.light { ThemeKind::Light } else { ThemeKind::Dark };
    let mode = if o.touch { InputMode::Touch } else { InputMode::Pointer };
    let env = PaintEnv::new(o.lang, theme);
    let tr = ultar_sheet::i18n::translator;
    let ui = |lang| Ui::new(env, mode, Size::new(1.0, 1.0), tr(lang)).map_err(|e| e.to_string());
    let title = tr(o.lang).t("app.sheet.name").into_owned();
    let client = Client::connect(&socket, APP_ID).map_err(|e| e.to_string())?;
    let config = WindowConfig::toplevel(&title);
    let mut window = UiWindow::new(client, &config, ui(o.lang)?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let tree = std::mem::replace(window.ui_mut(), ui(o.lang)?);
    let mut app = SheetApp::new(tree, Book::new(), o.areas.clone(), o.recover.clone())?;
    let (reader, loaded) = channel();
    let bus = o
        .bus
        .as_ref()
        .and_then(|p| Bus::connect(p, ClientOptions::named(APP_ID)).ok());
    match o.uri.as_deref() {
        Some(uri) => app.open_uri(uri).map_err(|e| format!("{e:?}"))?,
        None => app.offer_recovery_new().map_err(|e| format!("{e:?}"))?,
    }
    std::mem::swap(window.ui_mut(), &mut app.ui);
    Ok(Session { window, app, loaded, reader, bus, reported: None })
}

/// Reads a workbook in a thread of its own, so the window stays alive.
fn load(reader: Sender<Loaded>, path: PathBuf) {
    std::thread::spawn(move || {
        let result = file::open(&path);
        let _ = reader.send((path, result));
    });
}

/// Tells the device mesh which workbook, sheet and cell is open (only when it changed).
fn report(s: &mut Session) {
    let Some(bus) = &s.bus else {
        return;
    };
    use ultar_bus::messages::mesh::{Activity, SetActivity};
    let now = s.app.report();
    let shown = now.uri.is_some().then(|| now.clone());
    if s.reported.as_ref() == Some(&shown) {
        return;
    }
    let activity = shown.as_ref().map(|r| Activity {
        app: APP_ID.to_owned(),
        title: r.name.clone(),
        uri: r.uri.clone(),
        file: s.app.file_on_this_device().map(|p| p.to_string_lossy().into_owned()),
    });
    let _ = bus.call(&SetActivity { activity }, Duration::from_millis(300));
    s.reported = Some(shown);
}

/// One event and what follows from it; `false` when the window closes.
fn turn(s: &mut Session, size: &mut Size) -> Result<bool, String> {
    let wait = if matches!(s.app.state(), State::Loading(_)) { POLL_LOADING } else { POLL };
    let actions = s.window.dispatch(Some(wait)).map_err(|e| e.to_string())?;
    let closing = s.window.window().close_requested();
    let keys = s.window.take_unhandled_keys();
    let pointer = s.window.take_pointer_events();
    let pasted = s.window.take_pasted();
    std::mem::swap(s.window.ui_mut(), &mut s.app.ui);
    let app = &mut s.app;
    let result = (|| {
        if app.ui.size() != *size {
            *size = app.ui.size();
            app.rebuild()?;
        }
        while let Ok((path, result)) = s.loaded.try_recv() {
            app.loaded(path, result)?;
        }
        app.actions(actions)?;
        for event in pointer {
            app.pointer(event)?;
        }
        for key in keys {
            app.key(key)?;
        }
        if let Some(text) = pasted {
            app.pasted(&text)?;
        }
        if closing {
            app.close_requested()?;
        }
        app.tick(Instant::now());
        Ok::<(), ultar_ui::ui::UiError>(())
    })();
    GRID_FOCUSED.store(app.grid_focused(), Ordering::Relaxed);
    let out = app.take_out();
    std::mem::swap(s.window.ui_mut(), &mut s.app.ui);
    result.map_err(|e| format!("{e:?}"))?;
    for o in out {
        if !follow(s, o)? {
            return Ok(false);
        }
    }
    if closing {
        s.window.keep_open();
    }
    report(s);
    s.window.draw().map_err(|e| e.to_string())?;
    Ok(true)
}

/// Does what the app asked of the window; `false` for closing.
fn follow(s: &mut Session, o: Out) -> Result<bool, String> {
    match o {
        Out::Close => return Ok(false),
        Out::Copy(text) => s.window.offer_text(text).map_err(|e| e.to_string())?,
        Out::Paste => s.window.request_paste().map_err(|e| e.to_string())?,
        Out::Load(path) => load(s.reader.clone(), path),
        Out::Title(title) => {
            let surface = s.window.window().surface();
            s.window
                .client_mut()
                .send(&Request::SetTitle { surface, title })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(true)
}

fn run(o: Options) -> Result<(), String> {
    let mut s = open(&o)?;
    // the outs of the start (title, a file to read)
    std::mem::swap(s.window.ui_mut(), &mut s.app.ui);
    let first = s.app.take_out();
    std::mem::swap(s.window.ui_mut(), &mut s.app.ui);
    for out in first {
        if !follow(&mut s, out)? {
            return Ok(());
        }
    }
    let mut size = Size::new(0.0, 0.0);
    while turn(&mut s, &mut size)? {}
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Some(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ultar-sheet: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!(
                "usage: ultar-sheet [--compositor PATH] [--bus PATH] [--areas DIR] [--recover DIR] [--lang ar|en|de] [--light] [--touch] [URI]"
            );
            ExitCode::from(2)
        }
    }
}
