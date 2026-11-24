//! The services under supervision: start in dependency order (a service
//! with `notify` counts as started only once it reported `READY`, within
//! its deadline), restart with back-off up to the crash-loop limit,
//! requests (start, stop, restart, status) and the ordered shutdown.

use crate::order::{order, Order};
use crate::output::{Forwarder, INIT_NAME};
use crate::spec::ServiceSpec;
use crate::start::{start, Environment};
use crate::unit::{PipeNews, Unit, UnitState};
use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;
use std::collections::{BTreeMap, VecDeque};
use std::os::fd::{AsFd, BorrowedFd};
use std::time::{Duration, Instant};
#[cfg(test)]
use ultar_bus::messages::init::ServiceState;
use ultar_bus::messages::init::ServiceStatus;
use ultar_service::log::output::Stream;
use ultar_service::ReadyEvent;

/// Time a process gets to disappear after the hard end before init stops
/// waiting for it.
const REAP_GRACE: Duration = Duration::from_secs(2);
/// Longest wait for the log service to take the waiting lines before the
/// last services stop.
const FLUSH_WAIT: Duration = Duration::from_secs(1);

/// A pipe init watches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Watch {
    /// The readiness pipe of a service.
    Ready(String),
    /// Output pipe number `n` of a service.
    Output(String, usize),
}

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlError {
    /// No such service.
    Unknown,
    /// Already in the requested state.
    Already,
    /// The system is shutting down.
    ShuttingDown,
}

#[derive(Debug, Default)]
struct Shutdown {
    waves: VecDeque<Vec<String>>,
    current: Vec<String>,
    flush_until: Option<Instant>,
}

/// All services.
pub struct Supervisor {
    units: BTreeMap<String, Unit>,
    order: Order,
    env: Environment,
    /// Output on its way to the log service.
    pub forwarder: Forwarder,
    journal: Box<dyn FnMut(&str)>,
    shutdown: Option<Shutdown>,
}

/// Signals the process group of the service whose (unreaped) process is
/// `pid`; the process itself if there is no such group yet (the launcher
/// starts the session; before it did, only the process exists). Never
/// both: a second stop signal means "now" to the service runtime.
fn signal_group(pid: u32, signal: Signal) {
    let Ok(raw) = i32::try_from(pid) else {
        return;
    };
    if kill(Pid::from_raw(-raw), signal).is_err() {
        let _ = kill(Pid::from_raw(raw), signal);
    }
}

/// Signals what is left of the process group of a service whose process
/// `pid` was reaped already. Only the group: the number of a reaped
/// process may belong to an unrelated process by now, but it cannot be
/// reused while a group of that number exists.
fn signal_leftovers(pid: u32, signal: Signal) {
    if let Ok(raw) = i32::try_from(pid) {
        let _ = kill(Pid::from_raw(-raw), signal);
    }
}

impl Supervisor {
    /// Supervises `specs`; `journal` receives init's own lines.
    pub fn new(
        specs: Vec<ServiceSpec>,
        env: Environment,
        forwarder: Forwarder,
        journal: Box<dyn FnMut(&str)>,
    ) -> Self {
        let mut supervisor = Self {
            units: BTreeMap::new(),
            order: Order::default(),
            env,
            forwarder,
            journal,
            shutdown: None,
        };
        supervisor.add(specs);
        supervisor
    }

    /// Adds services not known yet (service files of a newly installed
    /// app); returns how many.
    pub fn add(&mut self, specs: Vec<ServiceSpec>) -> usize {
        let mut added = 0;
        for spec in specs {
            if !self.units.contains_key(&spec.name) {
                self.units.insert(spec.name.clone(), Unit::new(spec));
                added += 1;
            }
        }
        let graph = self
            .units
            .iter()
            .map(|(name, unit)| (name.clone(), unit.spec.life.after.clone()))
            .collect();
        self.order = order(&graph);
        for name in self.order.cyclic.clone() {
            if let Some(unit) = self.units.get_mut(&name) {
                if !unit.is_live() {
                    unit.state = UnitState::Failed;
                    unit.reason = Some("dependency_cycle".into());
                }
            }
            self.note(&format!("refused {name} reason=dependency_cycle"));
        }
        added
    }

    /// One line of init's own: to the journal and the log service.
    pub fn note(&mut self, line: &str) {
        (self.journal)(line);
        self.forwarder.push(INIT_NAME, Stream::Err, line);
    }

    fn deps_satisfied(&self, unit: &Unit) -> bool {
        unit.spec
            .life
            .after
            .iter()
            .all(|dep| self.units.get(dep).is_none_or(Unit::satisfies_dependants))
    }

    fn launch(&mut self, name: &str, now: Instant) {
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        unit.note_start();
        match start(&unit.spec, &self.env, now) {
            Ok(run) => {
                let line = match (&unit.spec.life.description, unit.spec.life.oneshot) {
                    (Some(what), true) => {
                        format!("start {name} pid={} one-time step: {what}", run.pid)
                    }
                    _ => format!("start {name} pid={}", run.pid),
                };
                unit.state = if unit.spec.life.notify {
                    UnitState::Starting {
                        deadline: now + Duration::from_millis(unit.spec.life.start_timeout_ms),
                    }
                } else {
                    UnitState::Running
                };
                unit.run = Some(run);
                self.note(&line);
            }
            Err(error) => {
                let permanent = matches!(error.word, "identity_needs_root" | "rights_all_refused");
                let pause = if permanent {
                    None
                } else {
                    unit.note_failure(Duration::ZERO, now)
                };
                unit.reason = Some(error.word.to_owned());
                unit.state = pause.map_or(UnitState::Failed, |pause| UnitState::Backoff {
                    until: now + pause,
                });
                let line = format!(
                    "refused {name} reason={} detail={}",
                    error.word, error.detail
                );
                self.note(&line);
            }
        }
    }

    /// Starts what may start, enforces deadlines, moves the shutdown on and
    /// forwards output.
    pub fn tick(&mut self, now: Instant) {
        if self.shutdown.is_none() {
            let due: Vec<String> = self
                .units
                .iter()
                .filter(|(_, unit)| match unit.state {
                    UnitState::Waiting => self.deps_satisfied(unit),
                    UnitState::Backoff { until } => until <= now,
                    _ => false,
                })
                .map(|(name, _)| name.clone())
                .collect();
            for name in due {
                self.launch(&name, now);
            }
        }
        self.enforce_deadlines(now);
        self.advance_shutdown(now);
        self.forwarder.pump(now);
    }

    fn enforce_deadlines(&mut self, now: Instant) {
        let mut lines = Vec::new();
        for (name, unit) in &mut self.units {
            let Some(pid) = unit.run.as_ref().map(|run| run.pid) else {
                continue;
            };
            match unit.state {
                UnitState::Starting { deadline } if deadline <= now => {
                    signal_group(pid, Signal::SIGKILL);
                    unit.reason = Some("ready_timeout".into());
                    unit.failing = true;
                    unit.state = UnitState::Stopping {
                        deadline: now + REAP_GRACE,
                        killed: true,
                    };
                    lines.push(format!("refused {name} reason=ready_timeout"));
                }
                UnitState::Stopping {
                    deadline,
                    killed: false,
                } if deadline <= now => {
                    signal_group(pid, Signal::SIGKILL);
                    if let Some(group) = unit.run.as_ref().and_then(|run| run.group.as_ref()) {
                        crate::groups::kill_all(group);
                    }
                    unit.state = UnitState::Stopping {
                        deadline: now + REAP_GRACE,
                        killed: true,
                    };
                    lines.push(format!("kill {name} reason=stop_timeout"));
                }
                _ => {}
            }
        }
        for line in lines {
            self.note(&line);
        }
    }

    /// The earliest moment `tick` has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        let units = self.units.values().filter_map(|unit| match unit.state {
            UnitState::Starting { deadline } | UnitState::Stopping { deadline, .. } => {
                Some(deadline)
            }
            UnitState::Backoff { until } => Some(until),
            _ => None,
        });
        let flush = self
            .shutdown
            .as_ref()
            .and_then(|shutdown| shutdown.flush_until);
        units.chain(flush).min()
    }

    /// The pipes to watch, with their descriptors.
    pub fn watched(&self) -> Vec<(Watch, BorrowedFd<'_>)> {
        let mut out = Vec::new();
        for (name, unit) in &self.units {
            let Some(run) = &unit.run else { continue };
            if let Some(ready) = &run.ready {
                out.push((Watch::Ready(name.clone()), ready.reader().as_fd()));
            }
            for (index, pipe) in run.outputs.iter().enumerate() {
                out.push((Watch::Output(name.clone(), index), pipe.reader().as_fd()));
            }
        }
        out
    }

    /// Reads the pipe `watch` (reported readable).
    pub fn on_readable(&mut self, watch: &Watch, now: Instant) {
        match watch {
            Watch::Ready(name) => self.read_ready(name, now),
            Watch::Output(name, index) => self.read_output(name, *index),
        }
    }

    fn read_ready(&mut self, name: &str, _now: Instant) {
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        let Some(run) = unit.run.as_mut() else { return };
        let Some(pipe) = run.ready.as_mut() else {
            return;
        };
        let news = pipe.read();
        let ready = matches!(&news, PipeNews::Ready(events)
            if events.iter().any(|e| matches!(e, ReadyEvent::Ready { .. })));
        if news == PipeNews::Closed {
            run.ready = None;
        }
        if ready && matches!(unit.state, UnitState::Starting { .. }) {
            unit.state = UnitState::Running;
            self.note(&format!("ready {name}"));
        } else if news == PipeNews::Closed && matches!(unit.state, UnitState::Starting { .. }) {
            // Gave up before it was ready: treated like a failed start.
            if let Some(pid) = unit.run.as_ref().map(|run| run.pid) {
                signal_group(pid, Signal::SIGKILL);
            }
            unit.failing = true;
            unit.reason = Some("not_ready".into());
        }
    }

    fn read_output(&mut self, name: &str, index: usize) {
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        let Some(run) = unit.run.as_mut() else { return };
        let Some(pipe) = run.outputs.get_mut(index) else {
            return;
        };
        let stream = pipe.stream;
        match pipe.read() {
            PipeNews::Lines(lines) => {
                for line in lines {
                    self.forwarder.push(name, stream, &line);
                }
            }
            PipeNews::Closed => {
                run.outputs.remove(index);
            }
            PipeNews::Ready(_) => {}
        }
    }
}

#[path = "supervisor_control.rs"]
mod control;
#[path = "supervisor_exit.rs"]
mod exit;

pub use control::{exit_text, status_line};

#[cfg(test)]
#[path = "supervisor_tests.rs"]
mod tests;

/// Things the other parts of the supervisor need from each other.
impl Supervisor {
    fn unit_names(&self) -> Vec<String> {
        self.units.keys().cloned().collect()
    }

    /// Whether a shutdown runs.
    pub fn is_shutting_down(&self) -> bool {
        self.shutdown.is_some()
    }

    /// Whether every automatic service reached a stable state (nothing
    /// waits, starts or backs off).
    pub fn settled(&self) -> bool {
        self.units.values().all(|unit| {
            !matches!(
                unit.state,
                UnitState::Waiting | UnitState::Starting { .. } | UnitState::Backoff { .. }
            )
        })
    }

    /// Services whose process runs.
    pub fn live_count(&self) -> usize {
        self.units.values().filter(|unit| unit.is_live()).count()
    }

    /// What the bus reports for every service.
    pub fn status(&self) -> Vec<ServiceStatus> {
        let own = ultar_sys::euid();
        self.units
            .iter()
            .map(|(name, unit)| ServiceStatus {
                name: name.clone(),
                state: unit.state.public(),
                pid: unit.run.as_ref().map_or(0, |run| run.pid),
                uid: unit.spec.confinement.uid.unwrap_or(own),
                restarts: unit.restarts,
                last_exit: unit.last_exit,
            })
            .collect()
    }

    /// State of one service.
    #[cfg(test)]
    fn state_of(&self, name: &str) -> Option<ServiceState> {
        self.units.get(name).map(|unit| unit.state.public())
    }
}
