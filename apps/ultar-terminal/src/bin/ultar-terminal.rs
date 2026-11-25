//! `ultar-terminal` – the terminal as a compositor client.
//!
//! ```text
//! ultar-terminal [--compositor PATH] [--home DIR] [--lang ar|en|de] [--light]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`; the shell starts in `--home`, else
//! `/areas/home` (the user's home in the app's cell), else `$HOME`.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use ultar_i18n::Lang;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_terminal::app::Terminal;
use ultar_ui::event::{Event, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards the compositor (the desktop's name of the terminal).
const APP_ID: &str = "org.ultar.terminal";
/// How often a running program's output is looked at.
const POLL: Duration = Duration::from_millis(50);

/// A program runs: Ctrl+C goes to the terminal (stop), not to the field (copy).
static RUNNING: AtomicBool = AtomicBool::new(false);

fn shortcut(event: &Event) -> bool {
    match event {
        Event::Key { key, modifiers: m } => match key {
            Key::ArrowUp | Key::ArrowDown | Key::PageUp | Key::PageDown => !m.command(),
            Key::Char('l' | 'L') => m.ctrl,
            Key::Char('c' | 'C') => m.ctrl && RUNNING.load(Ordering::Relaxed),
            _ => false,
        },
        _ => false,
    }
}

struct Options {
    compositor: Option<PathBuf>,
    home: PathBuf,
    lang: Lang,
    light: bool,
}

fn default_home() -> PathBuf {
    let cell = PathBuf::from("/areas/home");
    if cell.is_dir() {
        return cell;
    }
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("/"), PathBuf::from)
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let mut o = Options {
        compositor: std::env::var_os("ULTAR_COMPOSITOR").map(PathBuf::from),
        home: default_home(),
        lang: Lang::from_env(),
        light: false,
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--home" => o.home = args.next()?.into(),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            // the launcher passes every app the apps' bus; these need none
            "--bus" => {
                args.next()?;
            }
            _ => return None,
        }
    }
    Some(o)
}

fn run(o: Options) -> Result<(), String> {
    let socket = o.compositor.ok_or("compositor")?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let env = PaintEnv::new(o.lang, theme);
    let ui = || {
        Ui::new(
            env,
            InputMode::Pointer,
            Size::new(1.0, 1.0),
            ultar_terminal::i18n::translator(o.lang),
        )
        .map_err(|e| e.to_string())
    };
    let title = ultar_terminal::i18n::translator(o.lang)
        .t("app.terminal.name")
        .into_owned();
    let client = Client::connect(&socket, APP_ID).map_err(|e| e.to_string())?;
    let mut window =
        UiWindow::new(client, &WindowConfig::toplevel(&title), ui()?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let tree = std::mem::replace(window.ui_mut(), ui()?);
    let mut term = Terminal::new(tree, o.home).map_err(|e| e.to_string())?;
    std::mem::swap(window.ui_mut(), &mut term.ui);
    let mut size = Size::new(0.0, 0.0);
    loop {
        let actions = window.dispatch(Some(POLL)).map_err(|e| e.to_string())?;
        if window.window().close_requested() {
            return Ok(());
        }
        let keys = window.take_unhandled_keys();
        let pointer = window.take_pointer_events();
        std::mem::swap(window.ui_mut(), &mut term.ui);
        let result = (|| {
            if term.ui.size() != size {
                size = term.ui.size();
                term.rebuild()?;
            }
            term.actions(actions)?;
            for e in pointer {
                term.pointer(e)?;
            }
            for k in keys {
                term.key(k)?;
            }
            term.poll()?;
            Ok::<(), ultar_ui::ui::UiError>(())
        })();
        RUNNING.store(term.busy(), Ordering::Relaxed);
        let closed = term.closed();
        std::mem::swap(window.ui_mut(), &mut term.ui);
        result.map_err(|e| format!("{e:?}"))?;
        if closed {
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
                eprintln!("ultar-terminal: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: ultar-terminal [--compositor PATH] [--home DIR] [--lang ar|en|de] [--light]");
            ExitCode::from(2)
        }
    }
}
