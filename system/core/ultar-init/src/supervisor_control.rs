//! Requests: start, stop, restart, status.

use super::{ControlError, Supervisor};
use crate::unit::UnitState;
use std::time::Instant;
use ultar_bus::messages::init::{Action, Exit, ServiceState, ServiceStatus};

impl Supervisor {
    /// Carries out `action` for `name`; returns the state right after.
    pub fn control(
        &mut self,
        action: Action,
        name: &str,
        now: Instant,
    ) -> Result<ServiceState, ControlError> {
        if self.shutdown.is_some() {
            return Err(ControlError::ShuttingDown);
        }
        let unit = self.units.get_mut(name).ok_or(ControlError::Unknown)?;
        let live = unit.is_live();
        match (action, live) {
            (Action::Start, true) => return Err(ControlError::Already),
            (Action::Stop, false) if unit.state == UnitState::Stopped => {
                return Err(ControlError::Already);
            }
            (Action::Start, false) | (Action::Restart, false) => {
                unit.reset_failures();
                unit.state = UnitState::Waiting;
            }
            (Action::Stop, false) => unit.state = UnitState::Stopped,
            (Action::Stop, true) | (Action::Restart, true) => {
                unit.start_after_stop = action == Action::Restart;
                if action == Action::Restart {
                    unit.reset_failures();
                }
                self.stop_unit(name, now);
            }
        }
        self.note(&format!("request {} {name}", action.word()));
        self.tick(now);
        self.units
            .get(name)
            .map(|unit| unit.state.public())
            .ok_or(ControlError::Unknown)
    }

    /// The table of the control socket (`ultar-ctl status`): a header and
    /// one line per service – name, state, pid, restarts, uptime.
    pub fn status_text(&self) -> String {
        let mut out = status_line("SERVICE", "STATE", "PID", "RESTARTS", "UPTIME");
        for status in self.status() {
            let uptime = self
                .units
                .get(&status.name)
                .and_then(|unit| unit.run.as_ref())
                .map_or_else(
                    || "-".to_owned(),
                    |run| format!("{}s", run.since.elapsed().as_secs()),
                );
            let pid = match status.pid {
                0 => "-".to_owned(),
                pid => pid.to_string(),
            };
            out.push_str(&status_line(
                &status.name,
                status.state.word(),
                &pid,
                &status.restarts.to_string(),
                &uptime,
            ));
        }
        out
    }
}

/// One line of the status table (fixed columns, as tools parse it).
pub fn status_line(name: &str, state: &str, pid: &str, restarts: &str, uptime: &str) -> String {
    format!("{name:<20} {state:<10} {pid:>7} {restarts:>8} {uptime:>10}\n")
}

/// A short word for how a process ended (`-`: never ended).
pub fn exit_text(status: &ServiceStatus) -> String {
    match status.last_exit {
        None => "-".to_owned(),
        Some(Exit::Code(code)) => format!("code {code}"),
        Some(Exit::Signal(signal)) => format!("signal {signal}"),
    }
}
