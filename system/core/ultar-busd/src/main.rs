//! `ultar-busd` – runs the Ultar OS bus broker on the service runtime.
//!
//! ```text
//! ultar-busd [--listen PATH=IDENTITY | --listen-cred PATH | --listen-token PATH
//!             | --listen-apps PATH]...
//!            [--app-map FILE --app-map-owner UID --app-uids FIRST-LAST]
//!            [--mode OCTAL] [--owner UID] [--group GID] [--max-connections N]
//!            [--uid-map FILE] [--token-file FILE | --token-fd N]
//!            [--policy FILE | --allow-all]
//!            [--log-socket PATH] [--allow-root] [--lang ar|en|de] [PATH]
//! ```
//!
//! * `--listen PATH=IDENTITY`: every peer of the socket is `IDENTITY`; the
//!   socket file's owner and mode (or the mount that exposes it in a cell)
//!   decide who can connect.
//! * `--listen-cred PATH`: peers are identified by the **kernel
//!   credentials** of the connection (`SO_PEERCRED`) through `--uid-map`;
//!   a user id without entry is refused. Refused at start when the kernel
//!   does not report credentials.
//! * `--listen-token PATH`: peers present a token from `--token-file`
//!   (the fallback for kernels without credentials) or from the table the
//!   service manager passes on descriptor `--token-fd N` (for kernels whose
//!   file owners and modes cannot be checked).
//! * `--listen-apps PATH`: the apps' socket. A peer is the app whose user
//!   id (kernel credentials) the launcher's file `--app-map` names
//!   (`UID = "app.<id>"`, only for apps whose manifest declares bus access);
//!   ids outside `--app-uids`, ids without line and a file that is not the
//!   launcher's (`--app-map-owner`) or not safe are refused (`apps.rs`).
//! * `--mode`, `--owner`, `--group`, `--max-connections` apply to the
//!   listener named before them; the sum of all bounds must fit the
//!   broker's limits (see `budget.rs`), or the start is refused.
//! * A bare `PATH` (or `ULTAR_BUS_SOCKET`) is `--listen PATH=system`.
//!
//! `SIGTERM`/`SIGINT` stop the broker: clients are told `ShuttingDown`,
//! the socket files are removed, the exit code is 0. `SIGHUP` reads the
//! policy file again; a faulty file keeps the old policy.
//!
//! A fixed-identity socket whose file, owner or mode the kernel does not
//! show after binding is reported as `bus.identity.unenforced` (warning).
//!
//! Self-limits (see `ultar_service::harden`): no core dumps, no address
//! space limit (memory is bounded by the group), 1024 descriptors, 1024
//! threads, umask 077, no kernel rights, and no root unless `--allow-root`
//! says so.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

mod apps;
mod args;
mod budget;
mod enforce;
mod observe;
#[cfg(test)]
mod policy_tests;
#[cfg(test)]
mod service_files_tests;
mod tokens;

use args::{parse_arguments, Arguments, IdentitySource, SOCKET_ENV};
use std::io::Write;
use std::process::ExitCode;
use std::sync::Arc;
use ultar_bus::msg::{synopsis, text, Lang, MsgKey};
use ultar_bus::{Broker, BusError, FixedIdentity, Identity, PeerIdentity, Policy};
use ultar_service::log::{Level, LogTarget, LoggerConfig, Record};
use ultar_service::{
    exit, peercred, socket, HardeningError, Runtime, SecretSource, ServiceSpec, StartError, UidMap,
    Wake, MAX_SECRET_TABLE_LEN,
};

/// Writes one operator line to standard error; a closed stream is ignored.
fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

fn say_key(lang: Lang, key: MsgKey) {
    say(text(lang, key));
}

fn say_detail(lang: Lang, key: MsgKey, detail: &dyn std::fmt::Display) {
    say(&format!("{} {detail}", text(lang, key)));
}

fn print_usage(lang: Lang) {
    say(&format!("{}\n  {}", text(lang, MsgKey::Usage), synopsis()));
}

fn load_policy(arguments: &Arguments, lang: Lang) -> Option<Policy> {
    match &arguments.policy {
        Some(path) => match Policy::load(path) {
            Ok(policy) => Some(policy),
            Err(err) => {
                say(&format!(
                    "{} {}: {err}",
                    text(lang, MsgKey::PolicyRejected),
                    path.display()
                ));
                None
            }
        },
        None if arguments.allow_all => {
            say_key(lang, MsgKey::AllowAllWarning);
            Some(Policy::allow_all())
        }
        None => {
            say_key(lang, MsgKey::PolicyRequired);
            None
        }
    }
}

/// The authenticators the listeners share.
struct Sources {
    credentials: Option<Arc<dyn PeerIdentity>>,
    tokens: Option<Arc<dyn PeerIdentity>>,
}

/// Loads the uid map and the token file, if listeners need them. Fails
/// closed: a credential listener on a kernel without credentials, or a
/// faulty file, stops the start.
fn load_sources(arguments: &Arguments, runtime: &Runtime, lang: Lang) -> Option<Sources> {
    let needs = |source: &IdentitySource| arguments.listeners.iter().any(|l| l.source == *source);
    let mut sources = Sources {
        credentials: None,
        tokens: None,
    };
    if let (true, Some(path)) = (needs(&IdentitySource::Credentials), &arguments.uid_map) {
        if !peercred::probe() {
            say_key(lang, MsgKey::CredentialsUnsupported);
            return None;
        }
        match UidMap::load(path) {
            Ok(map) => sources.credentials = Some(Arc::new(map.into_peer_identity())),
            Err(err) => {
                say_detail(lang, MsgKey::UidMapRejected, &err);
                return None;
            }
        }
    }
    if let (true, Some(source)) = (needs(&IdentitySource::Token), &arguments.token) {
        let table = runtime.read_secret(source, MAX_SECRET_TABLE_LEN);
        match table
            .map_err(tokens::TokenFileError::Secret)
            .and_then(|t| tokens::from_secret(&t))
        {
            Ok(auth) => sources.tokens = Some(Arc::new(auth)),
            Err(err) => {
                say_detail(lang, MsgKey::TokenFileRejected, &err);
                return None;
            }
        }
    }
    Some(sources)
}

/// The apps' authenticator: kernel credentials and the launcher's app map
/// (checked at every connect, see `apps.rs`).
fn app_identity(arguments: &Arguments) -> Option<Arc<dyn PeerIdentity>> {
    match (
        &arguments.app_map,
        arguments.app_map_owner,
        arguments.app_uids,
    ) {
        (Some(path), Some(owner), Some(uids)) => {
            Some(Arc::new(apps::AppIdentity::new(apps::AppMapSource {
                path: path.clone(),
                owner,
                uids,
            })))
        }
        _ => None,
    }
}

fn start_broker(
    arguments: &Arguments,
    policy: Policy,
    sources: &Sources,
    runtime: &Runtime,
    lang: Lang,
) -> Option<Broker> {
    let observer = Arc::new(observe::LogObserver(runtime.logger().clone()));
    let mut builder = Broker::builder(policy)
        .observer(observer)
        .config(budget::config());
    for listener in &arguments.listeners {
        let auth: Option<Arc<dyn PeerIdentity>> = match &listener.source {
            IdentitySource::Fixed(name) => Identity::new(name)
                .ok()
                .map(|identity| Arc::new(FixedIdentity::new(identity)) as Arc<dyn PeerIdentity>),
            IdentitySource::Credentials => sources.credentials.clone(),
            IdentitySource::Token => sources.tokens.clone(),
            IdentitySource::Apps => app_identity(arguments),
        };
        let Some(auth) = auth else {
            say_key(lang, MsgKey::BadArguments);
            return None;
        };
        // `/run` is empty after every boot: create the socket directory.
        // The socket's own mode decides who may connect.
        if let Err(err) = socket::ensure_parent(&listener.path, 0o755) {
            say_detail(lang, MsgKey::StartFailed, &err);
            return None;
        }
        builder = builder.listen_with_access(&listener.path, auth, listener.access);
    }
    if let Err(shortfall) = budget::check(builder.needs()) {
        say_detail(lang, MsgKey::LimitsTooSmall, &shortfall);
        return None;
    }
    match builder.start() {
        Ok(broker) => Some(broker),
        Err(BusError::NotASocket) => {
            say_key(lang, MsgKey::PathNotSocket);
            None
        }
        Err(err) => {
            say_detail(lang, MsgKey::StartFailed, &err);
            None
        }
    }
}

fn start_runtime(arguments: &Arguments, lang: Lang) -> Result<Runtime, ExitCode> {
    let mut spec = ServiceSpec::new("ultar-busd");
    spec.hardening.allow_root = arguments.allow_root;
    spec.hardening.limits = Some(budget::LIMITS);
    if let Some(SecretSource::Fd(fd)) = &arguments.token {
        spec.inherited.push(*fd);
    }
    if let Some(path) = &arguments.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    Runtime::start(spec).map_err(|error| {
        match error {
            StartError::Hardening(HardeningError::RunningAsRoot) => {
                say_key(lang, MsgKey::RootRefused);
            }
            other => say_detail(lang, MsgKey::HardeningFailed, &other),
        }
        exit::code(error.exit_code())
    })
}

/// Reads the policy file again after `SIGHUP`; a faulty file changes
/// nothing.
fn reload_policy(arguments: &Arguments, broker: &Broker, runtime: &Runtime) {
    let Some(path) = &arguments.policy else {
        return;
    };
    match Policy::load(path) {
        Ok(policy) => {
            broker.set_policy(policy);
            runtime.logger().log(Level::Notice, "bus.policy_reloaded");
        }
        Err(_) => runtime
            .logger()
            .log(Level::Error, "bus.policy_reload_rejected"),
    }
}

/// Serves until a stop is requested, then shuts the broker down.
fn serve(arguments: &Arguments, broker: Broker, runtime: &Runtime) {
    let mut reloads = 0;
    while let Wake::Reload(count) = runtime.lifecycle().wait_change(reloads) {
        reloads = count;
        reload_policy(arguments, &broker, runtime);
    }
    let stats = broker.stats();
    runtime.logger().emit(
        Record::new(Level::Info, "bus.stopping")
            .count("connections", stats.connections as u64)
            .count("dropped_slow", stats.dropped_slow),
    );
    // Tells every client `ShuttingDown` and removes the socket files.
    broker.shutdown();
}

/// The language for operator messages: `--lang` wherever it stands on the
/// command line (also when the rest could not be parsed), else the
/// environment.
fn language(words: &[String]) -> Lang {
    words
        .iter()
        .position(|word| word == "--lang")
        .and_then(|index| words.get(index + 1))
        .map_or_else(Lang::from_env, |tag| Lang::from_tag(tag))
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let lang = language(&words);
    let parsed = parse_arguments(words.into_iter(), std::env::var(SOCKET_ENV).ok());
    let arguments = match parsed {
        Ok(arguments) => arguments,
        Err(error) => {
            say_key(
                lang,
                match error {
                    args::ArgumentError::PolicyConflict => MsgKey::PolicyConflict,
                    _ => MsgKey::BadArguments,
                },
            );
            print_usage(lang);
            return exit::code(exit::USAGE);
        }
    };
    if arguments.help {
        print_usage(lang);
        return ExitCode::SUCCESS;
    }
    // Before anything is opened and before the first thread exists.
    let runtime = match start_runtime(&arguments, lang) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    exit::code(runtime.run(|runtime| work(&arguments, runtime, lang)))
}

/// Everything after the runtime is up; runs on the work thread.
fn work(arguments: &Arguments, runtime: &Runtime, lang: Lang) -> i32 {
    let Some(policy) = load_policy(arguments, lang) else {
        return exit::USAGE;
    };
    let Some(sources) = load_sources(arguments, runtime, lang) else {
        return exit::FAILURE;
    };
    let Some(broker) = start_broker(arguments, policy, &sources, runtime, lang) else {
        return exit::FAILURE;
    };
    for path in broker.paths() {
        say(&format!(
            "{} {}",
            text(lang, MsgKey::Listening),
            path.display()
        ));
    }
    runtime.logger().emit(
        Record::new(Level::Info, "bus.listening").count("sockets", broker.paths().len() as u64),
    );
    for listener in enforce::unenforced(&arguments.listeners) {
        runtime.logger().emit(
            Record::new(Level::Warn, "bus.identity.unenforced").count("listener", listener as u64),
        );
    }
    runtime.ready();
    serve(arguments, broker, runtime);
    say_key(lang, MsgKey::Stopped);
    exit::OK
}
