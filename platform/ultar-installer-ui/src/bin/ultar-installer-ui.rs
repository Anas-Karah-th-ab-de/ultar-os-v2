//! `ultar-installer-ui` – the graphical installer as a compositor client.
//!
//! ```text
//! ultar-installer-ui --source PATH --disk PATH=NAME [--disk …] [--plans DIR]
//!                    [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light]
//! ```
//! The live session's launcher passes the installation medium and the disks it offers (from
//! the device HAL). Exit status: 0 installed (restart), 3 cancelled (nothing written),
//! 1 failure, 2 usage.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;
use ultar_bus::{Client as Bus, ClientOptions};
use ultar_i18n::Lang;
use ultar_installer_ui::engine::{inspect, write};
use ultar_installer_ui::ports::{BusDevices, FolderStore};
use ultar_installer_ui::wizard::Wizard;
use ultar_installer_ui::{Exit, InstallerApp};
use ultar_surface::client::{Client, WindowConfig};
use ultar_surface::ui::UiWindow;
use ultar_ui::paint::PaintEnv;
use ultar_ui::tokens::{InputMode, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::Size;

const APP_ID: &str = "system.installer";
const TICK: Duration = Duration::from_millis(100);

struct Options {
    compositor: Option<PathBuf>,
    bus: Option<PathBuf>,
    lang: Lang,
    light: bool,
    source: Option<PathBuf>,
    disks: Vec<(PathBuf, String)>,
    plans: PathBuf,
}

fn parse(args: impl Iterator<Item = String>) -> Option<Options> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut o = Options {
        compositor: env("ULTAR_COMPOSITOR"),
        bus: env("ULTAR_BUS_SOCKET"),
        lang: Lang::from_env(),
        light: false,
        source: None,
        disks: Vec::new(),
        plans: PathBuf::from("."),
    };
    let mut args = args.peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--compositor" => o.compositor = Some(args.next()?.into()),
            "--bus" => o.bus = Some(args.next()?.into()),
            "--lang" => o.lang = Lang::parse(&args.next()?)?,
            "--light" => o.light = true,
            "--source" => o.source = Some(args.next()?.into()),
            "--plans" => o.plans = args.next()?.into(),
            "--disk" => {
                let v = args.next()?;
                let (path, name) = v.split_once('=').unwrap_or((v.as_str(), v.as_str()));
                o.disks.push((path.into(), name.to_owned()));
            }
            _ => return None,
        }
    }
    Some(o)
}

enum Run {
    Exit(Exit),
    Failed,
}

type Report = Result<(u64, u64), Result<(), ultar_installer_ui::engine::EngineError>>;

/// Connects, builds the app and its window.
fn open(o: Options) -> Option<(UiWindow, InstallerApp)> {
    let socket = o.compositor.clone()?;
    let source = o.source.clone()?;
    let disks = o
        .disks
        .iter()
        .filter_map(|(p, n)| inspect(p, n).ok())
        .collect();
    let bus = Bus::connect(o.bus.as_ref()?, ClientOptions::named(APP_ID)).ok()?;
    let theme = if o.light {
        ThemeKind::Light
    } else {
        ThemeKind::Dark
    };
    let new_ui = || {
        let tr = ultar_installer_ui::i18n::translator(o.lang);
        Ui::new(
            PaintEnv::new(o.lang, theme),
            InputMode::Pointer,
            Size::new(1.0, 1.0),
            tr,
        )
        .ok()
    };
    let ui = new_ui()?;
    let client = Client::connect(&socket, APP_ID).ok()?;
    let title = ui.translator().t("installer.title").into_owned();
    let mut window = UiWindow::new(client, &WindowConfig::toplevel(&title), ui).ok()?;
    let ui = std::mem::replace(window.ui_mut(), new_ui()?);
    let wizard = Wizard::new(source, disks);
    let mut app = InstallerApp::new(
        ui,
        wizard,
        Box::new(BusDevices(bus)),
        Box::new(FolderStore(o.plans)),
    )
    .ok()?;
    std::mem::swap(window.ui_mut(), &mut app.ui);
    Some((window, app))
}

/// Starts writing the confirmed plan on its own thread.
fn start(app: &mut InstallerApp) -> Option<mpsc::Receiver<Report>> {
    let (confirm, plan) = app.take_install()?;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let progress_tx = tx.clone();
        let mut progress = move |d: u64, t: u64| {
            let _ = progress_tx.send(Ok((d, t)));
        };
        let result = write::execute(&plan, &confirm, &mut progress);
        let _ = tx.send(Err(result));
    });
    Some(rx)
}

fn run(o: Options) -> Run {
    let Some((mut window, mut app)) = open(o) else {
        return Run::Failed;
    };
    let mut worker: Option<mpsc::Receiver<Report>> = None;
    loop {
        let Ok(actions) = window.dispatch(Some(TICK)) else {
            return Run::Failed;
        };
        if window.window().close_requested() && worker.is_none() {
            return Run::Exit(Exit::Cancelled);
        }
        std::mem::swap(window.ui_mut(), &mut app.ui);
        let mut ok = app.actions(actions).is_ok();
        if let Some(rx) = start(&mut app) {
            worker = Some(rx);
        }
        if let Some(rx) = &worker {
            while let Ok(msg) = rx.try_recv() {
                ok &= match msg {
                    Ok((d, t)) => app.progress(d, t).is_ok(),
                    Err(result) => app.finished(result).is_ok(),
                };
            }
        }
        std::mem::swap(window.ui_mut(), &mut app.ui);
        if !ok {
            return Run::Failed;
        }
        if let Some(exit) = app.exit() {
            return Run::Exit(exit);
        }
        if window.draw().is_err() {
            return Run::Failed;
        }
    }
}

fn main() -> ExitCode {
    let Some(options) = parse(std::env::args().skip(1)) else {
        return ExitCode::from(2);
    };
    match run(options) {
        Run::Exit(Exit::Restart) => ExitCode::SUCCESS,
        Run::Exit(Exit::Cancelled) => ExitCode::from(3),
        Run::Failed => ExitCode::FAILURE,
    }
}
