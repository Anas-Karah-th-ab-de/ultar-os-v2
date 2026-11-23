//! The life of a service: `starting → ready → stopping → stopped`.
//!
//! One [`Lifecycle`] is shared by the main thread, the signal thread and
//! whoever else can ask the service to stop (a bus method, a fatal error).
//! The state only ever moves forward. Hooks registered with
//! [`Lifecycle::on_stop`] run exactly once, on the thread that requests the
//! stop; they are how a service blocked in a bus call or in `accept` gets
//! woken (close the client, connect to the own socket).

use std::fmt;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Where a service is in its life.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    /// Process started; not answering yet.
    Starting,
    /// Answering requests.
    Ready,
    /// Asked to stop; flushing and closing.
    Stopping,
    /// Done; the process is about to exit.
    Stopped,
}

impl State {
    /// Stable lower-case name (used in logs and in the readiness protocol).
    pub const fn name(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Stopping => "stopping",
            Self::Stopped => "stopped",
        }
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A signal the runtime reacts to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Signal {
    /// `SIGHUP` (1): reload.
    Hangup,
    /// `SIGINT` (2): stop.
    Interrupt,
    /// `SIGTERM` (15): stop.
    Terminate,
}

impl Signal {
    /// The Linux signal number.
    pub const fn number(self) -> u32 {
        match self {
            Self::Hangup => 1,
            Self::Interrupt => 2,
            Self::Terminate => 15,
        }
    }

    /// The signal with this number, if the runtime handles it.
    pub const fn from_number(number: u32) -> Option<Self> {
        match number {
            1 => Some(Self::Hangup),
            2 => Some(Self::Interrupt),
            15 => Some(Self::Terminate),
            _ => None,
        }
    }

    /// Stable lower-case name for logs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Hangup => "hup",
            Self::Interrupt => "int",
            Self::Terminate => "term",
        }
    }
}

/// Why a service stops.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StopCause {
    /// A stop signal arrived.
    Signal(Signal),
    /// Someone allowed to do so asked over the service's own interface.
    Request,
    /// The service cannot continue (lost its bus connection for good, …).
    Fault,
}

impl StopCause {
    /// Stable lower-case name for logs.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Signal(signal) => signal.name(),
            Self::Request => "request",
            Self::Fault => "fault",
        }
    }
}

/// Why [`Lifecycle::wait_change`] returned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wake {
    /// A stop was requested.
    Stop(StopCause),
    /// A reload was requested; the total number of reloads so far.
    Reload(u64),
}

type StopHook = Box<dyn FnOnce() + Send>;
type ReloadHook = Box<dyn Fn() + Send + Sync>;

struct Status {
    state: State,
    cause: Option<StopCause>,
    reloads: u64,
    /// The stop hooks have run. Threads that wait for the stop are only
    /// woken after that, so what the hooks do (announce the stop, log it)
    /// always comes before what the woken thread does.
    hooks_done: bool,
}

impl Status {
    /// Whether a thread waiting for `wanted` may continue.
    fn reached(&self, wanted: State) -> bool {
        match self.state {
            State::Stopping => {
                wanted < State::Stopping || (wanted == State::Stopping && self.hooks_done)
            }
            state => state >= wanted,
        }
    }
}

struct Inner {
    status: Mutex<Status>,
    changed: Condvar,
    stop_hooks: Mutex<Vec<StopHook>>,
    reload_hooks: Mutex<Vec<ReloadHook>>,
}

/// Most hooks of each kind; registering more is refused.
pub const MAX_HOOKS: usize = 32;

/// Shared state machine of one service. Cloning is cheap.
#[derive(Clone)]
pub struct Lifecycle {
    inner: Arc<Inner>,
}

impl fmt::Debug for Lifecycle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Lifecycle")
            .field("state", &self.state())
            .finish()
    }
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Lifecycle {
    /// A lifecycle in state [`State::Starting`].
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                status: Mutex::new(Status {
                    state: State::Starting,
                    cause: None,
                    reloads: 0,
                    hooks_done: false,
                }),
                changed: Condvar::new(),
                stop_hooks: Mutex::new(Vec::new()),
                reload_hooks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// The current state.
    pub fn state(&self) -> State {
        lock(&self.inner.status).state
    }

    /// Why the service stops, once it does.
    pub fn stop_cause(&self) -> Option<StopCause> {
        lock(&self.inner.status).cause
    }

    /// Number of reload requests so far.
    pub fn reloads(&self) -> u64 {
        lock(&self.inner.status).reloads
    }

    /// Whether a stop was requested (state `Stopping` or `Stopped`).
    pub fn is_stopping(&self) -> bool {
        self.state() >= State::Stopping
    }

    /// `Starting → Ready`. Returns `false` when the service is already
    /// ready or stopping (the state is then left alone).
    pub fn set_ready(&self) -> bool {
        let mut status = lock(&self.inner.status);
        if status.state != State::Starting {
            return false;
        }
        status.state = State::Ready;
        self.inner.changed.notify_all();
        true
    }

    /// Moves to `Stopping` and runs the stop hooks. Returns `true` for the
    /// first request; later requests change nothing and return `false`.
    pub fn request_stop(&self, cause: StopCause) -> bool {
        {
            let mut status = lock(&self.inner.status);
            if status.state >= State::Stopping {
                return false;
            }
            status.state = State::Stopping;
            status.cause = Some(cause);
        }
        // Outside the status lock: hooks may query the state.
        let hooks = std::mem::take(&mut *lock(&self.inner.stop_hooks));
        for hook in hooks {
            hook();
        }
        lock(&self.inner.status).hooks_done = true;
        self.inner.changed.notify_all();
        true
    }

    /// Moves to `Stopped` (from any state).
    pub fn mark_stopped(&self) {
        let mut status = lock(&self.inner.status);
        status.state = State::Stopped;
        status.hooks_done = true;
        self.inner.changed.notify_all();
    }

    /// Registers work to do when a stop is requested. If the stop was
    /// already requested the hook runs at once. Returns `false` (and drops
    /// the hook) when [`MAX_HOOKS`] hooks are registered.
    pub fn on_stop(&self, hook: impl FnOnce() + Send + 'static) -> bool {
        // The status lock is held while the hook is stored so that a
        // concurrent `request_stop` either sees it or has finished moving
        // the state.
        let status = lock(&self.inner.status);
        if status.state >= State::Stopping {
            drop(status);
            hook();
            return true;
        }
        let mut hooks = lock(&self.inner.stop_hooks);
        if hooks.len() >= MAX_HOOKS {
            return false;
        }
        hooks.push(Box::new(hook));
        true
    }

    /// Registers work to do on every reload request (`SIGHUP`). Returns
    /// `false` when [`MAX_HOOKS`] hooks are registered.
    pub fn on_reload(&self, hook: impl Fn() + Send + Sync + 'static) -> bool {
        let mut hooks = lock(&self.inner.reload_hooks);
        if hooks.len() >= MAX_HOOKS {
            return false;
        }
        hooks.push(Box::new(hook));
        true
    }

    /// Runs the reload hooks; ignored once the service is stopping.
    /// Returns whether the hooks ran.
    pub fn reload(&self) -> bool {
        {
            let mut status = lock(&self.inner.status);
            if status.state >= State::Stopping {
                return false;
            }
            status.reloads += 1;
            self.inner.changed.notify_all();
        }
        for hook in lock(&self.inner.reload_hooks).iter() {
            hook();
        }
        true
    }

    /// Blocks until the state is at least `wanted`.
    pub fn wait_for(&self, wanted: State) {
        let mut status = lock(&self.inner.status);
        while !status.reached(wanted) {
            status = self
                .inner
                .changed
                .wait(status)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks until the state is at least `wanted` or `timeout` passed.
    /// Returns whether the state was reached.
    pub fn wait_for_timeout(&self, wanted: State, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut status = lock(&self.inner.status);
        loop {
            if status.reached(wanted) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            status = match self.inner.changed.wait_timeout(status, deadline - now) {
                Ok((guard, _)) => guard,
                Err(poisoned) => poisoned.into_inner().0,
            };
        }
    }

    /// Blocks until a stop is requested or the reload counter passes
    /// `seen_reloads`. Lets the main thread of a daemon do the reload
    /// itself (re-read a policy file) instead of in a hook on the signal
    /// thread. A stop wins over a reload.
    pub fn wait_change(&self, seen_reloads: u64) -> Wake {
        let mut status = lock(&self.inner.status);
        loop {
            if status.reached(State::Stopping) {
                return Wake::Stop(status.cause.unwrap_or(StopCause::Fault));
            }
            if status.reloads > seen_reloads && status.state < State::Stopping {
                return Wake::Reload(status.reloads);
            }
            status = self
                .inner
                .changed
                .wait(status)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Blocks until a stop is requested and returns its cause.
    pub fn wait_stop(&self) -> StopCause {
        self.wait_for(State::Stopping);
        self.stop_cause().unwrap_or(StopCause::Fault)
    }
}

#[cfg(test)]
#[path = "lifecycle_tests.rs"]
mod tests;
