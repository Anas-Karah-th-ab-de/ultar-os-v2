//! Time and status in the launcher: the list of apps, waiting for a state, hard ends after the stop
//! deadline, restarts that are due, output that waits for the log service,
//! and the shutdown.

use crate::appid::AppId;
use crate::instances::Key;
use crate::launcher::{app_id, lock, Launcher};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{AppStatus, InstanceState, InstanceStatus, Role, MAX_APPS};

/// How often waiting output lines are pushed again.
const OUTPUT_RETRY: Duration = Duration::from_millis(500);

impl Launcher {
    /// The installed apps and their instances.
    pub fn list(&self) -> Vec<AppStatus> {
        let core = lock(&self.core);
        core.store
            .state
            .apps()
            .filter(|(_, e)| e.installed)
            .take(MAX_APPS)
            .map(|(id, e)| AppStatus {
                app: id.to_string(),
                uid: e.uid,
                profile: e.profile.word().to_owned(),
                version: e.version.clone(),
                instances: [Role::Ui, Role::Service]
                    .into_iter()
                    .filter_map(|role| {
                        core.instances
                            .get(&(id.clone(), role))
                            .map(|i| i.status(role))
                    })
                    .collect(),
            })
            .collect()
    }

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

    /// One turn of the timer: the next step of stops whose deadline
    /// passed, restarts that are due (handed to the restart thread, so a
    /// slow start never holds up the steps of other instances). Returns when
    /// to look again and whether the launcher stopped and nothing runs any
    /// more.
    fn tick(&self, now: Instant, restarts: &Sender<Key>) -> (Option<Instant>, bool) {
        let mut core = lock(&self.core);
        let stopping = core.stopping;
        let grace = core.config.stop_grace;
        let mut next: Option<Instant> = None;
        for (key, i) in core.instances.iter_mut() {
            match (i.state, i.deadline) {
                (InstanceState::Stopping | InstanceState::Starting, Some(at)) if at <= now => {
                    i.escalate(grace, now);
                }
                (InstanceState::Backoff, Some(_)) if stopping => {
                    i.state = InstanceState::Stopped;
                    i.deadline = None;
                }
                (InstanceState::Backoff, Some(at)) if at <= now => {
                    // marked, so the next turn does not ask again
                    i.deadline = None;
                    let _ = restarts.send(key.clone());
                }
                _ => {}
            }
            if let Some(at) = i.deadline {
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        }
        (next, stopping && core.active() == 0)
    }

    /// The timer loop: runs until [`Launcher::shutdown`] was called and
    /// every instance ended. Restarts run on a thread of their own that
    /// lives as long as the loop (stage 1 of a cell dies with the thread
    /// that started it, `PR_SET_PDEATHSIG`).
    pub fn run_timer(self: &Arc<Self>) {
        let (restarts, due) = channel::<Key>();
        let restarter = {
            let launcher = Arc::clone(self);
            std::thread::Builder::new()
                .name("ultar-launcher-restart".into())
                .spawn(move || {
                    for key in due {
                        let _ = launcher.start_key(key, &crate::open::Opening::default(), true);
                    }
                })
        };
        loop {
            let (next, done) = self.tick(Instant::now(), &restarts);
            if done {
                break;
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
        drop(restarts);
        if let Ok(restarter) = restarter {
            let _ = restarter.join();
        }
    }

    /// Waits (at most `timeout`) until no instance of `id` is active;
    /// returns the lock again and whether they all ended.
    pub(crate) fn wait_ended<'a>(
        &'a self,
        mut core: std::sync::MutexGuard<'a, crate::launcher::Core>,
        id: &AppId,
        timeout: Duration,
    ) -> (std::sync::MutexGuard<'a, crate::launcher::Core>, bool) {
        let deadline = Instant::now() + timeout;
        loop {
            let active = core
                .instances
                .iter()
                .any(|(key, i)| &key.0 == id && i.state.is_active());
            let left = deadline.saturating_duration_since(Instant::now());
            if !active || left.is_zero() {
                return (core, !active);
            }
            core = self
                .wake
                .wait_timeout(core, left)
                .map_or_else(|e| e.into_inner().0, |(g, _)| g);
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
