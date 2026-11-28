//! Running instances: start (one per app and role), stop, the end of a
//! run, restarts of background services and the shutdown.

use crate::appid::AppId;
use crate::error::LaunchError;
use crate::launcher::{app_id, lock, Core, Launcher};
use crate::manifest::Restart;
use crate::open::Opening;
use crate::plan::{self, Prepared};
use crate::policy::{History, Next};
use crate::spawn::{self, Events, Handshake, START_DEADLINE};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{
    Exit, InstanceState, InstanceStatus, Role, StartReply, StopReply,
};
use ultar_service::log::output::Stream;
use ultar_service::log::{Level, Record};
use ultar_sys::cell::{send_signal, StopSignal};

/// Key of an instance.
pub(crate) type Key = (AppId, Role);

/// One instance (app and role).
#[derive(Debug)]
pub(crate) struct Instance {
    pub(crate) state: InstanceState,
    /// The app's process id as the launcher sees it (shown, never signalled).
    pub(crate) pid: u32,
    /// Stage 1, the launcher's own child: the only process signalled (0: none).
    pub(crate) helper: u32,
    /// The memory group of this instance's role, below the app's group
    /// (`apps/<app>/<role>`; the last resort of a stop).
    pub(crate) group: Option<std::path::PathBuf>,
    /// Steps of a stop taken: 0 `SIGTERM`, 1 end the cell, 2 hard end.
    pub(crate) escalated: u8,
    /// When the run began (the restart rules count short runs).
    pub(crate) started: Instant,
    pub(crate) generation: u64,
    pub(crate) restarts: u32,
    pub(crate) last_exit: Option<Exit>,
    pub(crate) history: History,
    pub(crate) requested: bool,
    pub(crate) deadline: Option<Instant>,
    pub(crate) restart: Option<Restart>,
}

impl Instance {
    fn new(generation: u64, restart: Option<Restart>) -> Self {
        Self {
            state: InstanceState::Starting,
            pid: 0,
            helper: 0,
            group: None,
            escalated: 0,
            started: Instant::now(),
            generation,
            restarts: 0,
            last_exit: None,
            history: History::default(),
            requested: false,
            deadline: None,
            restart,
        }
    }

    pub(crate) fn status(&self, role: Role) -> InstanceStatus {
        InstanceStatus {
            role,
            state: self.state,
            pid: self.pid,
            restarts: self.restarts,
            last_exit: self.last_exit,
        }
    }

    /// Asks the instance to end (`hard`: the whole cell at once). Only
    /// stage 1 is signalled; it passes the request on.
    pub(crate) fn stop(&mut self, hard: bool, grace: Duration, now: Instant) -> bool {
        match self.state {
            InstanceState::Starting | InstanceState::Running | InstanceState::Stopping => {
                self.requested = true;
                if self.helper != 0 {
                    let signal = if hard {
                        StopSignal::KillRequest
                    } else {
                        StopSignal::Terminate
                    };
                    let _ = send_signal(self.helper, signal);
                }
                if hard {
                    self.escalated = self.escalated.max(1);
                }
                if self.state != InstanceState::Stopping || hard {
                    self.deadline = Some(now + grace);
                }
                self.state = InstanceState::Stopping;
                true
            }
            InstanceState::Backoff => {
                self.state = InstanceState::Stopped;
                self.deadline = None;
                false
            }
            _ => false,
        }
    }

    /// The next step of a stop whose deadline passed: end the cell, then
    /// end stage 1. At the hard end it returns the role's memory group,
    /// which the caller empties once it let go of the launcher's lock
    /// ([`crate::cleanup::end_group`] waits; review R2-3).
    pub(crate) fn escalate(&mut self, grace: Duration, now: Instant) -> Option<std::path::PathBuf> {
        if self.escalated == 0 {
            self.escalated = 1;
            self.deadline = Some(now + grace);
            if self.helper != 0 {
                let _ = send_signal(self.helper, StopSignal::KillRequest);
            }
            return None;
        }
        self.escalated = 2;
        self.deadline = None;
        if self.helper != 0 {
            let _ = send_signal(self.helper, StopSignal::Kill);
        }
        self.group.clone()
    }
}

/// Reports of one watched instance.
struct Watcher {
    launcher: Arc<Launcher>,
    key: Key,
    generation: u64,
    log_name: String,
}

impl Events for Watcher {
    fn line(&self, stream: Stream, text: &str) {
        let mut output = lock(&self.launcher.output);
        output.push(&self.log_name, stream, text);
        output.pump(Instant::now());
    }

    fn ended(&self, exit: Exit) {
        self.launcher.ended(&self.key, self.generation, exit);
    }
}

fn exit_record(key: &Key, exit: Exit) -> Record {
    let (word, value) = match exit {
        Exit::Code(code) => ("code", code),
        Exit::Signal(signal) => ("signal", signal),
        Exit::Setup(code) => ("setup", code),
    };
    Record::new(Level::Info, "launcher.ended")
        .id_or_invalid("app", key.0.as_str())
        .word("role", key.1.word())
        .word("how", word)
        .signed("value", i64::from(value))
}

impl Core {
    pub(crate) fn active(&self) -> usize {
        self.instances
            .values()
            .filter(|i| i.state.is_active())
            .count()
    }

    /// Reserves the instance `key` for a start; `Err(Some(reply))`: it runs.
    fn reserve(
        &mut self,
        key: &Key,
        opening: &Opening,
        again: bool,
    ) -> Result<(Prepared, u64), Result<StartReply, LaunchError>> {
        if self.stopping {
            return Err(Err(LaunchError::ShuttingDown));
        }
        if self.removing.contains(&key.0) {
            return Err(Err(LaunchError::Busy));
        }
        let to_open = self.check_opening(key, opening).map_err(Err)?;
        if let Some(i) = self.instances.get(key).filter(|i| i.state.is_active()) {
            // a URI or a second window would have to reach the running app:
            // apps have no bus endpoint yet
            if opening.uri.is_some() || (opening.new_window && to_open) {
                return Err(Err(LaunchError::NoOpen));
            }
            return Err(Ok(StartReply {
                state: i.state,
                pid: i.pid,
                already: true,
                opened: false,
            }));
        }
        if self.active() >= self.config.max_running as usize {
            return Err(Err(LaunchError::Limit));
        }
        let prepared = plan::prepare(self, &key.0, key.1, opening).map_err(Err)?;
        self.generation += 1;
        let generation = self.generation;
        let instance = self
            .instances
            .entry(key.clone())
            .or_insert_with(|| Instance::new(generation, prepared.restart));
        if again {
            instance.restarts = instance.restarts.saturating_add(1);
        } else {
            instance.history.reset();
        }
        instance.generation = generation;
        instance.restart = prepared.restart;
        instance.state = InstanceState::Starting;
        instance.requested = false;
        instance.deadline = None;
        instance.pid = 0;
        instance.helper = 0;
        instance.escalated = 0;
        instance.group = prepared.plan.group_dir.clone();
        instance.started = Instant::now();
        Ok((prepared, generation))
    }
}

impl Launcher {
    pub(crate) fn start_key(
        self: &Arc<Self>,
        key: Key,
        opening: &Opening,
        again: bool,
    ) -> Result<StartReply, LaunchError> {
        let (prepared, generation) = match lock(&self.core).reserve(&key, opening, again) {
            Ok(reserved) => reserved,
            Err(answer) => return answer,
        };
        let Ok(mut spawned) = spawn::spawn(&self.helper, &prepared.plan, &prepared.env) else {
            self.ended(
                &key,
                generation,
                Exit::Setup(crate::cell::CellFailure::Spawn as i32),
            );
            return Err(LaunchError::StartFailed);
        };
        self.spawned(&key, generation, &spawned);
        let handshake = spawned.handshake(Instant::now() + START_DEADLINE);
        let running = match handshake {
            Handshake::Running(pid) => Some(pid).filter(|pid| self.running(&key, generation, *pid)),
            Handshake::Failed(_) | Handshake::Lost => None,
        };
        if running.is_none() {
            // the cell is ended (or ends); the watcher reaps stage 1 and
            // reports the end
            spawned.abort();
        }
        if spawned.watch(self.watcher(&key, generation)).is_err() {
            self.ended(
                &key,
                generation,
                Exit::Setup(crate::cell::CellFailure::Spawn as i32),
            );
            return Err(LaunchError::StartFailed);
        }
        let pid = running.ok_or(LaunchError::StartFailed)?;
        self.logger.emit(
            Record::new(Level::Info, "launcher.started")
                .id_or_invalid("app", key.0.as_str())
                .word("role", key.1.word())
                .count("pid", u64::from(pid)),
        );
        Ok(StartReply {
            state: InstanceState::Running,
            pid,
            already: false,
            opened: opening.uri.is_some(),
        })
    }

    /// Stage 1 of `key` exists: a stop from now on reaches it.
    fn spawned(&self, key: &Key, generation: u64, spawned: &spawn::Spawned) {
        let mut core = lock(&self.core);
        let grace = core.config.stop_grace;
        if let Some(i) = core
            .instances
            .get_mut(key)
            .filter(|i| i.generation == generation)
        {
            i.helper = spawned.helper();
            i.deadline = Some(Instant::now() + START_DEADLINE + grace);
        }
    }

    /// The cell of `key` runs with `pid`; `false` when a stop was asked for
    /// meanwhile (the caller ends the cell).
    fn running(&self, key: &Key, generation: u64, pid: u32) -> bool {
        let mut core = lock(&self.core);
        let stopping = core.stopping;
        let grace = core.config.stop_grace;
        let Some(i) = core
            .instances
            .get_mut(key)
            .filter(|i| i.generation == generation)
        else {
            return false;
        };
        i.pid = pid;
        if i.requested || stopping {
            // the caller ends the cell (stage 1 is asked to)
            i.state = InstanceState::Stopping;
            i.escalated = 1;
            i.deadline = Some(Instant::now() + grace);
            return false;
        }
        i.state = InstanceState::Running;
        i.deadline = None;
        true
    }

    fn watcher(self: &Arc<Self>, key: &Key, generation: u64) -> Arc<dyn Events> {
        Arc::new(Watcher {
            launcher: Arc::clone(self),
            key: key.clone(),
            generation,
            log_name: key.0.log_name(),
        })
    }

    /// Stops `role` of `app` (a service stays stopped).
    pub fn stop(&self, app: &str, role: Role) -> Result<StopReply, LaunchError> {
        let id = app_id(app)?;
        let mut core = lock(&self.core);
        core.installed(&id)?;
        let grace = core.config.stop_grace;
        let state = match core.instances.get_mut(&(id, role)) {
            Some(i) => {
                i.stop(false, grace, Instant::now());
                i.state
            }
            None => InstanceState::Stopped,
        };
        self.wake.notify_all();
        Ok(StopReply { state })
    }

    /// Asks every instance of `id` to end; returns how many were running.
    pub(crate) fn stop_all_locked(&self, core: &mut Core, id: &AppId, hard: bool) -> u32 {
        let grace = core.config.stop_grace;
        let now = Instant::now();
        let stopped = core
            .instances
            .iter_mut()
            .filter(|(key, _)| &key.0 == id)
            .map(|(_, i)| u32::from(i.stop(hard, grace, now)))
            .sum();
        self.wake.notify_all();
        stopped
    }

    /// An instance ended (from its watcher).
    pub(crate) fn ended(&self, key: &Key, generation: u64, exit: Exit) {
        let mut core = lock(&self.core);
        let stopping = core.stopping;
        let Some(i) = core
            .instances
            .get_mut(key)
            .filter(|i| i.generation == generation)
        else {
            return;
        };
        let now = Instant::now();
        let ran = now.saturating_duration_since(i.started);
        let next = i
            .history
            .after_end(key.1, i.restart, exit, i.requested || stopping, ran, now);
        i.last_exit = Some(exit);
        i.pid = 0;
        // reported before stage 1 is reaped: no signal goes to its number later
        i.helper = 0;
        i.escalated = 0;
        (i.state, i.deadline) = match next {
            Next::Stay(state) => (state, None),
            Next::RestartAt(at) => (InstanceState::Backoff, Some(at)),
        };
        drop(core);
        self.logger.emit(exit_record(key, exit));
        self.wake.notify_all();
    }
}
