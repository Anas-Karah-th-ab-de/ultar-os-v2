//! `ultar-installer-ui` – the graphical installer as a compositor client.
//!
//! ```text
//! ultar-installer-ui --source PATH (--disk PATH=NAME [--disk …] | --disks-dir DIR) [--plans DIR]
//!                    [--compositor PATH] [--bus PATH] [--lang ar|en|de] [--light] [--report]
//! ```
//! The live session's launcher passes the installation medium and the disks it offers. On the
//! Ultar kernel's installation medium (profile `installer`) these are the kernel's whole-disk
//! nodes: `--source /dev/ultar/medium --disks-dir /dev/ultar/disk` (every entry of the
//! directory is one disk, the boot disk is never among them; only the installer's user opens
//! them). `--report` writes one line per change of step, focus or the Next button's state to
//! standard error (`[installer-ui] step=… focus=… next=…`; never typed text) – the session's
//! console and the end-to-end test follow it. "Restart" on the last step asks the service manager
//! over the bus to end every service in order and restart the machine (`init.power`); when that
//! call is refused or the bus is gone, the installer only ends (status 0) and says why. Exit
//! status: 0 installed (restart), 3 cancelled (nothing written), 1 failure, 2 usage.
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
    report: bool,
}

/// Most disks taken from a `--disks-dir` directory.
const MAX_DISKS: usize = 32;

/// The entries of a disk directory (sorted by name), each named after its entry.
fn disks_in(dir: &std::path::Path) -> Option<Vec<(PathBuf, String)>> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .ok()?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter(|n| !n.starts_with('.'))
        .take(MAX_DISKS)
        .collect();
    names.sort();
    Some(names.into_iter().map(|n| (dir.join(&n), n)).collect())
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
        report: false,
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
            "--report" => o.report = true,
            "--disks-dir" => o
                .disks
                .extend(disks_in(std::path::Path::new(&args.next()?))?),
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
    // The version to be installed is the live system's (the medium boots what it installs).
    app.model.version = release_version(std::path::Path::new("/etc/os-release"));
    std::mem::swap(window.ui_mut(), &mut app.ui);
    Some((window, app))
}

/// `VERSION_ID` of an os-release file (at most 4 KiB read).
fn release_version(path: &std::path::Path) -> Option<String> {
    use std::io::Read;
    let mut text = String::new();
    std::fs::File::open(path)
        .ok()?
        .take(4096)
        .read_to_string(&mut text)
        .ok()?;
    ultar_brand::release::field(&text, "VERSION_ID").map(str::to_owned)
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

/// The line `--report` writes when step, focus or the Next button changed.
fn report_line(app: &InstallerApp) -> String {
    let focus = app.ui.focused().and_then(|node| {
        app.view
            .registry
            .entries()
            .iter()
            .find(|(n, _)| *n == node)
            .map(|(_, act)| format!("{act:?}"))
    });
    let next = app.view.next.map(|n| {
        if app.ui.is_disabled(n) {
            // the reason's catalog key (never a value the user typed)
            format!("off:{}", app.model.wizard.blocked().unwrap_or("-"))
        } else {
            "on".to_owned()
        }
    });
    format!(
        "[installer-ui] step={:?} focus={} next={}",
        app.model.wizard.step,
        focus.as_deref().unwrap_or("-"),
        next.as_deref().unwrap_or("-")
    )
}

fn run(o: Options) -> Run {
    let report = o.report;
    let Some((mut window, mut app)) = open(o) else {
        if report {
            eprintln!("[installer-ui] cannot start (compositor, bus, medium or disks missing)");
        }
        return Run::Failed;
    };
    let mut worker: Option<mpsc::Receiver<Report>> = None;
    let mut last = String::new();
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
                    Err(result) => {
                        if report {
                            match &result {
                                Ok(()) => eprintln!("[installer-ui] installation written"),
                                Err(e) => eprintln!("[installer-ui] installation failed: {e}"),
                            }
                        }
                        app.finished(result).is_ok()
                    }
                };
            }
        }
        if report {
            let line = report_line(&app);
            if line != last {
                eprintln!("{line}");
                last = line;
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

/// Asks the service manager to end every service in order and restart the machine.
fn restart(bus: Option<&std::path::Path>) -> Result<(), String> {
    use ultar_bus::messages::init::{Power, PowerAction};
    let path = bus.ok_or("no bus socket")?;
    let client = Bus::connect(path, ClientOptions::named(APP_ID)).map_err(|e| format!("{e:?}"))?;
    client
        .call(
            &Power {
                action: PowerAction::Reboot,
            },
            Duration::from_secs(5),
        )
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

fn main() -> ExitCode {
    let Some(options) = parse(std::env::args().skip(1)) else {
        return ExitCode::from(2);
    };
    let (bus, report) = (options.bus.clone(), options.report);
    match run(options) {
        Run::Exit(Exit::Restart) => {
            // said before the call: the service manager may end this process before the answer arrives
            if report {
                eprintln!("[installer-ui] restart requested");
            }
            if let Err(e) = restart(bus.as_deref()) {
                if report {
                    eprintln!("[installer-ui] restart not possible: {e}");
                }
            }
            ExitCode::SUCCESS
        }
        Run::Exit(Exit::Cancelled) => ExitCode::from(3),
        Run::Failed => ExitCode::FAILURE,
    }
}
