//! `ultar-web [ADDRESS] [--socket PATH] [--engine PROGRAM] [--profile DIR] [--downloads DIR]
//! [--filter FILE] [--no-cell] [--lang ar|en|de] [--light] [--private]`
//!
//! The Ultar Browser: connects to the compositor, starts the engine host in its cell and
//! shows a window. `--no-cell` (development only) starts the engine host unconfined.
//! Exit codes: 0 closed normally, 1 failed, 2 usage.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use ultar_i18n::Lang;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_web::app::{Browser, FilesApp, Paths};
use ultar_web::cell::CellSpec;
use ultar_web::engine::offline::OfflineEngine;
use ultar_web::engine::reader::fetch::Route;
use ultar_web::engine::reader::ReaderEngine;
use ultar_web::engine::remote::{HostConfig, RemoteEngine};
use ultar_web::engine::WebEngine;

/// Where the engine bundle is installed.
const BUNDLE: &str = "/usr/lib/ultar/web";

#[derive(Debug, Default)]
struct Args {
    url: Option<String>,
    socket: Option<PathBuf>,
    engine: Option<PathBuf>,
    profile: Option<PathBuf>,
    downloads: Option<PathBuf>,
    filter: Option<PathBuf>,
    no_cell: bool,
    lang: Option<Lang>,
    light: bool,
    private: bool,
}

fn parse() -> Option<Args> {
    let mut a = Args {
        // a link another app opened (the launcher hands it over); an argument wins
        url: std::env::var("ULTAR_OPEN_URI")
            .ok()
            .filter(|u| !u.is_empty()),
        ..Args::default()
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--socket" => a.socket = Some(it.next()?.into()),
            "--engine" => a.engine = Some(it.next()?.into()),
            "--profile" => a.profile = Some(it.next()?.into()),
            "--downloads" => a.downloads = Some(it.next()?.into()),
            "--filter" => a.filter = Some(it.next()?.into()),
            "--lang" => a.lang = Some(Lang::parse(&it.next()?)?),
            "--no-cell" => a.no_cell = true,
            "--light" => a.light = true,
            "--private" => a.private = true,
            s if s.starts_with("--") => return None,
            s => a.url = Some(s.to_string()),
        }
    }
    Some(a)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/data/ultar"), PathBuf::from)
}

fn fail(message: &dyn std::fmt::Display) -> ExitCode {
    let _ = writeln!(std::io::stderr(), "ultar-web: {message}");
    ExitCode::from(1)
}

/// Starts the engine host in its cell; everything it fetches goes through `proxy` (Tor).
fn start_engine(
    a: &Args,
    profile: &std::path::Path,
    downloads: &std::path::Path,
    proxy: String,
) -> Result<RemoteEngine, Box<dyn std::error::Error>> {
    let program = a
        .engine
        .clone()
        .unwrap_or_else(|| PathBuf::from(BUNDLE).join("libexec/ultar-web-engine"));
    let bundle = program
        .parent()
        .and_then(|p| p.parent())
        .map_or_else(|| PathBuf::from(BUNDLE), PathBuf::from);
    let cell = if a.no_cell {
        None
    } else {
        Some(CellSpec::page(&bundle, profile, downloads)?)
    };
    let mut agent = ultar_brand::release::Line::<160>::new();
    ultar_brand::release::write_user_agent(&mut agent, "UltarWeb", env!("CARGO_PKG_VERSION"))?;
    let config = HostConfig {
        program,
        cell,
        profile: profile.join("engine"),
        downloads: downloads.to_path_buf(),
        user_agent: agent.as_str().to_string(),
        filter: a
            .filter
            .clone()
            .or_else(|| Some(bundle.join("share/ultar-web/filter.json")).filter(|p| p.is_file())),
        proxy: Some(proxy),
    };
    std::fs::create_dir_all(&config.profile)?;
    Ok(RemoteEngine::start(&config)?)
}

fn start(a: Args) -> Result<(), Box<dyn std::error::Error>> {
    let lang = a.lang.unwrap_or_else(Lang::from_env);
    let profile = a
        .profile
        .clone()
        .unwrap_or_else(|| home().join(".local/share/ultar-web"));
    let downloads = a
        .downloads
        .clone()
        .unwrap_or_else(|| home().join("Downloads"));
    std::fs::create_dir_all(&profile)?;
    std::fs::create_dir_all(&downloads)?;
    // Pages go only through Tor (the persona of privacy::persona, the site never sees this
    // device's address); if Tor cannot start, no page goes out at all. The Tor client lives in
    // this process for the whole session, the engine reaches it only through its secret port.
    let tor = match ultar_tor::Tor::start(&profile.join("tor")) {
        Ok(t) => t,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "ultar-web: tor: {e}");
            return show(OfflineEngine::new(), a, lang, Paths { profile, downloads });
        }
    };
    match start_engine(&a, &profile, &downloads, tor.http_proxy_url()) {
        Ok(engine) => show(engine, a, lang, Paths { profile, downloads }),
        Err(e) => {
            // No full engine (bundle missing or refused to start, e.g. on the PC image): pages
            // are shown by the reader engine in pure Rust (text, links, Arabic); only if even
            // that cannot start, the window opens without pages (engine::offline).
            let _ = writeln!(
                std::io::stderr(),
                "ultar-web: full engine not available: {e}"
            );
            let route = Route::Tor(tor.proxy_url());
            match ReaderEngine::new(lang, &route) {
                Ok(reader) => show(reader, a, lang, Paths { profile, downloads }),
                Err(e) => {
                    let _ = writeln!(std::io::stderr(), "ultar-web: reader engine: {e}");
                    show(OfflineEngine::new(), a, lang, Paths { profile, downloads })
                }
            }
        }
    }
}

/// The browser window over `engine` until it is closed.
fn show<E: WebEngine>(
    engine: E,
    a: Args,
    lang: Lang,
    paths: Paths,
) -> Result<(), Box<dyn std::error::Error>> {
    let theme = if a.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let tr = ultar_web::catalog::translator(lang)?;
    let ui = ultar_ui::ui::Ui::new(
        PaintEnv::new(lang, theme),
        InputMode::Pointer,
        ultar_ui::Size::new(992.0, 584.0),
        tr,
    )?;
    let files = Box::new(FilesApp {
        program: PathBuf::from("/usr/bin/ultar-files"),
    });
    let mut browser = Browser::new(engine, ui, Some(paths), files)?;
    if a.private {
        browser.new_tab(true)?;
        browser.close_tab(0)?;
    }
    if let Some(url) = a.url {
        let field = browser.chrome().address;
        browser.open_typed(&url, field)?;
    }
    let socket = a
        .socket
        .unwrap_or_else(|| PathBuf::from(ultar_web::window::DEFAULT_SOCKET));
    let client = ultar_surface::client::Client::connect(&socket, ultar_web::window::APP_ID)?;
    ultar_web::window::run(&mut browser, client)?;
    Ok(())
}

fn main() -> ExitCode {
    let Some(args) = parse() else {
        let _ = writeln!(std::io::stderr(), "usage: ultar-web [ADDRESS] [--socket PATH] [--engine PROGRAM] [--profile DIR] [--downloads DIR] [--filter FILE] [--no-cell] [--lang ar|en|de] [--light] [--private]");
        return ExitCode::from(2);
    };
    match start(args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&e),
    }
}
