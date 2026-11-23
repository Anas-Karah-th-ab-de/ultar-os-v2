//! The update service: the engine plus policy, permissions and progress reporting.
//!
//! [`Core`] is transport independent and synchronous, so every rule here is testable without
//! a bus; `busfront` puts it on the bus. One operation runs at a time; a second one is
//! refused with [`UpdateError::Busy`].

use crate::bootctl::BootControl;
use crate::clock::Clock;
use crate::commit::BootOutcome;
use crate::contract::{HistoryItem, Phase, PolicyMessage, Progress, StatusReply};
use crate::engine::{Engine, EnginePhase};
use crate::error::UpdateError;
use crate::fault::CancelFlag;
use crate::health::HealthCheck;
use crate::history::{self, HistoryKind};
use crate::policy::Policy;
use crate::source::UpdateSource;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

/// Opens the configured network source (a fresh one per operation, so a changed session
/// token is picked up).
pub trait SourceFactory: Send + Sync {
    /// The source to ask for updates.
    fn open(&self) -> Result<Box<dyn UpdateSource + Send>, UpdateError>;
}

/// Receives every state change (the bus topic in production).
pub trait Publish: Send + Sync {
    /// Called with the new state.
    fn publish(&self, progress: &Progress);
}

/// Who may do what.
#[derive(Debug, Clone, Default)]
pub struct Access {
    /// Bus identities that may apply, cancel, set the policy, use offline bundles and start
    /// the previous version. Empty: nobody.
    pub admins: Vec<String>,
    /// The identity whose `network` reports are believed. `None`: nobody (the network then
    /// always counts as metered).
    pub network_identity: Option<String>,
    /// Folders offline bundles may be taken from (stick mount points). Empty: none.
    pub offline_roots: Vec<PathBuf>,
}

/// A long operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// Look for a newer release.
    Check,
    /// Download the newest release.
    Download {
        /// The user accepted a metered connection.
        allow_metered: bool,
    },
    /// Install the staged release.
    Apply,
    /// Take the update from a local bundle.
    Offline {
        /// File or folder.
        path: PathBuf,
        /// Accept an older release.
        allow_downgrade: bool,
    },
    /// What the policy allows by itself: check, download, install.
    Automatic,
}

/// Seconds between automatic runs unless the timer says otherwise (6 hours).
pub const DEFAULT_CHECK_INTERVAL: u64 = 6 * 60 * 60;
/// Jobs callers without administrator rights may start per [`UNPRIVILEGED_WINDOW`].
pub const UNPRIVILEGED_JOBS: usize = 6;
/// Length of the rate-limit window for unprivileged callers, in seconds.
pub const UNPRIVILEGED_WINDOW: u64 = 600;

pub(crate) struct Snapshot {
    pub(crate) progress: Progress,
    pub(crate) metered: Option<bool>,
    pub(crate) available: Option<(String, bool, String)>,
    pub(crate) percent: u64,
    pub(crate) status: Option<StatusReply>,
}

/// Who started the running job, and how often unprivileged callers started one lately.
pub(crate) struct Guard {
    /// The running job was started by an administrator or by the service itself.
    pub(crate) privileged: bool,
    /// Seconds between automatic runs (set by the timer; used to schedule the next one).
    pub(crate) interval: u64,
    recent: VecDeque<u64>,
}

impl Guard {
    /// Drops entries outside the window and tells whether one more unprivileged job fits.
    fn admit_unprivileged(&mut self, now: u64) -> bool {
        // Entries from the future (clock set back) are dropped too, so a clock change can
        // never lock callers out for longer than one window.
        self.recent
            .retain(|t| *t <= now && now - *t < UNPRIVILEGED_WINDOW);
        self.recent.len() < UNPRIVILEGED_JOBS
    }
}

/// The update service without its transport.
pub struct Core<B: BootControl> {
    pub(crate) engine: Mutex<Engine<B>>,
    pub(crate) cancel: Arc<CancelFlag>,
    pub(crate) snapshot: Mutex<Snapshot>,
    pub(crate) sources: Arc<dyn SourceFactory>,
    publisher: Arc<dyn Publish>,
    access: Access,
    pub(crate) state_dir: PathBuf,
    running: AtomicBool,
    pub(crate) guard: Mutex<Guard>,
    pub(crate) clock: Arc<dyn Clock>,
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn idle_progress() -> Progress {
    Progress {
        phase: Phase::Idle,
        done: 0,
        total: 0,
        version: None,
        critical: false,
        notes_key: None,
        error_key: None,
    }
}

impl<B: BootControl> Core<B> {
    /// Builds the service. `cancel` must be the checkpoint the engine was opened with
    /// (`Environment::control`), otherwise cancelling has no effect.
    pub fn new(
        engine: Engine<B>,
        cancel: Arc<CancelFlag>,
        sources: Arc<dyn SourceFactory>,
        publisher: Arc<dyn Publish>,
        access: Access,
    ) -> Self {
        let state_dir = engine.config().state_dir.clone();
        let clock = engine.clock();
        let core = Self {
            guard: Mutex::new(Guard {
                privileged: false,
                interval: DEFAULT_CHECK_INTERVAL,
                recent: VecDeque::new(),
            }),
            clock,
            engine: Mutex::new(engine),
            cancel,
            snapshot: Mutex::new(Snapshot {
                progress: idle_progress(),
                metered: None,
                available: None,
                percent: 0,
                status: None,
            }),
            sources,
            publisher,
            access,
            state_dir,
            running: AtomicBool::new(false),
        };
        core.refresh(&lock(&core.engine), None);
        core
    }

    /// True when `caller` is an administrator of the service.
    pub fn is_admin(&self, caller: &str) -> bool {
        self.access.admins.iter().any(|a| a == caller)
    }

    fn require_admin(&self, caller: &str) -> Result<(), UpdateError> {
        if self.is_admin(caller) {
            Ok(())
        } else {
            Err(UpdateError::PermissionDenied)
        }
    }

    pub(crate) fn announce(&self, edit: impl FnOnce(&mut Snapshot)) {
        let progress = {
            let mut snapshot = lock(&self.snapshot);
            edit(&mut snapshot);
            snapshot.progress.clone()
        };
        self.publisher.publish(&progress);
    }

    pub(crate) fn start_phase(&self, phase: Phase) {
        self.announce(|s| {
            s.progress.phase = phase;
            s.progress.done = 0;
            s.progress.total = 0;
            s.progress.error_key = None;
            s.percent = 0;
        });
    }

    /// Byte progress; published only when the percentage changes (at most 101 events).
    pub(crate) fn advance(&self, done: u64, total: u64) {
        let percent = done.saturating_mul(100).checked_div(total).unwrap_or(0);
        let changed = {
            let mut snapshot = lock(&self.snapshot);
            snapshot.progress.done = done;
            snapshot.progress.total = total;
            let changed = snapshot.percent != percent;
            snapshot.percent = percent;
            changed
        };
        if changed {
            self.announce(|_| {});
        }
    }

    /// Brings the snapshot in line with the engine and publishes it.
    pub(crate) fn refresh(&self, engine: &Engine<B>, error_key: Option<&'static str>) {
        let status = engine.status().ok();
        let error_key = error_key
            .map(str::to_string)
            .or_else(|| boot_notice(engine));
        let policy = engine.policy();
        self.announce(|s| {
            let Some(status) = &status else {
                s.progress.error_key = Some(UpdateError::StateCorrupt.key().to_string());
                return;
            };
            let version = status.pending_version.map(|v| v.to_string());
            let (phase, version) = match status.phase {
                EnginePhase::Partial => (Phase::Paused, version),
                EnginePhase::Staged => (Phase::Staged, version),
                EnginePhase::RebootRequired => (Phase::RebootRequired, version),
                EnginePhase::TrialRunning => (Phase::Verifying, version),
                EnginePhase::Idle => match &s.available {
                    Some((version, _, _)) => (Phase::Available, Some(version.clone())),
                    None => (Phase::Idle, None),
                },
            };
            let known = s
                .available
                .as_ref()
                .filter(|a| Some(&a.0) == version.as_ref());
            s.progress = Progress {
                phase,
                done: 0,
                total: 0,
                critical: known.is_some_and(|a| a.1),
                notes_key: known.map(|a| a.2.clone()),
                version,
                error_key: error_key.clone(),
            };
            s.status = Some(StatusReply {
                progress: s.progress.clone(),
                current_version: status.current_version.to_string(),
                slot: status.active_slot.letter().to_string(),
                channel: policy.channel.clone(),
                rollback_floor: status.rollback_floor,
                previous_startable: status.previous_startable,
                previous_version: status.previous_version.map(|v| v.to_string()),
                metered: s.metered,
            });
        });
    }

    /// The full state. Never blocks on a running operation.
    pub fn status(&self) -> Result<StatusReply, UpdateError> {
        let snapshot = lock(&self.snapshot);
        let mut reply = snapshot.status.clone().ok_or(UpdateError::StateCorrupt)?;
        reply.progress = snapshot.progress.clone();
        reply.metered = snapshot.metered;
        Ok(reply)
    }

    /// Reserves the single operation slot for `job` after checking `caller`'s rights.
    /// The caller must then hand the job to [`Core::run`] (or call [`Core::release`]).
    ///
    /// Callers that are not administrators may only check and download, and only
    /// [`UNPRIVILEGED_JOBS`] times per [`UNPRIVILEGED_WINDOW`] seconds together – so that no
    /// app can keep the single slot busy against the administrator or the automatic run.
    pub fn accept(&self, caller: &str, job: &Job) -> Result<(), UpdateError> {
        let privileged = self.is_admin(caller) || *job == Job::Automatic;
        match job {
            Job::Apply => self.require_admin(caller)?,
            Job::Offline { path, .. } => {
                self.require_admin(caller)?;
                self.offline_path(path)?;
            }
            Job::Check | Job::Download { .. } | Job::Automatic => {}
        }
        let mut guard = lock(&self.guard);
        let now = self.clock.now_unix();
        if !privileged && !guard.admit_unprivileged(now) {
            return Err(UpdateError::RateLimited);
        }
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| UpdateError::Busy)?;
        if !privileged {
            guard.recent.push_back(now);
        }
        guard.privileged = privileged;
        // A cancel request that arrived after the previous job had ended belongs to nobody.
        self.cancel.reset();
        Ok(())
    }

    /// Gives the operation slot back without running a job.
    pub fn release(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Resolves an offline location (links and `..` included) and checks that it lies inside
    /// one of the configured folders. Returns the resolved path, which is what is then read.
    pub(crate) fn offline_path(&self, path: &Path) -> Result<PathBuf, UpdateError> {
        let real = std::fs::canonicalize(path).map_err(|_| UpdateError::InvalidArgument)?;
        let allowed = self
            .access
            .offline_roots
            .iter()
            .any(|root| std::fs::canonicalize(root).is_ok_and(|root| real.starts_with(root)));
        if allowed {
            Ok(real)
        } else {
            Err(UpdateError::PermissionDenied)
        }
    }

    /// Stops the running operation at its next checkpoint, or – when none is running – drops
    /// what is pending (staged bundle, release waiting for its restart). Admin only.
    pub fn cancel(&self, caller: &str) -> Result<(), UpdateError> {
        self.require_admin(caller)?;
        if self.running.load(Ordering::SeqCst) {
            self.cancel.cancel();
            return Ok(());
        }
        let mut engine = lock(&self.engine);
        let result = engine.cancel_pending();
        lock(&self.snapshot).available = None;
        self.refresh(&engine, result.as_ref().err().map(UpdateError::key));
        result
    }

    /// Makes the previous version start next. Admin only.
    pub fn boot_previous(&self, caller: &str) -> Result<(), UpdateError> {
        self.require_admin(caller)?;
        let mut engine = self.engine.try_lock().map_err(|_| UpdateError::Busy)?;
        engine.boot_previous()?;
        self.refresh(&engine, None);
        Ok(())
    }

    /// Records what the network service says about the connection.
    pub fn network(&self, caller: &str, metered: bool) -> Result<(), UpdateError> {
        if self.access.network_identity.as_deref() != Some(caller) {
            return Err(UpdateError::PermissionDenied);
        }
        lock(&self.snapshot).metered = Some(metered);
        Ok(())
    }

    /// The stored policy.
    pub fn policy(&self) -> PolicyMessage {
        let policy = Policy::load(&self.state_dir.join("policy")).0;
        PolicyMessage {
            automatic: policy.automatic,
            window: policy.window,
            channel: policy.channel,
            allow_metered: policy.allow_metered,
            rate_limit_kib: policy.rate_limit_kib,
        }
    }

    /// Validates and stores a new policy. Admin only.
    pub fn set_policy(&self, caller: &str, message: &PolicyMessage) -> Result<(), UpdateError> {
        self.require_admin(caller)?;
        let policy = Policy {
            automatic: message.automatic,
            window: message.window,
            channel: message.channel.clone(),
            allow_metered: message.allow_metered,
            rate_limit_kib: message.rate_limit_kib,
        };
        // Through the engine (not behind its back): a staged bundle of another channel goes.
        let engine = self.engine.try_lock().map_err(|_| UpdateError::Busy)?;
        engine.set_policy(&policy)?;
        self.refresh(&engine, None);
        let mut snapshot = lock(&self.snapshot);
        snapshot.available = None;
        if let Some(status) = &mut snapshot.status {
            status.channel = policy.channel;
        }
        Ok(())
    }

    /// The newest `limit` history entries (at most 200), newest first.
    pub fn history(&self, limit: u16) -> Vec<HistoryItem> {
        let limit = usize::from(limit).min(history::MAX_ENTRIES);
        history::load(&self.state_dir.join("history"))
            .into_iter()
            .rev()
            .take(limit)
            .map(|entry| HistoryItem {
                time: entry.time,
                kind_key: entry.kind.key().to_string(),
                version: entry.version.map(|v| v.to_string()),
                detail_key: entry.detail_key,
            })
            .collect()
    }

    /// To be called once after the system started (see [`Engine::resume_after_boot`]).
    pub fn resume_after_boot(&self, health: &dyn HealthCheck) -> Result<BootOutcome, UpdateError> {
        let mut engine = lock(&self.engine);
        let outcome = engine.resume_after_boot(health);
        let error_key = match &outcome {
            Ok(BootOutcome::RolledBack) => Some(UpdateError::BootFailed.key()),
            Ok(BootOutcome::HealthFailed) => Some(UpdateError::Unhealthy.key()),
            Ok(_) => None,
            Err(error) => Some(error.key()),
        };
        self.refresh(&engine, error_key);
        outcome
    }
}

/// What the last start told about a new release (it did not start, or failed its health
/// check), as long as nothing newer happened: taken from the history, so the status says the
/// same as the history and survives later operations that end without news (an automatic
/// check right after the start, a daemon restart). The next history entry clears it.
fn boot_notice<B: BootControl>(engine: &Engine<B>) -> Option<String> {
    let last = engine.history().pop()?;
    (last.kind == HistoryKind::RolledBack)
        .then_some(last.detail_key)
        .flatten()
}
