//! `ultar-appbuilder` – the App-Builder window (APP-RULES §2: start, connect, loop; the logic
//! lives in the library).
//!
//! Environment: `ULTAR_COMPOSITOR_SOCKET` (default `/run/ultar/ui/compositor`),
//! `ULTAR_BUS_SOCKET` (launcher; unset → installing answers `no-launcher`), `ULTAR_APP_DATA`
//! (the app's data area for the developer key; default `/data`, where the cell binds it).
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;
use ultar_appbuilder::state::{self, Command, Event, State, Target};
use ultar_appbuilder::{i18n, view, AppError};
use ultar_i18n::Lang;
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_ui::event::ActionKind;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

const APP_ID: &str = "km-code-craft.appbuilder";
const COMPOSITOR_DEFAULT: &str = "/run/ultar/ui/compositor";
const POLL: Duration = Duration::from_millis(100);

fn worker() -> (Sender<Command>, Receiver<Event>) {
    let (cmd_tx, cmd_rx) = mpsc::channel::<Command>();
    let (ev_tx, ev_rx) = mpsc::channel::<Event>();
    std::thread::spawn(move || {
        let data = std::env::var_os("ULTAR_APP_DATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/data"));
        let bus = std::env::var_os("ULTAR_BUS_SOCKET").and_then(|p| {
            ultar_bus::Client::connect(Path::new(&p), ultar_bus::ClientOptions::named(APP_ID)).ok()
        });
        let install = |p: &Path| match &bus {
            Some(b) => ultar_appbuilder::package::install(b, p),
            None => Err(AppError::NoLauncher),
        };
        for cmd in cmd_rx {
            if ev_tx.send(state::run(cmd, &data, &install)).is_err() {
                return;
            }
        }
    });
    (cmd_tx, ev_rx)
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let lang = Lang::from_env();
    let tr = i18n::translator(lang);
    let title = tr.t("app.appbuilder.name").into_owned();
    let env = PaintEnv::new(lang, ThemeKind::Dark);
    let ui = Ui::new(env, InputMode::Pointer, Size::new(960.0, 640.0), tr)?;
    let socket =
        std::env::var("ULTAR_COMPOSITOR_SOCKET").unwrap_or_else(|_| COMPOSITOR_DEFAULT.to_string());
    let client = Client::connect(Path::new(&socket), APP_ID)?;
    let mut win = UiWindow::new(client, &WindowConfig::toplevel(&title), ui)?;
    let (cmd_tx, ev_rx) = worker();
    let mut s = State::default();
    let mut built = view::build(win.ui_mut(), &s)?;
    loop {
        let actions = win.dispatch(Some(POLL))?;
        if win.window().close_requested() {
            return Ok(());
        }
        let mut dirty = false;
        for a in actions {
            match (built.targets.get(&a.node).copied(), &a.kind) {
                (Some(Target::Project), ActionKind::TextChanged) => {
                    let text = win.ui_mut().text(a.node).unwrap_or_default().to_owned();
                    s.set_project(&text);
                }
                (Some(Target::Project), ActionKind::Submitted) => {
                    dirty |= send(&mut s, Target::Check, &cmd_tx);
                }
                (Some(t), ActionKind::Activated) => dirty |= send(&mut s, t, &cmd_tx),
                _ => {}
            }
        }
        while let Ok(ev) = ev_rx.try_recv() {
            s.event(ev);
            dirty = true;
        }
        if dirty {
            built = view::build(win.ui_mut(), &s)?;
        }
        win.draw()?;
    }
}

fn send(s: &mut State, target: Target, tx: &Sender<Command>) -> bool {
    if let Some(cmd) = s.act(target) {
        let _ = tx.send(cmd);
    }
    true
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
