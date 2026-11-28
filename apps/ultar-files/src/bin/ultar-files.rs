//! `ultar-files` – the Files app as a compositor client.
//!
//! ```text
//! ultar-files [--compositor PATH] [--bus PATH] [--areas DIR] [--lang ar|en|de] [--light]
//!             [--desktop] [URI]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--bus` to `$ULTAR_BUS_SOCKET`, `--areas` to
//! `$ULTAR_AREAS` or `/areas` (where the cell binds the granted areas), the URI to
//! `$ULTAR_OPEN_URI` (set by the launcher). `--desktop` shows the desktop icons on the
//! desktop surface (needs the compositor right of the desktop surface).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_files::fs::Fs;
use ultar_files::service::bus::{BusLauncher, BusVolumes};
use ultar_files::{App, Mode, Ports};
use ultar_i18n::Lang;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::Role;
use ultar_surface::ui::UiWindow;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards compositor and bus.
const APP_ID: &str = "org.ultar.files";
/// How often background work and drives are looked at while idle.
const POLL: Duration = Duration::from_millis(100);

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    areas: PathBuf,
    lang: Lang,
    light: bool,
    desktop: bool,
    uri: Option<String>,
}

/// Wrong command line.
#[derive(Debug)]
struct Usage;

fn parse(args: impl Iterator<Item = String>) -> Result<Options, Usage> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        bus: env("ULTAR_BUS_SOCKET"),
        areas: env("ULTAR_AREAS").unwrap_or_else(|| PathBuf::from("/areas")),
        lang: Lang::from_env(),
        light: false,
        desktop: false,
        uri: std::env::var("ULTAR_OPEN_URI").ok(),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(Usage);
        match arg.as_str() {
            "--compositor" => o.compositor = Some(value()?.into()),
            "--bus" => o.bus = Some(value()?.into()),
            "--areas" => o.areas = value()?.into(),
            "--lang" => o.lang = Lang::parse(&value()?).ok_or(Usage)?,
            "--light" => o.light = true,
            "--desktop" => o.desktop = true,
            a if !a.starts_with('-') => o.uri = Some(a.to_owned()),
            _ => return Err(Usage),
        }
    }
    Ok(o)
}

/// The compositor window (or the desktop surface) for `ui`.
fn open_window(
    socket: &std::path::Path,
    desktop: bool,
    title: &str,
    ui: Ui,
) -> Result<UiWindow, String> {
    let config = if desktop {
        WindowConfig {
            min_size: (0, 0),
            ..WindowConfig::with_role(Role::Background, "", false)
        }
    } else {
        WindowConfig::toplevel(title)
    };
    let client = Client::connect(socket, APP_ID).map_err(|e| e.to_string())?;
    let mut window = UiWindow::new(client, &config, ui).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(ultar_files::update::is_shortcut));
    Ok(window)
}

/// Tells the device mesh which folder is open, so it can be continued on another device
/// ("continue on another device"); said again only when the folder changed. Without the
/// mesh nothing happens.
fn report_folder(bus: &Bus, app: &App, title: &str, reported: &mut Option<String>) {
    use ultar_bus::messages::mesh::{Activity, SetActivity};
    let uri = ultar_files::uri::of_loc(&app.state.folder);
    if reported.as_ref() == Some(&uri) {
        return;
    }
    let activity = Activity {
        app: APP_ID.to_owned(),
        title: title.to_owned(),
        uri: Some(uri.clone()),
        file: None,
    };
    let set = SetActivity {
        activity: Some(activity),
    };
    let _ = bus.call(&set, std::time::Duration::from_millis(300));
    *reported = Some(uri);
}

fn run(o: Options) -> Result<(), String> {
    let socket = o.compositor.ok_or("compositor")?;
    let bus = Bus::connect(&o.bus.ok_or("bus")?, ClientOptions::named(APP_ID))
        .map_err(|e| e.to_string())?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    // the person's theme, accent and text size, handed over by the app launcher
    let env = PaintEnv::new(o.lang, theme).with_session(o.light);
    let tr = ultar_files::i18n::translator(o.lang);
    let title = tr.t("app.files.name").into_owned();
    let ui =
        Ui::new(env, InputMode::Pointer, Size::new(1.0, 1.0), tr).map_err(|e| e.to_string())?;
    let mut window = open_window(&socket, o.desktop, &title, ui)?;
    let mesh = bus.clone();
    let ports = Ports {
        launcher: Box::new(BusLauncher(bus.clone())),
        volumes: Box::new(BusVolumes::new(bus)),
    };
    let mode = if o.desktop { Mode::Desk } else { Mode::Window };
    let placeholder = Ui::new(
        env,
        InputMode::Pointer,
        Size::new(1.0, 1.0),
        ultar_files::i18n::translator(o.lang),
    )
    .map_err(|e| e.to_string())?;
    let ui = std::mem::replace(window.ui_mut(), placeholder);
    let fs = Fs::open(&o.areas);
    let mut app = App::new(ui, fs, Some(o.areas), ports, mode, o.uri.as_deref())
        .map_err(|e| e.to_string())?;
    std::mem::swap(window.ui_mut(), &mut app.ui);
    let mut reported = None;
    loop {
        if !o.desktop {
            report_folder(&mesh, &app, &title, &mut reported);
        }
        let size = window.ui_mut().size();
        let actions = window.dispatch(Some(POLL)).map_err(|e| e.to_string())?;
        if window.window().close_requested() {
            return Ok(());
        }
        let keys = window.take_unhandled_keys();
        let key_input = window.last_input_was_key();
        std::mem::swap(window.ui_mut(), &mut app.ui);
        let result = (|| {
            if app.ui.size() != size {
                app.rebuild()?;
            }
            app.actions(actions, key_input)?;
            app.keys(keys)?;
            app.poll()
        })();
        std::mem::swap(window.ui_mut(), &mut app.ui);
        result.map_err(|e| e.to_string())?;
        window.draw().map_err(|e| e.to_string())?;
    }
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Err(Usage) => ExitCode::from(2),
    }
}
