//! Running instances: start (one per app and role), stop, the end of a
//! run, restarts of background services and the shutdown.

use crate::appid::AppId;
use crate::error::LaunchError;
use crate::launcher::{app_id, lock, Core, Launcher};
use crate::manifest::Restart;
use crate::plan::{self, Prepared};
use crate::policy::{History, Next};
use crate::spawn::{self, Events, Handshake, START_DEADLINE};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::{
    AppStatus, Exit, InstanceState, InstanceStatus, Role, StartReply, StopReply, MAX_APPS,
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
    pub(crate) pid: u32,
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

    /// Asks the instance to end (`hard`: at once).
    pub(crate) fn stop(&mut self, hard: bool, grace: Duration, now: Instant) -> bool {
        match self.state {
            InstanceState::Starting | InstanceState::Running | InstanceState::Stopping => {
                self.requested = true;
                if self.pid != 0 {
                    let signal = if hard {
                        StopSignal::Kill
                    } else {
                        StopSignal::Terminate
                    };
                    let _ = send_signal(self.pid, signal);
                }
                if self.state != InstanceState::Stopping || hard {
                    self.deadline = Some(if hard { now } else { now + grace });
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
        token: Option<u64>,
        again: bool,
    ) -> Result<(Prepared, u64), Result<StartReply, LaunchError>> {
        if self.stopping {
            return Err(Err(LaunchError::ShuttingDown));
        }
        if let Some(i) = self.instances.get(key).filter(|i| i.state.is_active()) {
            return Err(Ok(StartReply {
                state: i.state,
                pid: i.pid,
                already: true,
            }));
        }
        if self.active() >= self.config.max_running as usize {
            return Err(Err(LaunchError::Limit));
        }
        let prepared = plan::prepare(self, &key.0, key.1, token).map_err(Err)?;
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
        Ok((prepared, generation))
    }
}

impl Launcher {
    /// Starts `role` of `app`; a running instance is reported, not started
    /// twice. `token`: the compositor's activation token for the window.
    pub fn start(
        self: &Arc<Self>,
        app: &str,
        role: Role,
        token: Option<u64>,
    ) -> Result<StartReply, LaunchError> {
        let key = (app_id(app)?, role);
        self.start_key(key, token, false)
    }

    pub(crate) fn start_key(
        self: &Arc<Self>,
        key: Key,
        token: Option<u64>,
        again: bool,
    ) -> Result<StartReply, LaunchError> {
        let (prepared, generation) = match lock(&self.core).reserve(&key, token, again) {
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
        let pid = match spawned.handshake(Instant::now() + START_DEADLINE) {
            Handshake::Running(pid) => pid,
            Handshake::Failed(_) | Handshake::Lost => {
                // the watcher reaps the helper and reports the failed start
                if spawned.watch(self.watcher(&key, generation)).is_err() {
                    self.ended(
                        &key,
                        generation,
                        Exit::Setup(crate::cell::CellFailure::Spawn as i32),
                    );
                }
                return Err(LaunchError::StartFailed);
            }
        };
        let state = self.running(&key, generation, pid);
        self.logger.emit(
            Record::new(Level::Info, "launcher.started")
                .id_or_invalid("app", key.0.as_str())
                .word("role", key.1.word())
                .count("pid", u64::from(pid)),
        );
        if spawned.watch(self.watcher(&key, generation)).is_err() {
            let _ = send_signal(pid, StopSignal::Kill);
            return Err(LaunchError::StartFailed);
        }
        Ok(StartReply {
            state,
            pid,
            already: false,
        })
    }

    /// The cell of `key` runs with `pid`; a stop asked for meanwhile ends
    /// it at once. Returns the state now.
    fn running(&self, key: &Key, generation: u64, pid: u32) -> InstanceState {
        let mut core = lock(&self.core);
        let stopping = core.stopping;
        let Some(i) = core
            .instances
            .get_mut(key)
            .filter(|i| i.generation == generation)
        else {
            return InstanceState::Stopping;
        };
        i.pid = pid;
        if i.requested || stopping {
            let _ = send_signal(pid, StopSignal::Kill);
            i.state = InstanceState::Stopping;
        } else if i.state == InstanceState::Starting {
            i.state = InstanceState::Running;
        }
        i.state
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
        let next = i.history.after_end(
            key.1,
            i.restart,
            exit,
            i.requested || stopping,
            Instant::now(),
        );
        i.last_exit = Some(exit);
        i.pid = 0;
        (i.state, i.deadline) = match next {
            Next::Stay(state) => (state, None),
            Next::RestartAt(at) => (InstanceState::Backoff, Some(at)),
        };
        drop(core);
        self.logger.emit(exit_record(key, exit));
        self.wake.notify_all();
    }

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
}
