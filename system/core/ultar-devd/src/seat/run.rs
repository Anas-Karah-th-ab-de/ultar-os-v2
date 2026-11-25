//! Command line and main loop of `ultar-devd-seat`.
//!
//! `--once` sets the nodes, prints one line per node and the verdict, writes
//! the state file if `--state` is given and exits (no service runtime; for
//! tests and the kernel end-to-end run). Without it the program runs on the
//! service runtime as a service (root with `file_admin` only), sets every node
//! at start, writes the state, reports readiness and then follows the
//! kernel's device events.

use super::apply::{scan, write_state, Applied, DeviceModes, NodeReport};
use super::events::{DeviceEvent, EventSocket};
use super::{apply_node, wanted, SeatConfig};
use crate::cli::{EXIT_FAILED, EXIT_OK, EXIT_USAGE};
use crate::msg::Messages;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_hal_system::seat_nodes;
use ultar_i18n::Arg;
use ultar_service::log::{Level, LogTarget, LoggerConfig, Record};
use ultar_service::{Platform, Right, RightSet, Runtime, ServiceSpec};

/// How often the loop looks at the stop request while no event comes.
const TICK: Duration = Duration::from_millis(500);

/// The parsed command line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SeatArgs {
    /// Configuration file.
    pub config: PathBuf,
    /// Directory of the state file (`public_dirs` of the service).
    pub state: Option<PathBuf>,
    /// Root of the tree (`/`).
    pub root: PathBuf,
    /// Set once, print, exit.
    pub once: bool,
    /// Language option.
    pub lang: Option<String>,
    /// Ingestion socket of the log service.
    pub log_socket: Option<PathBuf>,
}

/// Parses the arguments after the program name; `None` on any mistake.
pub fn parse(args: &[String]) -> Option<SeatArgs> {
    let mut parsed = SeatArgs {
        root: PathBuf::from("/"),
        ..SeatArgs::default()
    };
    let mut config = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--once" => parsed.once = true,
            "--config" => config = Some(PathBuf::from(iter.next()?)),
            "--state" => parsed.state = Some(PathBuf::from(iter.next()?)),
            "--root" => parsed.root = PathBuf::from(iter.next()?),
            "--lang" => parsed.lang = Some(iter.next()?.clone()),
            "--log-socket" => parsed.log_socket = Some(PathBuf::from(iter.next()?)),
            _ => return None,
        }
    }
    parsed.config = config?;
    (parsed.once || parsed.state.is_some()).then_some(parsed)
}

fn line(err: &mut dyn Write, text: &str) {
    let _ = writeln!(err, "ultar-devd-seat: {text}");
}

fn applied_word(applied: &Applied) -> &'static str {
    match applied {
        Applied::Unchanged => "unchanged",
        Applied::Set => "set",
        Applied::KernelRefused => "kernel-refused",
        Applied::NotDevice => "not-a-device",
        Applied::Failed(_) => "failed",
    }
}

/// One line per node: `name reason uid:gid mode outcome`.
pub fn print_reports(out: &mut dyn Write, reports: &[NodeReport]) {
    for r in reports {
        let w = r.wanted;
        let detail = match &r.applied {
            Applied::Failed(text) => format!(" ({text})"),
            _ => String::new(),
        };
        let _ = writeln!(
            out,
            "{} {} {}:{} {:04o} {}{detail}",
            r.devname,
            w.reason.word(),
            w.uid,
            w.gid,
            w.mode,
            applied_word(&r.applied)
        );
    }
}

fn verdict(out: &mut dyn Write, messages: &Messages, modes: DeviceModes, count: usize) {
    let args = [
        ("modes", Arg::Str(modes.word())),
        ("nodes", Arg::Int(i64::try_from(count).unwrap_or(i64::MAX))),
    ];
    let _ = writeln!(out, "{}", messages.t_args("seat.verdict", &args));
}

fn store(
    state: Option<&Path>,
    modes: DeviceModes,
    messages: &Messages,
    err: &mut dyn Write,
) -> bool {
    let Some(dir) = state else { return true };
    match write_state(dir, modes) {
        Ok(()) => true,
        Err(error) => {
            let detail = error.to_string();
            line(
                err,
                &messages.t_args("seat.state-failed", &[("a", Arg::Str(&detail))]),
            );
            false
        }
    }
}

fn run_once(
    args: &SeatArgs,
    config: &SeatConfig,
    messages: &Messages,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let reports = scan(&args.root, config);
    print_reports(out, &reports);
    if let Some(group) = config.torch_group {
        if let Some(line) = super::light::report(&super::light::apply_torch(&args.root, group)) {
            let _ = writeln!(out, "{line}");
        }
    }
    let modes = DeviceModes::of(&reports, Platform::detect() == Platform::Ultar);
    verdict(out, messages, modes, reports.len());
    if !store(args.state.as_deref(), modes, messages, err) || modes == DeviceModes::Failed {
        return EXIT_FAILED;
    }
    EXIT_OK
}

fn start_runtime(
    args: &SeatArgs,
    messages: &Messages,
    err: &mut dyn Write,
) -> Result<Runtime, i32> {
    let mut spec = ServiceSpec::new("ultar-devd-seat");
    // Changing the owner of root's nodes is the one thing this program is for.
    spec.hardening.allow_root = true;
    spec.hardening.rights = Some(RightSet::of(&[Right::FileAdmin]));
    if let Some(path) = &args.log_socket {
        spec.log = LoggerConfig::to(LogTarget::Socket(path.clone()));
    }
    Runtime::start(spec).map_err(|error| {
        let detail = error.to_string();
        line(
            err,
            &messages.t_args("seat.hardening-failed", &[("a", Arg::Str(&detail))]),
        );
        error.exit_code()
    })
}

/// The whole program. Must be called first thing in `main` (the service
/// runtime blocks the stop signals before any other thread exists).
pub fn run(args: &[String], out: &mut dyn Write, err: &mut (dyn Write + Send)) -> i32 {
    let lang = args
        .iter()
        .position(|a| a == "--lang")
        .and_then(|i| args.get(i + 1));
    let messages = Messages::new(Messages::choose_lang(lang.map(String::as_str)));
    let Some(parsed) = parse(args) else {
        let _ = writeln!(err, "{}", messages.t("seat.usage"));
        return EXIT_USAGE;
    };
    let config = match SeatConfig::load(&parsed.config) {
        Ok(config) => config,
        Err(error) => {
            line(
                err,
                &messages.t_args(error.key(), &[("a", Arg::Str(error.detail()))]),
            );
            return EXIT_FAILED;
        }
    };
    if parsed.once {
        return run_once(&parsed, &config, &messages, out, err);
    }
    let runtime = match start_runtime(&parsed, &messages, err) {
        Ok(runtime) => runtime,
        Err(code) => return code,
    };
    let lang = messages.lang();
    runtime.run(move |runtime| {
        let messages = Messages::new(lang);
        serve(&parsed, &config, &messages, runtime, err)
    })
}

/// The service state between events.
struct Seat<'a> {
    args: &'a SeatArgs,
    config: &'a SeatConfig,
    ultar: bool,
    modes: Option<DeviceModes>,
}

impl Seat<'_> {
    /// Records the verdict of `reports` (`full`: a whole scan replaces the
    /// verdict, single events can only make it worse). True if stored.
    fn settle(
        &mut self,
        reports: &[NodeReport],
        full: bool,
        runtime: &Runtime,
        messages: &Messages,
        err: &mut dyn Write,
    ) -> bool {
        let found = DeviceModes::of(reports, self.ultar);
        let next = match self.modes {
            Some(current) if !full && found == DeviceModes::Kept => current,
            Some(DeviceModes::Failed) if !full => DeviceModes::Failed,
            _ => found,
        };
        for r in reports.iter().filter(|r| r.applied != Applied::Unchanged) {
            runtime.logger().emit(
                Record::new(Level::Info, "seat.node")
                    .id_or_invalid("node", &r.devname.replace('/', "-"))
                    .word("reason", r.wanted.reason.word())
                    .word("outcome", applied_word(&r.applied)),
            );
        }
        if self.modes == Some(next) {
            return true;
        }
        self.modes = Some(next);
        runtime
            .logger()
            .emit(Record::new(Level::Info, "seat.modes").word("modes", next.word()));
        verdict(err, messages, next, reports.len());
        store(self.args.state.as_deref(), next, messages, err)
    }

    /// Handles one event: sets the node, then asks the kernel to announce the
    /// node again (`change`) so the compositor, which may have tried it before
    /// it was ready, looks at it once more.
    fn event(&self, event: &DeviceEvent) -> Option<NodeReport> {
        if !event.wants_apply() {
            return None;
        }
        let wanted = wanted(&self.args.root, &event.devname, self.config)?;
        let report = apply_node(&self.args.root, &event.devname, wanted);
        if report.applied == Applied::Set {
            if let Some(name) = event.devname.strip_prefix("input/") {
                let path = seat_nodes::input_uevent(&self.args.root, name);
                let _ = std::fs::write(path, "change");
            }
        }
        Some(report)
    }
}

fn serve(
    args: &SeatArgs,
    config: &SeatConfig,
    messages: &Messages,
    runtime: &Runtime,
    err: &mut dyn Write,
) -> i32 {
    // Subscribe before the first scan, so no node added in between is missed.
    let events = EventSocket::open();
    if let Err(error) = &events {
        let detail = error.to_string();
        line(
            err,
            &messages.t_args("seat.no-events", &[("a", Arg::Str(&detail))]),
        );
    }
    let ultar = Platform::detect() == Platform::Ultar;
    let mut seat = Seat {
        args,
        config,
        ultar,
        modes: None,
    };
    if !seat.settle(&scan(&args.root, config), true, runtime, messages, err) {
        return EXIT_FAILED;
    }
    runtime.ready();
    while !runtime.lifecycle().is_stopping() {
        let Ok(socket) = &events else {
            std::thread::sleep(TICK);
            continue;
        };
        let batch = socket.wait(TICK);
        let (reports, full) = if batch.lost {
            (scan(&args.root, config), true)
        } else {
            (
                batch
                    .events
                    .iter()
                    .filter_map(|e| seat.event(e))
                    .collect::<Vec<_>>(),
                false,
            )
        };
        if !reports.is_empty() || full {
            seat.settle(&reports, full, runtime, messages, err);
        }
    }
    line(err, &messages.t("seat.stopped"));
    EXIT_OK
}
