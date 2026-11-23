//! Command line and main flow of the `ultar-devd` binary.
//!
//! The daemon runs on the service runtime (`ultar_service`): it limits itself at start,
//! stops cleanly on `SIGTERM`/`SIGINT`, reads its bus token from a file or an inherited
//! descriptor, logs through the log service and publishes the retained capability set
//! again after a broker restart. `--once` is a tool mode without any of that: resolve,
//! print, exit.

use crate::access::{AdminAccess, AllowAll, PolicyAccess};
use crate::bus::{MemoryPublisher, SessionPublisher};
use crate::cli::{print_list, EXIT_FAILED, EXIT_OK, EXIT_USAGE};
use crate::daemon::{Daemon, DaemonConfig};
use crate::msg::Messages;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::device_service::{DevicePhase, SERVICE};
use ultar_bus::Policy;
use ultar_i18n::Arg;
use ultar_service::log::{Level, LogTarget, LoggerConfig, Record};
use ultar_service::{
    read_secret_fd, read_secret_file, BusSession, HardeningError, Runtime, Secret, SecretError,
    SecretFilePolicy, ServiceSpec, SessionOptions, StartError, StopCause,
};

/// How long the daemon waits for the bus broker to appear at start.
pub const BUS_WAIT: Duration = Duration::from_secs(30);

/// The parsed command line of the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonArgs {
    /// Inputs and state location.
    pub config: DaemonConfig,
    /// Bus socket; not needed with `--once`.
    pub bus: Option<PathBuf>,
    /// Resolve once, print the device list and exit (no bus).
    pub once: bool,
    /// Language option.
    pub lang: Option<String>,
    /// Bus policy file that decides who has `devices.admin`.
    pub policy: Option<PathBuf>,
    /// Every caller the broker lets through is an administrator.
    pub allow_all: bool,
    /// File with the bus token (owner and mode are checked).
    pub token_file: Option<PathBuf>,
    /// Inherited descriptor to read the bus token from instead.
    pub token_fd: Option<i32>,
    /// Ingestion socket of the log service.
    pub log_socket: Option<PathBuf>,
    /// The operator declares that the daemon runs as root.
    pub allow_root: bool,
}

/// Why the command line was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageError {
    /// Missing or unknown option, missing value.
    Arguments,
    /// Neither `--policy` nor `--allow-all` (and not `--once`).
    NoPolicy,
    /// `--policy` together with `--allow-all`.
    PolicyConflict,
}

impl UsageError {
    /// Key of the message for the operator.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Arguments => "devd.cli.daemon-usage",
            Self::NoPolicy => "devd.daemon.policy-required",
            Self::PolicyConflict => "devd.daemon.policy-conflict",
        }
    }
}

#[derive(Default)]
struct Raw {
    paths: [Option<PathBuf>; 8],
    kernel_abi: Option<String>,
    lang: Option<String>,
    token_fd: Option<i32>,
    once: bool,
    allow_all: bool,
    allow_root: bool,
}

/// Index of a path option in [`Raw::paths`].
fn path_slot(option: &str) -> Option<usize> {
    const OPTIONS: [&str; 8] = [
        "--depot",
        "--keys",
        "--state",
        "--bus",
        "--root",
        "--policy",
        "--token-file",
        "--log-socket",
    ];
    OPTIONS.iter().position(|name| *name == option)
}

fn collect(args: &[String]) -> Result<Raw, UsageError> {
    let mut raw = Raw::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || iter.next().ok_or(UsageError::Arguments);
        match arg.as_str() {
            "--once" => raw.once = true,
            "--allow-all" => raw.allow_all = true,
            "--allow-root" => raw.allow_root = true,
            "--kernel-abi" => raw.kernel_abi = Some(value()?.clone()),
            "--lang" => raw.lang = Some(value()?.clone()),
            "--token-fd" => {
                raw.token_fd = Some(value()?.parse().map_err(|_| UsageError::Arguments)?);
            }
            // There is no `--token`: a secret on the command line is visible to
            // every process on the machine.
            option => {
                let slot = path_slot(option).ok_or(UsageError::Arguments)?;
                raw.paths[slot] = Some(PathBuf::from(value()?));
            }
        }
    }
    Ok(raw)
}

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<DaemonArgs, UsageError> {
    let raw = collect(args)?;
    let [depot, keys, state, bus, root, policy, token_file, log_socket] = raw.paths;
    if bus.is_none() && !raw.once {
        return Err(UsageError::Arguments);
    }
    if token_file.is_some() && raw.token_fd.is_some() {
        return Err(UsageError::Arguments);
    }
    let config = DaemonConfig {
        root: root.unwrap_or_else(|| PathBuf::from("/")),
        depot: depot.ok_or(UsageError::Arguments)?,
        keys: keys.ok_or(UsageError::Arguments)?,
        state: state.ok_or(UsageError::Arguments)?,
        kernel_abi: raw.kernel_abi,
    };
    if policy.is_some() && raw.allow_all {
        return Err(UsageError::PolicyConflict);
    }
    if !raw.once && policy.is_none() && !raw.allow_all {
        return Err(UsageError::NoPolicy);
    }
    Ok(DaemonArgs {
        config,
        bus,
        once: raw.once,
        lang: raw.lang,
        policy,
        allow_all: raw.allow_all,
        token_file,
        token_fd: raw.token_fd,
        log_socket,
        allow_root: raw.allow_root,
    })
}

fn log(err: &mut dyn Write, text: &str) {
    let _ = writeln!(err, "ultar-devd: {text}");
}

fn started(daemon: &Daemon, messages: &Messages, runtime: &Runtime, err: &mut dyn Write) {
    let devices = daemon.list().devices;
    let active = devices
        .iter()
        .filter(|d| d.phase == DevicePhase::Active)
        .count();
    let count = |n: usize| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let args = [("devices", count(devices.len())), ("active", count(active))];
    log(err, &messages.t_args("devd.daemon.started", &args));
    runtime.logger().emit(
        Record::new(Level::Info, "devd.started")
            .count("devices", devices.len() as u64)
            .count("active", active as u64),
    );
}

fn start_failed(
    error: &crate::daemon::StartError,
    messages: &Messages,
    err: &mut dyn Write,
) -> i32 {
    let detail = match error {
        crate::daemon::StartError::Hardware(inner) => messages.t(inner.key()),
        crate::daemon::StartError::UnknownKernel => String::new(),
    };
    log(
        err,
        &messages.t_args(error.key(), &[("a", Arg::Str(&detail))]),
    );
    EXIT_FAILED
}

/// `--once`: resolve the devices without a bus, print the list, exit.
fn run_once(
    args: DaemonArgs,
    messages: &Messages,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    match Daemon::start(args.config, Box::new(MemoryPublisher::default())) {
        Ok(daemon) => {
            print_list(out, messages, &daemon.list());
            EXIT_OK
        }
        Err(error) => start_failed(&error, messages, err),
    }
}

fn start_runtime(
    args: &DaemonArgs,
    messages: &Messages,
    err: &mut dyn Write,
) -> Result<Runtime, i32> {
    let mut spec = ServiceSpec::new("ultar-devd");
    spec.hardening.allow_root = args.allow_root;
    if let Some(fd) = args.token_fd {
        spec.hardening.keep_descriptors.push(fd);
    }
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    Runtime::start(spec).map_err(|error| {
        match error {
            StartError::Hardening(HardeningError::RunningAsRoot) => {
                log(err, &messages.t("devd.daemon.root-refused"));
            }
            other => {
                let detail = other.to_string();
                let args = [("a", Arg::Str(&detail))];
                log(err, &messages.t_args("devd.daemon.hardening-failed", &args));
            }
        }
        error.exit_code()
    })
}

fn load_access(args: &DaemonArgs) -> Result<Box<dyn AdminAccess>, String> {
    match &args.policy {
        None => Ok(Box::new(AllowAll)),
        Some(path) => Policy::load(path)
            .map(|policy| Box::new(PolicyAccess(policy)) as Box<dyn AdminAccess>)
            .map_err(|error| error.to_string()),
    }
}

fn load_token(args: &DaemonArgs) -> Result<Option<Secret>, SecretError> {
    match (&args.token_file, args.token_fd) {
        (Some(path), _) => read_secret_file(path, &SecretFilePolicy::for_current_user()).map(Some),
        (None, Some(fd)) => read_secret_fd(fd).map(Some),
        (None, None) => Ok(None),
    }
}

/// Everything after the runtime is up: connect, start, serve. Returns the exit code.
fn serve(
    parsed: DaemonArgs,
    socket: &std::path::Path,
    messages: &Messages,
    runtime: &Runtime,
    err: &mut dyn Write,
) -> i32 {
    let fail = |key: &str, detail: &str, err: &mut dyn Write| {
        log(err, &messages.t_args(key, &[("a", Arg::Str(detail))]));
        EXIT_FAILED
    };
    let access = match load_access(&parsed) {
        Ok(access) => access,
        Err(detail) => return fail("devd.daemon.policy-rejected", &detail, err),
    };
    let mut options = SessionOptions::named("ultar-devd");
    options.connect_wait = BUS_WAIT;
    options.token = match load_token(&parsed) {
        Ok(token) => token,
        // The key of the reason, never any part of the token.
        Err(error) => return fail("devd.daemon.token-rejected", error.key(), err),
    };
    let bus_lost = |error: &ultar_bus::BusError, err: &mut dyn Write| {
        runtime.logger().log(Level::Error, "devd.bus_lost");
        fail("devd.daemon.bus-lost", &error.to_string(), err)
    };
    let session = match BusSession::connect(socket, options) {
        Ok(session) => Arc::new(session),
        Err(error) => return bus_lost(&error, err),
    };
    // Register first: a client that reacts to the retained capability message can call
    // the service at once; its request waits in the endpoint until serving starts.
    let endpoint = match session.register(SERVICE) {
        Ok(endpoint) => endpoint,
        Err(error) => return bus_lost(&error, err),
    };
    let publisher = Box::new(SessionPublisher(session.clone()));
    let mut daemon = match Daemon::start(parsed.config, publisher) {
        Ok(daemon) => daemon,
        Err(error) => return start_failed(&error, messages, err),
    };
    daemon.set_access(access);
    daemon.set_logger(runtime.logger().clone());
    // A stop signal closes the connection, which ends the blocking wait in `serve`.
    let closer = session.client().clone();
    runtime.lifecycle().on_stop(move || closer.close());
    started(&daemon, messages, runtime, err);
    runtime.ready();
    match daemon.serve(&endpoint) {
        Err(error) if !runtime.lifecycle().is_stopping() => bus_lost(&error, err),
        _ => stopped(messages, runtime, err),
    }
}

/// The orderly end: by a stop signal or by `devices.shutdown`.
fn stopped(messages: &Messages, runtime: &Runtime, err: &mut dyn Write) -> i32 {
    // `devices.shutdown` ends the service without a signal; tell the runtime.
    runtime.lifecycle().request_stop(StopCause::Request);
    runtime.logger().log(Level::Info, "devd.stopped");
    log(err, &messages.t("devd.daemon.stopped"));
    EXIT_OK
}

/// The whole daemon: parse, harden, connect, start, serve. Returns the exit code.
///
/// Must be called first thing in `main`: the service runtime it starts has to block the
/// stop signals before any other thread exists.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut (dyn Write + Send)) -> i32 {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(error) => {
            let lang = args
                .iter()
                .position(|arg| arg == "--lang")
                .and_then(|index| args.get(index + 1));
            let messages = Messages::new(Messages::choose_lang(lang.map(String::as_str)));
            if error != UsageError::Arguments {
                let _ = writeln!(err, "{}", messages.t(error.key()));
            }
            let _ = writeln!(err, "{}", messages.t("devd.cli.daemon-usage"));
            return EXIT_USAGE;
        }
    };
    let messages = Messages::new(Messages::choose_lang(parsed.lang.as_deref()));
    let Some(socket) = parsed.bus.clone().filter(|_| !parsed.once) else {
        return run_once(parsed, &messages, out, err);
    };
    let runtime = match start_runtime(&parsed, &messages, err) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    // The work runs on a second thread (the main thread keeps the signals), which
    // makes its own message catalogue; `err` moves there for the daemon's lines.
    let lang = messages.lang();
    runtime.run(move |runtime| {
        let messages = Messages::new(lang);
        serve(parsed, &socket, &messages, runtime, err)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| (*w).to_string()).collect()
    }

    #[test]
    fn parses_all_options() {
        let parsed = parse(&args(&[
            "--depot",
            "/d",
            "--keys",
            "/k",
            "--state",
            "/s",
            "--bus",
            "/b.sock",
            "--root",
            "/r",
            "--kernel-abi",
            "ultar-kern-abi-1",
            "--lang",
            "en",
            "--policy",
            "/p.toml",
            "--token-file",
            "/t",
            "--log-socket",
            "/l.sock",
            "--allow-root",
        ]))
        .unwrap();
        assert_eq!(parsed.policy, Some(PathBuf::from("/p.toml")));
        assert_eq!(parsed.token_file, Some(PathBuf::from("/t")));
        assert_eq!(parsed.log_socket, Some(PathBuf::from("/l.sock")));
        assert!(parsed.allow_root && !parsed.allow_all && parsed.token_fd.is_none());
        assert_eq!(parsed.config.depot, PathBuf::from("/d"));
        assert_eq!(parsed.config.keys, PathBuf::from("/k"));
        assert_eq!(parsed.config.state, PathBuf::from("/s"));
        assert_eq!(parsed.config.root, PathBuf::from("/r"));
        assert_eq!(parsed.bus, Some(PathBuf::from("/b.sock")));
        assert_eq!(
            parsed.config.kernel_abi.as_deref(),
            Some("ultar-kern-abi-1")
        );
        assert_eq!(parsed.lang.as_deref(), Some("en"));
        assert!(!parsed.once);
        let once = parse(&args(&[
            "--depot", "/d", "--keys", "/k", "--state", "/s", "--once",
        ]));
        let once = once.unwrap();
        assert!(once.once && once.bus.is_none());
        assert_eq!(once.config.root, PathBuf::from("/"));
    }

    #[test]
    fn missing_or_unknown_options_are_usage_errors() {
        for bad in [
            &["--depot", "/d", "--keys", "/k", "--state", "/s"][..],
            &["--depot", "/d", "--keys", "/k", "--bus", "/b"],
            &["--depot", "/d", "--state", "/s", "--bus", "/b"],
            &["--keys", "/k", "--state", "/s", "--bus", "/b"],
            &["--depot"],
            &["--kernel-abi"],
            &["--lang"],
            &["--what"],
            // A secret on the command line is not accepted.
            &[
                "--depot", "/d", "--keys", "/k", "--state", "/s", "--bus", "/b", "--token", "x",
            ],
            &[
                "--depot",
                "/d",
                "--keys",
                "/k",
                "--state",
                "/s",
                "--bus",
                "/b",
                "--token-fd",
                "x",
            ],
            &[],
        ] {
            assert_eq!(parse(&args(bad)), Err(UsageError::Arguments), "{bad:?}");
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["--what"]), &mut out, &mut err), EXIT_USAGE);
        assert!(!err.is_empty());
    }

    #[test]
    fn access_and_token_options_are_checked() {
        let base = [
            "--depot", "/d", "--keys", "/k", "--state", "/s", "--bus", "/b",
        ];
        assert_eq!(parse(&args(&base)), Err(UsageError::NoPolicy));
        let both = [&base[..], &["--policy", "/p", "--allow-all"]].concat();
        assert_eq!(parse(&args(&both)), Err(UsageError::PolicyConflict));
        let fd = [&base[..], &["--allow-all", "--token-fd", "5"]].concat();
        assert_eq!(parse(&args(&fd)).unwrap().token_fd, Some(5));
        let two = [
            &base[..],
            &["--allow-all", "--token-fd", "5", "--token-file", "/t"],
        ]
        .concat();
        assert_eq!(parse(&args(&two)), Err(UsageError::Arguments));
        for error in [
            UsageError::Arguments,
            UsageError::NoPolicy,
            UsageError::PolicyConflict,
        ] {
            for lang in [
                ultar_i18n::Lang::Ar,
                ultar_i18n::Lang::En,
                ultar_i18n::Lang::De,
            ] {
                let text = Messages::new(lang).t(error.key());
                assert_ne!(text, error.key(), "{lang:?}");
            }
        }
    }
}
