//! The main loop of init: signals, children, pipes, requests – one thread
//! that sleeps in `poll` until something happens or a deadline is due.
//!
//! PID 1 duties: every child is reaped (also orphans init inherits), the
//! loop never ends on its own, and stop signals start the ordered
//! shutdown: `SIGTERM`/`SIGINT` → restart (as PID 1; exit otherwise),
//! `SIGUSR2`/`SIGPWR` → power off. `SIGHUP` reads the bus policy and the
//! service directories again (new services only).

use crate::control::{answer, Admins, BusLink};
use crate::ctl::Ctl;
use crate::spec::load_dir;
use crate::supervisor::Supervisor;
use nix::sys::signal::{SigSet, Signal};
use nix::sys::signalfd::{SfdFlags, SignalFd};
use nix::sys::wait::{waitpid, WaitPidFlag, WaitStatus};
use nix::unistd::Pid;
use std::os::fd::{AsFd, BorrowedFd};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::Exit;

/// Longest sleep: a lost `SIGCHLD` delays a reap by at most this.
const MAX_IDLE: Duration = Duration::from_secs(1);
/// Sleep while output waits for a slow log service.
const PUMP_IDLE: Duration = Duration::from_millis(50);

/// How init ends.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Halt {
    /// Power off (PID 1).
    PowerOff,
    /// Restart (PID 1).
    Reboot,
    /// End the process (not PID 1).
    Exit,
}

/// Where init finds and puts things.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Directories of service files.
    pub services: Vec<PathBuf>,
    /// Bus socket of init (`None`: no bus service).
    pub bus_socket: Option<PathBuf>,
    /// Bus policy (administrators of `init`).
    pub bus_policy: PathBuf,
    /// Legacy text control socket (`ultar-ctl`).
    pub control_socket: Option<PathBuf>,
}

/// The signals init handles; blocked before any thread exists.
pub fn block_signals() -> nix::Result<SignalFd> {
    let mut mask = SigSet::empty();
    for signal in [
        Signal::SIGCHLD,
        Signal::SIGTERM,
        Signal::SIGINT,
        Signal::SIGUSR1,
        Signal::SIGUSR2,
        Signal::SIGPWR,
        Signal::SIGHUP,
    ] {
        mask.add(signal);
    }
    mask.thread_block()?;
    SignalFd::with_flags(&mask, SfdFlags::SFD_NONBLOCK | SfdFlags::SFD_CLOEXEC)
}

/// Reaps every child that ended.
pub fn reap(supervisor: &mut Supervisor, now: Instant) {
    loop {
        let (pid, exit) = match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
            Ok(WaitStatus::Exited(pid, code)) => (pid, Exit::Code(code)),
            Ok(WaitStatus::Signaled(pid, signal, _)) => (pid, Exit::Signal(signal as i32)),
            Ok(WaitStatus::StillAlive) | Err(_) => return,
            Ok(_) => continue,
        };
        if let Ok(pid) = u32::try_from(pid.as_raw()) {
            supervisor.on_exit(pid, exit, now);
        }
    }
}

/// Everything the loop owns.
pub struct Daemon {
    /// The services.
    pub supervisor: Supervisor,
    signals: SignalFd,
    bus: Option<BusLink>,
    admins: Admins,
    ctl: Option<Ctl>,
    paths: Paths,
    pid1: bool,
    settled: bool,
}

enum Source {
    Signals,
    Bus,
    Ctl,
    Pipe(crate::supervisor::Watch),
}

impl Daemon {
    /// Sets up the loop (signals must be blocked already, see
    /// [`block_signals`]).
    pub fn new(supervisor: Supervisor, signals: SignalFd, paths: Paths, pid1: bool) -> Self {
        let mut supervisor = supervisor;
        let bus = paths.bus_socket.clone().and_then(|socket| {
            BusLink::start(socket)
                .map_err(|e| supervisor.note(&format!("bus unavailable detail={e}")))
                .ok()
        });
        let ctl = paths.control_socket.as_ref().and_then(|path| {
            Ctl::bind(path)
                .map_err(|e| supervisor.note(&format!("control socket unavailable detail={e}")))
                .ok()
        });
        Self {
            admins: Admins::load(&paths.bus_policy),
            supervisor,
            signals,
            bus,
            ctl,
            paths,
            pid1,
            settled: false,
        }
    }

    fn sleep_for(&self, now: Instant) -> Duration {
        let mut sleep = MAX_IDLE;
        if let Some(deadline) = self.supervisor.next_deadline() {
            sleep = sleep.min(
                deadline
                    .saturating_duration_since(now)
                    .max(Duration::from_millis(1)),
            );
        }
        if self.supervisor.forwarder.waiting() > 0 && self.supervisor.forwarder.connected() {
            sleep = sleep.min(PUMP_IDLE);
        }
        sleep
    }

    /// Waits for the next event; returns the readable sources.
    fn wait(&self, now: Instant) -> Vec<Source> {
        let mut fds: Vec<BorrowedFd<'_>> = vec![self.signals.as_fd()];
        let mut sources = vec![Source::Signals];
        if let Some(bus) = &self.bus {
            fds.push(bus.wake_fd().as_fd());
            sources.push(Source::Bus);
        }
        if let Some(ctl) = &self.ctl {
            fds.push(ctl.fd());
            sources.push(Source::Ctl);
        }
        for (watch, fd) in self.supervisor.watched() {
            fds.push(fd);
            sources.push(Source::Pipe(watch));
        }
        let ready = ultar_sys::poll_readable(&fds, Some(self.sleep_for(now))).unwrap_or_default();
        sources
            .into_iter()
            .zip(ready)
            .filter_map(|(source, readable)| readable.then_some(source))
            .collect()
    }

    fn on_signal(&mut self, halt: &mut Option<Halt>) {
        while let Ok(Some(info)) = self.signals.read_signal() {
            match Signal::try_from(info.ssi_signo as i32) {
                Ok(Signal::SIGUSR2 | Signal::SIGPWR) if self.pid1 => *halt = Some(Halt::PowerOff),
                Ok(Signal::SIGTERM | Signal::SIGINT) => {
                    *halt = Some(if self.pid1 { Halt::Reboot } else { Halt::Exit });
                }
                Ok(Signal::SIGHUP) => self.reload(),
                _ => {}
            }
        }
    }

    /// `SIGHUP`: the bus policy again, and new service files.
    pub fn reload(&mut self) {
        self.admins = Admins::load(&self.paths.bus_policy);
        let (specs, refused) = load_dir(&self.paths.services);
        for (path, error) in refused {
            self.supervisor
                .note(&format!("refused file={} reason={error}", path.display()));
        }
        let added = self.supervisor.add(specs);
        self.supervisor.note(&format!("reload added={added}"));
    }

    fn report_settled(&mut self) {
        if !self.settled && self.supervisor.settled() {
            self.settled = true;
            self.supervisor.note("settled");
        }
    }

    /// Runs until a stop signal or request, the shutdown included.
    pub fn run(
        &mut self,
        mut extra: impl FnMut(&str, &str) -> Option<(String, Option<Halt>)>,
    ) -> Halt {
        let mut halt: Option<Halt> = None;
        loop {
            let now = Instant::now();
            self.supervisor.tick(now);
            self.report_settled();
            if let Some(halt) = halt {
                self.supervisor.begin_shutdown(now);
                if self.supervisor.shutdown_done() {
                    self.finish();
                    return halt;
                }
            }
            for source in self.wait(now) {
                let now = Instant::now();
                match source {
                    Source::Signals => self.on_signal(&mut halt),
                    Source::Bus => self.on_bus(&mut halt, now),
                    Source::Ctl => self.on_ctl(&mut halt, &mut extra, now),
                    Source::Pipe(watch) => self.supervisor.on_readable(&watch, now),
                }
            }
            reap(&mut self.supervisor, Instant::now());
        }
    }

    fn on_bus(&mut self, halt: &mut Option<Halt>, now: Instant) {
        let Some(bus) = &mut self.bus else { return };
        for request in bus.take() {
            if let Some(requested) =
                answer(request, &mut self.supervisor, &self.admins, now, self.pid1)
            {
                *halt = Some(requested);
            }
        }
    }

    fn on_ctl(
        &mut self,
        halt: &mut Option<Halt>,
        extra: &mut impl FnMut(&str, &str) -> Option<(String, Option<Halt>)>,
        now: Instant,
    ) {
        let Some(ctl) = &self.ctl else { return };
        for stream in ctl.accept_all() {
            let outcome = crate::ctl::handle(stream, &mut self.supervisor, now, extra, self.pid1);
            if let Some(requested) = outcome.halt {
                *halt = Some(requested);
            }
            if outcome.reload {
                self.reload();
            }
        }
    }

    fn finish(&mut self) {
        if let Some(bus) = self.bus.take() {
            bus.stop();
        }
        let deadline = Instant::now() + Duration::from_millis(500);
        while self.supervisor.forwarder.pump(Instant::now()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.supervisor.note("shutdown done");
    }
}
