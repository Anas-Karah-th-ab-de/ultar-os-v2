//! `ultar-launcher` – the app launcher daemon, and (as its own helper) the
//! two stages of a cell and the platform probe.
//!
//! ```text
//! ultar-launcher [--config FILE] [--log-socket PATH] [--allow-root] [--lang ar|en|de]
//! ultar-launcher --cell …        (stage 1, started by the daemon)
//! ultar-launcher --cell-init …   (stage 2, started by stage 1)
//! ultar-launcher --probe DIR     (what the kernel offers; exit code)
//! ```
//!
//! Kernel rights the daemon keeps (everything else is dropped at start):
//! `mount` (namespaces, the cell's mounts), `set_id` (the app's user),
//! `file_admin` (owner of the data directories), `manage_groups` (the
//! app's memory group), `kill_any` (stopping apps of other users).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use ultar_launcher::cell::{self, CELL_ARG, CELL_INIT_ARG, PROBE_ARG};
use ultar_launcher::config::Config;
use ultar_launcher::msg::{language, text, Lang, Msg};
use ultar_launcher::platform::Platform;
use ultar_launcher::service::{self, Admins};
use ultar_launcher::Launcher;
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::rights::{Right, RightSet};
use ultar_service::{exit, Runtime, ServiceSpec, Wake};

/// The shipped configuration file.
const DEFAULT_CONFIG: &str = "/etc/ultar/launcher.toml";

/// The daemon's command line.
#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    config: Option<PathBuf>,
    log_socket: Option<PathBuf>,
    allow_root: bool,
}

fn parse(words: &[String]) -> Option<Args> {
    let mut args = Args::default();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--allow-root" => args.allow_root = true,
            "--config" => args.config = Some(PathBuf::from(words.next()?)),
            "--log-socket" => args.log_socket = Some(PathBuf::from(words.next()?)),
            "--lang" => {
                words.next()?;
            }
            _ => return None,
        }
    }
    Some(args)
}

fn say(lang: Lang, msg: Msg, detail: &str) {
    let _ = writeln!(std::io::stderr(), "{} {detail}", text(lang, msg));
}

fn code(value: i32) -> ExitCode {
    ExitCode::from(u8::try_from(value).unwrap_or(1))
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    match words.first().map(String::as_str) {
        Some(CELL_ARG) => return code(cell::stage_one(&words[1..])),
        Some(CELL_INIT_ARG) => return code(cell::stage_two(&words[1..])),
        Some(PROBE_ARG) => {
            return code(words.get(1).map_or(1, |dir| cell::probe(Path::new(dir))));
        }
        _ => {}
    }
    let lang = language(&words);
    let Some(args) = parse(&words) else {
        say(lang, Msg::BadArguments, "");
        return code(exit::USAGE);
    };
    let mut spec = ServiceSpec::new("ultar-launcher");
    spec.hardening.allow_root = args.allow_root;
    // apps get their own limits in their cell; the daemon keeps the defaults
    spec.hardening.limits = None;
    spec.hardening.rights = Some(RightSet::of(&[
        Right::Mount,
        Right::SetId,
        Right::FileAdmin,
        Right::ManageGroups,
        Right::KillAny,
    ]));
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    let runtime = match Runtime::start(spec) {
        Ok(runtime) => runtime,
        Err(error) => {
            say(lang, Msg::DaemonFailed, error.key());
            return code(error.exit_code());
        }
    };
    code(runtime.run(|runtime| work(&args, runtime, lang)))
}

/// Reads the configuration, detects the platform and opens the launcher.
fn open(args: &Args, runtime: &Runtime) -> Result<(Arc<Launcher>, Config), String> {
    let file = args
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from(DEFAULT_CONFIG));
    let config = Config::load(&file).map_err(|e| e.to_string())?;
    let helper = std::env::current_exe().map_err(|_| "launcher.start.helper".to_owned())?;
    ultar_launcher::datadir::ensure_private_dir(&config.paths.cell_root)
        .map_err(|e| e.to_string())?;
    let platform = Platform::detect(
        &helper,
        &config.paths.cell_root,
        config.paths.group_root.as_deref(),
    );
    let launcher = Launcher::open(config.clone(), platform, helper, runtime.logger().clone())
        .map_err(|e| e.to_string())?;
    Ok((launcher, config))
}

/// Everything after the runtime is up; runs on the work thread.
fn work(args: &Args, runtime: &Runtime, lang: Lang) -> i32 {
    let (launcher, config) = match open(args, runtime) {
        Ok(opened) => opened,
        Err(detail) => {
            say(lang, Msg::DaemonFailed, &detail);
            return exit::FAILURE;
        }
    };
    let policy = config.paths.policy.clone();
    let admins = Arc::new(Mutex::new(Admins::load(policy.as_deref())));
    let timer = {
        let launcher = Arc::clone(&launcher);
        std::thread::Builder::new()
            .name("ultar-launcher-timer".into())
            .spawn(move || launcher.run_timer())
    };
    let Ok(timer) = timer else {
        return exit::FAILURE;
    };
    if let Some(socket) = config.paths.bus_socket {
        let follow = {
            let (launcher, life) = (Arc::clone(&launcher), runtime.lifecycle().clone());
            let socket = socket.clone();
            std::thread::Builder::new()
                .name("ultar-launcher-follow".into())
                .spawn(move || ultar_launcher::follow::serve(socket, launcher, life))
        };
        if follow.is_err() {
            launcher.shutdown();
            let _ = timer.join();
            return exit::FAILURE;
        }
        let (served, admins, life) = (
            Arc::clone(&launcher),
            Arc::clone(&admins),
            runtime.lifecycle().clone(),
        );
        let bus = std::thread::Builder::new()
            .name("ultar-launcher-bus".into())
            .spawn(move || service::serve(socket, served, admins, life));
        if bus.is_err() {
            launcher.shutdown();
            let _ = timer.join();
            return exit::FAILURE;
        }
    }
    runtime.ready();
    let mut reloads = 0;
    while let Wake::Reload(count) = runtime.lifecycle().wait_change(reloads) {
        reloads = count;
        *admins.lock().unwrap_or_else(PoisonError::into_inner) = Admins::load(policy.as_deref());
    }
    launcher.shutdown();
    let _ = timer.join();
    exit::OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_daemons_command_line() {
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(parse(&[]), Some(Args::default()));
        let args = parse(&words(&[
            "--config",
            "/c",
            "--allow-root",
            "--lang",
            "ar",
            "--log-socket",
            "/l",
        ]))
        .unwrap();
        assert_eq!(args.config, Some(PathBuf::from("/c")));
        assert_eq!(args.log_socket, Some(PathBuf::from("/l")));
        assert!(args.allow_root);
        assert_eq!(parse(&words(&["--config"])), None);
        assert_eq!(parse(&words(&["--frob"])), None);
        assert_eq!(code(300), ExitCode::from(1));
    }
}
