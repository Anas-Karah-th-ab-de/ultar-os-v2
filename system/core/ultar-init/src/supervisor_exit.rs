//! Ends of processes, and the ordered shutdown.

use super::{signal_group, signal_leftovers, Shutdown, Supervisor, FLUSH_WAIT};
use crate::launch::LaunchFailure;
use crate::spec::Restart;
use crate::unit::{PipeNews, Run, UnitState};
use nix::sys::signal::Signal;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::Exit;

/// Most reads of a dead service's output pipes (its output is read up to
/// what is there; a grandchild that keeps the pipe open cannot hold init).
const DRAIN_READS: usize = 64;

fn exit_word(exit: Exit) -> String {
    match exit {
        Exit::Code(code) => match LaunchFailure::from_code(code) {
            Some(failure) => format!("code={code} launch={}", failure.word()),
            None => format!("code={code}"),
        },
        Exit::Signal(signal) => format!("signal={signal}"),
    }
}

impl Supervisor {
    /// Reads what the pipes of `run` still hold, without waiting.
    fn drain(&mut self, name: &str, run: &mut Run) {
        for _ in 0..DRAIN_READS {
            let fds: Vec<_> = run
                .outputs
                .iter()
                .map(|pipe| pipe.reader().as_fd())
                .collect();
            let Ok(ready) = ultar_sys::poll_readable(&fds, Some(Duration::ZERO)) else {
                return;
            };
            let Some(index) = ready.iter().position(|readable| *readable) else {
                return;
            };
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
                _ => {
                    run.outputs.remove(index);
                }
            }
        }
    }

    /// A process ended (from `waitpid`). Processes that are not a
    /// service's (orphans that init inherited as PID 1) are ignored.
    pub fn on_exit(&mut self, pid: u32, exit: Exit, now: Instant) {
        let Some(name) = self
            .units
            .iter()
            .find(|(_, unit)| unit.run.as_ref().is_some_and(|run| run.pid == pid))
            .map(|(name, _)| name.clone())
        else {
            return;
        };
        let Some(mut run) = self.units.get_mut(&name).and_then(|unit| unit.run.take()) else {
            return;
        };
        self.drain(&name, &mut run);
        // Whatever the service left behind in its group or session ends too.
        signal_leftovers(pid, Signal::SIGKILL);
        if let Some(group) = &run.group {
            crate::groups::kill_all(group);
            crate::groups::remove(group);
        }
        let ran = now.saturating_duration_since(run.since);
        self.note(&format!(
            "exit {name} pid={pid} {} ran_ms={}",
            exit_word(exit),
            ran.as_millis()
        ));
        self.decide(&name, exit, ran, now);
    }

    /// The next state after an end.
    fn decide(&mut self, name: &str, exit: Exit, ran: Duration, now: Instant) {
        let shutting_down = self.shutdown.is_some();
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        unit.last_exit = Some(exit);
        let ok = exit == Exit::Code(0);
        let failing = std::mem::take(&mut unit.failing);
        let requested = matches!(unit.state, UnitState::Stopping { .. }) && !failing;
        let restart = match unit.spec.life.restart {
            Restart::Always => true,
            Restart::OnFailure => !ok || failing,
            Restart::Never => false,
        };
        unit.state = if shutting_down {
            UnitState::Stopped
        } else if requested {
            if std::mem::take(&mut unit.start_after_stop) {
                UnitState::Waiting
            } else {
                UnitState::Stopped
            }
        } else if unit.spec.life.oneshot && !failing {
            UnitState::Done { ok }
        } else if !restart {
            UnitState::Done { ok: ok && !failing }
        } else if ok && !failing && ran >= crate::unit::STABLE_AFTER {
            UnitState::Waiting
        } else {
            match unit.note_failure(ran, now) {
                Some(pause) => UnitState::Backoff { until: now + pause },
                None => {
                    unit.reason = Some("crash_loop".into());
                    UnitState::Failed
                }
            }
        };
        if unit.state == UnitState::Failed {
            let line = format!("failed {name} reason=crash_loop");
            self.note(&line);
        }
    }

    /// Begins the shutdown: no more starts; services stop in waves, the
    /// deepest first, the log service last.
    pub fn begin_shutdown(&mut self, now: Instant) {
        if self.shutdown.is_some() {
            return;
        }
        let mut waves: std::collections::VecDeque<Vec<String>> = self.order.stop_waves().into();
        // Services outside the order (cycles) never run; stop them first
        // should one have been started by hand.
        let ordered: Vec<String> = waves.iter().flatten().cloned().collect();
        let rest: Vec<String> = self
            .unit_names()
            .into_iter()
            .filter(|name| !ordered.contains(name))
            .collect();
        if !rest.is_empty() {
            waves.push_front(rest);
        }
        self.shutdown = Some(Shutdown {
            waves,
            ..Shutdown::default()
        });
        self.note("shutdown begin");
        self.advance_shutdown(now);
    }

    fn wave_done(&self, wave: &[String], now: Instant) -> bool {
        wave.iter().all(|name| {
            self.units.get(name).is_none_or(|unit| match unit.state {
                // A process that ignored even the hard end is given up on.
                UnitState::Stopping {
                    deadline,
                    killed: true,
                } => deadline <= now || !unit.is_live(),
                _ => !unit.is_live(),
            })
        })
    }

    /// Moves the shutdown on: when a wave is done, the next one gets its
    /// stop signal. Before the last wave (the log service) the waiting
    /// lines get [`FLUSH_WAIT`] at most to reach it.
    pub(super) fn advance_shutdown(&mut self, now: Instant) {
        loop {
            let Some(shutdown) = &self.shutdown else {
                return;
            };
            if !self.wave_done(&shutdown.current, now) {
                return;
            }
            let Some(next) = shutdown.waves.front().cloned() else {
                return;
            };
            let last = shutdown.waves.len() == 1;
            if last && self.forwarder.waiting() > 0 && self.forwarder.connected() {
                let Some(shutdown) = self.shutdown.as_mut() else {
                    return;
                };
                let until = *shutdown.flush_until.get_or_insert(now + FLUSH_WAIT);
                if now < until {
                    return;
                }
            }
            if let Some(shutdown) = self.shutdown.as_mut() {
                shutdown.waves.pop_front();
                shutdown.flush_until = None;
                shutdown.current = next.clone();
            }
            for name in next {
                self.stop_unit(&name, now);
            }
        }
    }

    /// Sends the stop signal (or settles a unit without process).
    pub(super) fn stop_unit(&mut self, name: &str, now: Instant) {
        let default_stop = self.env.stop_timeout;
        let Some(unit) = self.units.get_mut(name) else {
            return;
        };
        match (&unit.run, unit.state) {
            (Some(_), UnitState::Stopping { .. }) => {}
            (Some(run), _) => {
                signal_group(run.pid, Signal::SIGTERM);
                let timeout = unit
                    .spec
                    .life
                    .stop_timeout_ms
                    .map_or(default_stop, Duration::from_millis);
                unit.state = UnitState::Stopping {
                    deadline: now + timeout,
                    killed: false,
                };
            }
            (None, UnitState::Failed | UnitState::Done { .. }) => {}
            (None, _) => unit.state = UnitState::Stopped,
        }
    }

    /// Whether every wave is done.
    pub fn shutdown_done(&self) -> bool {
        self.shutdown
            .as_ref()
            .is_some_and(|shutdown| shutdown.waves.is_empty())
            && self
                .shutdown
                .as_ref()
                .is_some_and(|shutdown| self.wave_done(&shutdown.current, Instant::now()))
    }
}
