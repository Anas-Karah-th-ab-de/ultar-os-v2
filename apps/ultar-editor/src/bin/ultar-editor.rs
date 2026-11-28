//! `ultar-editor` – the text editor as a compositor client.
//!
//! ```text
//! ultar-editor [--compositor PATH] [--areas DIR] [--lang ar|en|de] [--light] [URI]
//! ```
//! `--compositor` defaults to `$ULTAR_COMPOSITOR`, `--areas` to `$ULTAR_AREAS` or `/areas`
//! (where the cell binds the user's places), the URI (`files://documents/a.txt`) to
//! `$ULTAR_OPEN_URI` (set by the launcher when Files opens a text file).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use ultar_editor::app::{Editor, Out};
use ultar_i18n::Lang;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::proto::Request;
use ultar_surface::ui::UiWindow;
use ultar_ui::event::{Event, Key};
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

/// App id towards the compositor (the desktop's name of the editor).
const APP_ID: &str = "org.ultar.editor";
/// Longest wait for an event.
const POLL: Duration = Duration::from_millis(250);

/// The text has the keyboard: every key but Tab goes to the editor before the widgets
/// (they would move the focus with the arrows).
static TEXT_FOCUSED: AtomicBool = AtomicBool::new(true);

fn shortcut(event: &Event) -> bool {
    TEXT_FOCUSED.load(Ordering::Relaxed)
        && matches!(event, Event::Key { key, modifiers } if *key != Key::Tab || modifiers.ctrl)
}

struct Options {
    compositor: Option<PathBuf>,
    areas: PathBuf,
    lang: Lang,
    light: bool,
    uri: Option<String>,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        areas: env("ULTAR_AREAS").unwrap_or_else(|| PathBuf::from("/areas")),
        lang: Lang::from_env(),
        light: false,
        uri: std::env::var("ULTAR_OPEN_URI").ok(),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--areas" => o.areas = args.next()?.into(),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            // the launcher passes every app the apps' bus; these need none
            "--bus" => {
                args.next()?;
            }
            a if !a.starts_with('-') => o.uri = Some(a.to_owned()),
            _ => return None,
        }
    }
    Some(o)
}

/// The window and the editor in it.
fn open(o: Options) -> Result<(UiWindow, Editor), String> {
    let socket = o.compositor.ok_or("compositor")?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    // the person's theme, accent and text size, handed over by the app launcher
    let env = PaintEnv::new(o.lang, theme).with_session(o.light);
    let tr = ultar_editor::i18n::translator;
    let ui = |lang| {
        Ui::new(env, InputMode::Pointer, Size::new(1.0, 1.0), tr(lang)).map_err(|e| e.to_string())
    };
    let title = tr(o.lang).t("app.editor.name").into_owned();
    let client = Client::connect(&socket, APP_ID).map_err(|e| e.to_string())?;
    let config = WindowConfig::toplevel(&title);
    let mut window = UiWindow::new(client, &config, ui(o.lang)?).map_err(|e| e.to_string())?;
    window.set_shortcuts(Some(shortcut));
    let (doc, failed) = Editor::open(o.uri.as_deref(), &o.areas);
    let tree = std::mem::replace(window.ui_mut(), ui(o.lang)?);
    let mut editor = Editor::new(tree, doc, o.areas).map_err(|e| e.to_string())?;
    if let Some(e) = failed {
        editor.open_failed(e).map_err(|e| format!("{e:?}"))?;
    }
    std::mem::swap(window.ui_mut(), &mut editor.ui);
    Ok((window, editor))
}

/// One event and what follows from it; `false` when the window closes.
fn turn(window: &mut UiWindow, editor: &mut Editor, size: &mut Size) -> Result<bool, String> {
    let actions = window.dispatch(Some(POLL)).map_err(|e| e.to_string())?;
    let closing = window.window().close_requested();
    let keys = window.take_unhandled_keys();
    let pointer = window.take_pointer_events();
    let pasted = window.take_pasted();
    std::mem::swap(window.ui_mut(), &mut editor.ui);
    let result = (|| {
        if editor.ui.size() != *size {
            *size = editor.ui.size();
            editor.rebuild()?;
        }
        editor.actions(actions)?;
        for event in pointer {
            editor.pointer(event)?;
        }
        for key in keys {
            editor.key(key)?;
        }
        if let Some(text) = pasted {
            editor.pasted(&text)?;
        }
        if closing {
            editor.close_requested()?;
        }
        Ok::<(), ultar_ui::ui::UiError>(())
    })();
    TEXT_FOCUSED.store(editor.text_focused(), Ordering::Relaxed);
    let out = editor.take_out();
    std::mem::swap(window.ui_mut(), &mut editor.ui);
    result.map_err(|e| format!("{e:?}"))?;
    for o in out {
        if !follow(window, o)? {
            return Ok(false);
        }
    }
    if closing {
        window.keep_open();
    }
    window.draw().map_err(|e| e.to_string())?;
    Ok(true)
}

/// Does what the editor asked of the window; `false` for closing.
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
    let (mut window, mut editor) = open(o)?;
    let mut size = Size::new(0.0, 0.0);
    while turn(&mut window, &mut editor, &mut size)? {}
    Ok(())
}

fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Some(o) => match run(o) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("ultar-editor: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            eprintln!("usage: ultar-editor [--compositor PATH] [--areas DIR] [--lang ar|en|de] [--light] [URI]");
            ExitCode::from(2)
        }
    }
}
