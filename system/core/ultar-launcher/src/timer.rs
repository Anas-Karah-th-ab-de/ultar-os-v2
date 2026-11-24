//! Time in the launcher: waiting for a state, hard ends after the stop
//! deadline, restarts that are due, output that waits for the log service,
//! and the shutdown.

use crate::launcher::{app_id, lock, Launcher};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{InstanceState, InstanceStatus, Role};
use ultar_sys::cell::{send_signal, StopSignal};

/// How often waiting output lines are pushed again.
const OUTPUT_RETRY: Duration = Duration::from_millis(500);

impl Launcher {
    /// Waits until `role` of `app` is in one of `states` (or `timeout`
    /// passed) and returns its status then. Woken by every change, no
    /// polling.
    pub fn wait_state(
        &self,
        app: &str,
        role: Role,
        states: &[InstanceState],
        timeout: Duration,
    ) -> Option<InstanceStatus> {
        let key = (app_id(app).ok()?, role);
        let deadline = Instant::now() + timeout;
        let mut core = lock(&self.core);
        loop {
            let status = core.instances.get(&key).map(|i| i.status(role));
            if let Some(status) = status.clone().filter(|s| states.contains(&s.state)) {
                return Some(status);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return status;
            }
            core = self
                .wake
                .wait_timeout(core, left)
                .map_or_else(|e| e.into_inner().0, |(guard, _)| guard);
        }
    }

    /// One turn of the timer: hard ends after the stop deadline, restarts
    /// that are due. Returns when to look again and whether the launcher
    /// stopped and nothing runs any more.
    fn tick(self: &Arc<Self>, now: Instant) -> (Option<Instant>, bool) {
        let mut due = Vec::new();
        let (next, done) = {
            let mut core = lock(&self.core);
            let stopping = core.stopping;
            let mut next: Option<Instant> = None;
            for (key, i) in core.instances.iter_mut() {
                match (i.state, i.deadline) {
                    (InstanceState::Stopping, Some(at)) if at <= now => {
                        if i.pid != 0 {
                            let _ = send_signal(i.pid, StopSignal::Kill);
                        }
                        i.deadline = None;
                    }
                    (InstanceState::Backoff, Some(at)) if at <= now && !stopping => {
                        due.push(key.clone())
                    }
                    (InstanceState::Backoff, Some(_)) if stopping => {
                        i.state = InstanceState::Stopped;
                        i.deadline = None;
                    }
                    (_, Some(at)) => next = Some(next.map_or(at, |n| n.min(at))),
                    _ => {}
                }
            }
            (next, stopping && core.active() == 0)
        };
        for key in due {
            let _ = self.start_key(key, None, true);
        }
        (next, done)
    }

    /// The timer loop: runs until [`Launcher::shutdown`] was called and
    /// every instance ended.
    pub fn run_timer(self: &Arc<Self>) {
        loop {
            let (next, done) = self.tick(Instant::now());
            if done {
                return;
            }
            let waiting = {
                let mut output = lock(&self.output);
                output.pump(Instant::now())
            };
            let now = Instant::now();
            let mut wait = next.map_or(Duration::from_secs(3600), |n| {
                n.saturating_duration_since(now)
            });
            if waiting {
                wait = wait.min(OUTPUT_RETRY);
            }
            let core = lock(&self.core);
            let _ = self.wake.wait_timeout(core, wait);
        }
    }

    /// Stops every instance and refuses new starts; [`Launcher::run_timer`]
    /// ends the rest after the grace time.
    pub fn shutdown(&self) {
        let mut core = lock(&self.core);
        core.stopping = true;
        let grace = core.config.stop_grace;
        let now = Instant::now();
        for i in core.instances.values_mut() {
            i.stop(false, grace, now);
        }
        drop(core);
        self.wake.notify_all();
    }
}
