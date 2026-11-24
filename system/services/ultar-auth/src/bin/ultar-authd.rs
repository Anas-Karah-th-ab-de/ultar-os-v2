//! `ultar-authd` – the authentication service of Ultar OS.
//!
//! ```text
//! ultar-authd --dir DIR --socket PATH --policy FILE [--log-socket PATH] [--allow-root]
//! ultar-authd --dir DIR passwd USER [--display NAME] [--pin] [--lang ar|en|de]
//! ```
//!
//! The daemon serves `auth.*` on the bus until `SIGTERM`. `passwd` (administrator at the
//! console, the installer) reads the secret twice from standard input – never from the
//! command line – and creates the account if it does not exist (the first account is the
//! administrator).

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;
use ultar_auth::hash::Cost;
use ultar_auth::{msg, server, Auth, AuthConfig, SystemClock};
use ultar_bus::messages::auth::{wipe, Method, Secret, SetOutcome, SetSecret, SERVICE};
use ultar_bus::{Client, ClientOptions, Policy};
use ultar_i18n::{Arg, Lang};
use ultar_service::log::{LogTarget, LoggerConfig};
use ultar_service::{exit, Runtime, ServiceSpec};

/// What the command line asks for.
#[derive(Debug, Default)]
struct Args {
    dir: Option<PathBuf>,
    socket: Option<PathBuf>,
    policy: Option<PathBuf>,
    log_socket: Option<PathBuf>,
    allow_root: bool,
    passwd: Option<String>,
    display: Option<String>,
    pin: bool,
    lang: Option<Lang>,
}

fn parse(mut argv: impl Iterator<Item = String>) -> Option<Args> {
    let mut a = Args::default();
    while let Some(word) = argv.next() {
        match word.as_str() {
            "--dir" => a.dir = Some(PathBuf::from(argv.next()?)),
            "--socket" => a.socket = Some(PathBuf::from(argv.next()?)),
            "--policy" => a.policy = Some(PathBuf::from(argv.next()?)),
            "--log-socket" => a.log_socket = Some(PathBuf::from(argv.next()?)),
            "--allow-root" => a.allow_root = true,
            "passwd" => a.passwd = Some(argv.next()?),
            "--display" => a.display = Some(argv.next()?),
            "--pin" => a.pin = true,
            "--lang" => a.lang = Lang::parse(&argv.next()?),
            _ => return None,
        }
    }
    a.dir.as_ref()?;
    Some(a)
}

fn read_secret(input: &mut impl BufRead) -> Option<Secret> {
    let mut line = String::new();
    input.read_line(&mut line).ok()?;
    let text = line.trim_end_matches(['\n', '\r']).to_owned();
    wipe(line);
    Some(Secret::new(text))
}

fn passwd(args: &Args, user: &str) -> ExitCode {
    let tr = msg::translator(args.lang.unwrap_or_else(Lang::from_env));
    let mut out = std::io::stderr();
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let (Some(first), Some(second)) = (read_secret(&mut input), read_secret(&mut input)) else {
        let _ = writeln!(out, "{}", tr.t("auth.cli.usage"));
        return ExitCode::from(2);
    };
    if first != second {
        let _ = writeln!(out, "{}", tr.t("auth.cli.mismatch"));
        return ExitCode::from(1);
    }
    let Some(dir) = args.dir.clone() else {
        return ExitCode::from(2);
    };
    let mut auth = Auth::open(AuthConfig {
        dir,
        clock: Arc::new(SystemClock),
        cost: Cost::STANDARD,
    });
    let request = SetSecret {
        user: user.to_owned(),
        display_name: args.display.clone().unwrap_or_else(|| user.to_owned()),
        method: if args.pin {
            Method::Pin
        } else {
            Method::Password
        },
        current: None,
        new: first,
    };
    let (key, code) = match auth.set_secret(&request, true) {
        SetOutcome::Done => ("auth.cli.done", 0),
        SetOutcome::Rejected | SetOutcome::Denied { .. } => ("auth.cli.rejected", 1),
        SetOutcome::Unavailable => ("auth.cli.failed", 1),
    };
    let text = tr.t_args(key, &[("detail", Arg::from("users"))]);
    let _ = writeln!(out, "{text}");
    ExitCode::from(code)
}

fn daemon(args: &Args) -> ExitCode {
    let (Some(dir), Some(socket), Some(policy)) = (&args.dir, &args.socket, &args.policy) else {
        let tr = msg::translator(args.lang.unwrap_or_else(Lang::from_env));
        let _ = writeln!(std::io::stderr(), "{}", tr.t("auth.cli.usage"));
        return ExitCode::from(2);
    };
    let Ok(policy) = Policy::load(policy) else {
        return ExitCode::from(exit::USAGE as u8);
    };
    let mut spec = ServiceSpec::new("ultar-authd");
    spec.hardening.allow_root = args.allow_root;
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    let Ok(runtime) = Runtime::start(spec) else {
        return ExitCode::from(exit::FAILURE as u8);
    };
    let dir = dir.clone();
    let socket = socket.clone();
    let code = runtime.run(move |rt| {
        let mut auth = Auth::open(AuthConfig {
            dir,
            clock: Arc::new(SystemClock),
            cost: Cost::STANDARD,
        });
        if !auth.usable() {
            rt.logger().emit(ultar_service::log::Record::new(
                ultar_service::log::Level::Error,
                "auth.store.unusable",
            ));
        }
        let Ok(bus) = Client::connect(&socket, ClientOptions::named("ultar-authd")) else {
            return exit::FAILURE;
        };
        let Ok(endpoint) = bus.register(SERVICE) else {
            return exit::FAILURE;
        };
        rt.ready();
        let stop = || rt.lifecycle().is_stopping();
        match server::serve(
            &endpoint,
            &mut auth,
            &policy,
            Duration::from_millis(200),
            &stop,
        ) {
            Ok(()) => exit::OK,
            Err(_) => exit::FAILURE,
        }
    });
    ExitCode::from(u8::try_from(code).unwrap_or(1))
}

fn main() -> ExitCode {
    let Some(args) = parse(std::env::args().skip(1)) else {
        let tr = msg::translator(Lang::from_env());
        let _ = writeln!(std::io::stderr(), "{}", tr.t("auth.cli.usage"));
        return ExitCode::from(2);
    };
    match args.passwd.clone() {
        Some(user) => passwd(&args, &user),
        None => daemon(&args),
    }
}
