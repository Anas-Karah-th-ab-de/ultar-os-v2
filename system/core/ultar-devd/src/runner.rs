//! Command line and main flow of the `ultar-devd` binary.

use crate::bus::MemoryPublisher;
use crate::cli::{print_list, EXIT_FAILED, EXIT_OK, EXIT_USAGE};
use crate::daemon::{Daemon, DaemonConfig};
use crate::msg::Messages;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::device_service::DevicePhase;
use ultar_bus::{Client, ClientOptions};
use ultar_i18n::Arg;

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
}

/// The command line is not understood.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageError;

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<DaemonArgs, UsageError> {
    let mut paths: [Option<PathBuf>; 5] = Default::default();
    let (mut kernel_abi, mut lang, mut once) = (None, None, false);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let slot = match arg.as_str() {
            "--depot" => 0,
            "--keys" => 1,
            "--state" => 2,
            "--bus" => 3,
            "--root" => 4,
            "--once" => {
                once = true;
                continue;
            }
            "--kernel-abi" => {
                kernel_abi = Some(iter.next().ok_or(UsageError)?.clone());
                continue;
            }
            "--lang" => {
                lang = Some(iter.next().ok_or(UsageError)?.clone());
                continue;
            }
            _ => return Err(UsageError),
        };
        paths[slot] = Some(PathBuf::from(iter.next().ok_or(UsageError)?));
    }
    let [depot, keys, state, bus, root] = paths;
    if bus.is_none() && !once {
        return Err(UsageError);
    }
    Ok(DaemonArgs {
        config: DaemonConfig {
            root: root.unwrap_or_else(|| PathBuf::from("/")),
            depot: depot.ok_or(UsageError)?,
            keys: keys.ok_or(UsageError)?,
            state: state.ok_or(UsageError)?,
            kernel_abi,
        },
        bus,
        once,
        lang,
    })
}

fn log(err: &mut dyn Write, text: &str) {
    let _ = writeln!(err, "ultar-devd: {text}");
}

fn started(daemon: &Daemon, messages: &Messages, err: &mut dyn Write) {
    let devices = daemon.list().devices;
    let active = devices
        .iter()
        .filter(|d| d.phase == DevicePhase::Active)
        .count();
    let count = |n: usize| Arg::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let args = [("devices", count(devices.len())), ("active", count(active))];
    log(err, &messages.t_args("devd.daemon.started", &args));
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

/// The whole daemon: parse, connect, start, serve. Returns the exit code.
pub fn run(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Ok(parsed) = parse(args) else {
        let messages = Messages::new(Messages::choose_lang(None));
        let _ = writeln!(err, "{}", messages.t("devd.cli.daemon-usage"));
        return EXIT_USAGE;
    };
    let messages = Messages::new(Messages::choose_lang(parsed.lang.as_deref()));
    let Some(socket) = parsed.bus.clone().filter(|_| !parsed.once) else {
        return run_once(parsed, &messages, out, err);
    };
    let bus_lost = |error: &ultar_bus::BusError, err: &mut dyn Write| {
        let detail = error.to_string();
        log(
            err,
            &messages.t_args("devd.daemon.bus-lost", &[("a", Arg::Str(&detail))]),
        );
        EXIT_FAILED
    };
    let options = ClientOptions {
        connect_retry_for: BUS_WAIT,
        ..ClientOptions::named("ultar-devd")
    };
    let bus = match Client::connect(&socket, options) {
        Ok(bus) => bus,
        Err(error) => return bus_lost(&error, err),
    };
    // Register first: a client that reacts to the retained capability message can call
    // the service at once; its request waits in the endpoint until serving starts.
    let endpoint = match Daemon::register(&bus) {
        Ok(endpoint) => endpoint,
        Err(error) => return bus_lost(&error, err),
    };
    let mut daemon = match Daemon::start(parsed.config, Box::new(bus.clone())) {
        Ok(daemon) => daemon,
        Err(error) => return start_failed(&error, &messages, err),
    };
    started(&daemon, &messages, err);
    match daemon.serve(&endpoint) {
        Ok(()) => {
            log(err, &messages.t("devd.daemon.stopped"));
            EXIT_OK
        }
        Err(error) => bus_lost(&error, err),
    }
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
        ]))
        .unwrap();
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
            &[],
        ] {
            assert_eq!(parse(&args(bad)), Err(UsageError), "{bad:?}");
        }
        let (mut out, mut err) = (Vec::new(), Vec::new());
        assert_eq!(run(&args(&["--what"]), &mut out, &mut err), EXIT_USAGE);
        assert!(!err.is_empty());
    }
}
