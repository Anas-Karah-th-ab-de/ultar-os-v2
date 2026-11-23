//! Ties the parts together: what every daemon's `main` calls.
//!
//! ```no_run
//! use ultar_service::{exit, Runtime, ServiceSpec};
//!
//! fn main() -> std::process::ExitCode {
//!     let runtime = match Runtime::start(ServiceSpec::new("example")) {
//!         Ok(runtime) => runtime,
//!         Err(error) => return exit::code(error.exit_code()),
//!     };
//!     exit::code(runtime.run(|runtime| {
//!         // open files and sockets, connect to the bus, …
//!         runtime.ready();
//!         let _cause = runtime.lifecycle().wait_stop();
//!         // flush, close, remove sockets …
//!         exit::OK
//!     }))
//! }
//! ```
//!
//! [`Runtime::start`] runs on the main thread before any other thread
//! exists: hardening (close descriptors, umask, working directory, root
//! check, limits, rights) → block the signals and open the signal
//! descriptor → adopt the readiness descriptor → start the logger →
//! install the panic hook.
//!
//! [`Runtime::run`] then starts the service's work on a second thread and
//! keeps the **main thread** for the signals (see [`crate::signal`] for
//! why it has to be the main thread). When the work returns, the runtime
//! reports `STOPPED`, flushes the last log records and returns the exit
//! code.

use crate::exit;
use crate::harden::{Hardening, HardeningError, HardeningReport, Outcome};
use crate::lifecycle::{Lifecycle, State};
use crate::log::{Level, LogTarget, Logger, LoggerConfig, Record};
use crate::ready::{ReadyEvent, ReadyNotifier};
use crate::signal::{self, MainSource, SignalConfig, SignalError, Waker};
use std::fmt;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

/// Time a service has to stop after `SIGTERM` unless it says otherwise.
/// Shorter than the 3 s after which `ultar-init` kills the group.
pub const DEFAULT_STOP_DEADLINE: Duration = Duration::from_millis(2500);
/// Longest time the last log records may take to reach the log service at
/// exit or in a panic.
pub const FINAL_FLUSH: Duration = Duration::from_millis(500);

/// What a service declares about itself.
#[derive(Clone, Debug)]
pub struct ServiceSpec {
    /// Short name for diagnostics (not an identity).
    pub name: &'static str,
    /// Self-limits; everything on by default.
    pub hardening: Hardening,
    /// Time from the stop signal to state `Stopped`.
    pub stop_deadline: Duration,
    /// Logger settings.
    pub log: LoggerConfig,
}

impl ServiceSpec {
    /// A service with default hardening, default deadline and no log
    /// target (records are discarded until a target is set).
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            hardening: Hardening::default(),
            stop_deadline: DEFAULT_STOP_DEADLINE,
            log: LoggerConfig::to(LogTarget::Discard),
        }
    }
}

/// Why the runtime could not start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StartError {
    /// A hardening step failed.
    Hardening(HardeningError),
    /// Signal handling could not be set up.
    Signals(SignalError),
}

impl StartError {
    /// The exit code a service returns for this error.
    pub const fn exit_code(&self) -> i32 {
        exit::HARDENING
    }

    /// Stable key for the operator message.
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Hardening(HardeningError::RunningAsRoot) => "service.start.root_refused",
            Self::Hardening(_) => "service.start.hardening_failed",
            Self::Signals(_) => "service.start.signals_failed",
        }
    }
}

impl fmt::Display for StartError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Hardening(error) => write!(f, "{}: {error}", self.key()),
            Self::Signals(error) => write!(f, "{}: {error}", self.key()),
        }
    }
}

impl std::error::Error for StartError {}

/// The running service runtime.
pub struct Runtime {
    name: &'static str,
    lifecycle: Lifecycle,
    logger: Logger,
    ready: Arc<ReadyNotifier>,
    report: HardeningReport,
    started: Instant,
    stop_deadline: Duration,
    /// The signal descriptors, until [`Runtime::run`] takes them.
    signals: Mutex<Option<(MainSource, Waker)>>,
}

fn install_panic_hook(logger: &Logger) {
    let logger = logger.clone();
    std::panic::set_hook(Box::new(move |info| {
        // The panic message may contain data and is not logged; the source
        // position is part of the program.
        let line = info.location().map_or(0, |location| location.line());
        logger.emit(Record::new(Level::Critical, "service.panic").count("line", u64::from(line)));
        logger.flush(FINAL_FLUSH);
        let position = info.location().map_or_else(String::new, |location| {
            format!(" {}:{}", location.file(), location.line())
        });
        let _ = writeln!(std::io::stderr(), "service.panic{position}");
        std::process::exit(exit::PANIC);
    }));
}

impl Runtime {
    /// Hardens the process and starts signal handling, logging and the
    /// panic hook. **Call first thing in `main`** (after reading the
    /// command line, before opening anything or spawning threads).
    pub fn start(mut spec: ServiceSpec) -> Result<Self, StartError> {
        let started = Instant::now();
        if let Some(fd) = ReadyNotifier::env_fd() {
            spec.hardening.keep_descriptors.push(fd);
        }
        let report = spec.hardening.apply().map_err(StartError::Hardening)?;
        let lifecycle = Lifecycle::new();
        // Blocks the signals in this (the main) thread; every thread
        // created from here on inherits the mask.
        let signals = MainSource::open().map_err(StartError::Signals)?;
        let ready = Arc::new(ReadyNotifier::from_env());
        let logger = Logger::start(spec.log);
        install_panic_hook(&logger);
        let runtime = Self {
            name: spec.name,
            lifecycle,
            logger,
            ready,
            report,
            started,
            stop_deadline: spec.stop_deadline,
            signals: Mutex::new(Some(signals)),
        };
        runtime.announce_start();
        Ok(runtime)
    }

    fn announce_start(&self) {
        self.logger.emit(
            Record::new(Level::Info, "service.starting")
                .count("pid", u64::from(std::process::id()))
                .count("closed_fds", self.report.closed_descriptors as u64),
        );
        for step in self.report.unsupported() {
            self.logger.emit(
                Record::new(Level::Warn, "service.hardening.unsupported").word("step", step.key()),
            );
        }
        for (step, outcome) in &self.report.steps {
            if *outcome == Outcome::Disabled {
                self.logger.emit(
                    Record::new(Level::Notice, "service.hardening.disabled")
                        .word("step", step.key()),
                );
            }
        }
        let (ready, logger) = (self.ready.clone(), self.logger.clone());
        let lifecycle = self.lifecycle.clone();
        self.lifecycle.on_stop(move || {
            ready.notify(ReadyEvent::Stopping);
            let cause = lifecycle
                .stop_cause()
                .map_or("unknown", |cause| cause.name());
            logger.emit(Record::new(Level::Info, "service.stopping").word("cause", cause));
        });
        let logger = self.logger.clone();
        self.lifecycle.on_reload(move || {
            logger.log(Level::Info, "service.reload");
        });
    }

    /// Diagnostic name of the service.
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The state machine (stop requests, hooks, waiting).
    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    /// The logger.
    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// What hardening did.
    pub fn hardening(&self) -> &HardeningReport {
        &self.report
    }

    /// Declares the service ready: state `Ready`, `READY` to the
    /// supervisor, a log record with the start-up time.
    pub fn ready(&self) {
        if !self.lifecycle.set_ready() {
            return;
        }
        self.ready.notify(ReadyEvent::Ready {
            pid: std::process::id(),
        });
        let millis = u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.logger
            .emit(Record::new(Level::Info, "service.ready").count("startup_ms", millis));
    }

    /// Runs `work` on a second thread and handles signals on the calling
    /// thread – which must be the **main thread** – until `work` returns.
    /// Then ends the service ([`Self::finish`]) and returns its exit code.
    ///
    /// `work` gets the runtime to declare readiness, to wait for the stop
    /// and to log. A stop signal moves the lifecycle to `Stopping` and
    /// runs the stop hooks; `work` is expected to return soon after (the
    /// deadline of the [`ServiceSpec`] is enforced by ending the process).
    pub fn run(self, work: impl FnOnce(&Runtime) -> i32 + Send) -> i32 {
        let signals = self
            .signals
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        let Some((source, waker)) = signals else {
            return self.finish(exit::FAILURE);
        };
        let config = SignalConfig::new(self.stop_deadline);
        let runtime = &self;
        let code = thread::scope(|scope| {
            let worker = thread::Builder::new()
                .name(format!("{}-work", self.name))
                .spawn_scoped(scope, move || {
                    let code = work(runtime);
                    // Releases the main thread from its wait for signals.
                    waker.wake();
                    code
                });
            match worker {
                Ok(worker) => {
                    signal::watch(source, &runtime.lifecycle, &config);
                    // A panic never gets here: the hook ends the process.
                    worker.join().unwrap_or(exit::PANIC)
                }
                Err(_) => exit::FAILURE,
            }
        });
        self.finish(code)
    }

    /// Ends the service: state `Stopped`, `STOPPED <code>` to the
    /// supervisor, last log records flushed (bounded wait). Returns `code`
    /// for `main` to return. [`Self::run`] calls this itself; call it
    /// directly only when the start is given up before `run`.
    pub fn finish(self, code: i32) -> i32 {
        self.logger
            .emit(Record::new(Level::Info, "service.stopped").signed("code", i64::from(code)));
        self.logger.flush(FINAL_FLUSH);
        self.ready.notify(ReadyEvent::Stopped { code });
        self.lifecycle.mark_stopped();
        debug_assert_eq!(self.lifecycle.state(), State::Stopped);
        code
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harden::Step;
    use ultar_sys::Errno;

    #[test]
    fn spec_defaults_are_strict() {
        let spec = ServiceSpec::new("x");
        assert_eq!(spec.hardening, Hardening::default());
        assert_eq!(spec.stop_deadline, DEFAULT_STOP_DEADLINE);
        assert_eq!(spec.log.target, LogTarget::Discard);
        // Shorter than the 3 s after which `ultar-init` kills the group.
        assert!(spec.stop_deadline < Duration::from_secs(3));
    }

    #[test]
    fn start_errors_have_keys_and_one_exit_code() {
        let errors = [
            StartError::Hardening(HardeningError::RunningAsRoot),
            StartError::Hardening(HardeningError::NotEffective(Step::Umask)),
            StartError::Signals(SignalError::Kernel(Errno::EPERM)),
        ];
        let keys: Vec<&str> = errors.iter().map(StartError::key).collect();
        assert_eq!(
            keys,
            [
                "service.start.root_refused",
                "service.start.hardening_failed",
                "service.start.signals_failed"
            ]
        );
        for error in errors {
            assert_eq!(error.exit_code(), exit::HARDENING);
            assert!(error.to_string().starts_with(error.key()));
        }
    }
}
