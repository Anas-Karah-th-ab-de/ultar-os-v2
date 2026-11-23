//! The update service: the engine plus policy, permissions and progress reporting.
//!
//! [`Core`] is transport independent and synchronous, so every rule here is testable without
//! a bus; `busfront` puts it on the bus. One operation runs at a time; a second one is
//! refused with [`UpdateError::Busy`].

use crate::bootctl::BootControl;
use crate::commit::BootOutcome;
use crate::contract::{HistoryItem, Phase, PolicyMessage, Progress, StatusReply};
use crate::engine::{Engine, EnginePhase, Offer};
use crate::error::UpdateError;
use crate::fault::CancelFlag;
use crate::health::HealthCheck;
use crate::history;
use crate::policy::{real_pacer, Policy};
use crate::source::UpdateSource;
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

/// A verified offer together with the source it came from.
type Found = (Offer, Box<dyn UpdateSource + Send>);

struct Snapshot {
    progress: Progress,
    metered: Option<bool>,
    available: Option<(String, bool, String)>,
    percent: u64,
    status: Option<StatusReply>,
}

/// The update service without its transport.
pub struct Core<B: BootControl> {
    engine: Mutex<Engine<B>>,
    cancel: Arc<CancelFlag>,
    snapshot: Mutex<Snapshot>,
    sources: Arc<dyn SourceFactory>,
    publisher: Arc<dyn Publish>,
    access: Access,
    state_dir: PathBuf,
    running: AtomicBool,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
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
        let core = Self {
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

    fn announce(&self, edit: impl FnOnce(&mut Snapshot)) {
        let progress = {
            let mut snapshot = lock(&self.snapshot);
            edit(&mut snapshot);
            snapshot.progress.clone()
        };
        self.publisher.publish(&progress);
    }

    fn start_phase(&self, phase: Phase) {
        self.announce(|s| {
            s.progress.phase = phase;
            s.progress.done = 0;
            s.progress.total = 0;
            s.progress.error_key = None;
            s.percent = 0;
        });
    }

    /// Byte progress; published only when the percentage changes (at most 101 events).
    fn advance(&self, done: u64, total: u64) {
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
    fn refresh(&self, engine: &Engine<B>, error_key: Option<&'static str>) {
        let status = engine.status().ok();
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
                error_key: error_key.map(str::to_string),
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
    pub fn accept(&self, caller: &str, job: &Job) -> Result<(), UpdateError> {
        match job {
            Job::Apply => self.require_admin(caller)?,
            Job::Offline { path, .. } => {
                self.require_admin(caller)?;
                self.offline_path(path)?;
            }
            Job::Check | Job::Download { .. } | Job::Automatic => {}
        }
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| ())
            .map_err(|_| UpdateError::Busy)
    }

    /// Gives the operation slot back without running a job.
    pub fn release(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Resolves an offline location (links and `..` included) and checks that it lies inside
    /// one of the configured folders. Returns the resolved path, which is what is then read.
    fn offline_path(&self, path: &Path) -> Result<PathBuf, UpdateError> {
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

    /// Runs an accepted job to its end and releases the operation slot. The result is also
    /// published (state and, for failures, the error key).
    pub fn run(&self, job: Job) -> Result<(), UpdateError> {
        let mut engine = lock(&self.engine);
        let result = self.run_job(&mut engine, job);
        let error_key = match &result {
            Ok(()) => None,
            Err(UpdateError::Cancelled) => {
                let _ = engine.cancel_pending();
                lock(&self.snapshot).available = None;
                None
            }
            Err(error) => Some(error.key()),
        };
        self.cancel.reset();
        // The slot is free before the final state is published: a client that reacts to that
        // event with its next request must not be told "busy".
        self.release();
        self.refresh(&engine, error_key);
        result
    }

    fn run_job(&self, engine: &mut Engine<B>, job: Job) -> Result<(), UpdateError> {
        match job {
            Job::Check => self.check(engine).map(|_| ()),
            Job::Download { allow_metered } => self.download(engine, allow_metered),
            Job::Apply => self.apply(engine),
            Job::Offline {
                path,
                allow_downgrade,
            } => {
                self.start_phase(Phase::Downloading);
                // Checked again here and the resolved path is used: what was checked is
                // what is read, even if the entry was swapped since the request came in.
                let path = self.offline_path(&path)?;
                let offer = engine.stage_offline(&path, allow_downgrade, &mut |done, total| {
                    self.advance(done, total)
                })?;
                self.remember(Some(&offer));
                Ok(())
            }
            Job::Automatic => self.automatic(engine),
        }
    }

    fn remember(&self, offer: Option<&Offer>) {
        lock(&self.snapshot).available = offer.map(|o| {
            let m = o.verified.manifest();
            (m.version.to_string(), m.critical, m.notes_key.clone())
        });
    }

    fn check(&self, engine: &mut Engine<B>) -> Result<Option<Found>, UpdateError> {
        self.start_phase(Phase::Checking);
        let mut source = self.sources.open()?;
        let channel = engine.policy().channel;
        let offer = engine.check(&mut *source, &channel)?;
        self.remember(offer.as_ref());
        Ok(offer.map(|offer| (offer, source)))
    }

    fn download(&self, engine: &mut Engine<B>, allow_metered: bool) -> Result<(), UpdateError> {
        // Every accepted job announces a busy phase first, so that a client that follows the
        // progress topic always sees a beginning and an end – also when the job is refused.
        self.start_phase(Phase::Checking);
        if lock(&self.snapshot).metered != Some(false) && !allow_metered {
            return Err(UpdateError::Metered);
        }
        let (offer, mut source) = self.check(engine)?.ok_or(UpdateError::NoUpdate)?;
        self.start_phase(Phase::Downloading);
        let mut pacer = real_pacer(engine.policy().rate_limit_kib);
        engine.download(&mut *source, &offer, &mut pacer, &mut |done, total| {
            self.advance(done, total)
        })?;
        Ok(())
    }

    fn apply(&self, engine: &mut Engine<B>) -> Result<(), UpdateError> {
        self.start_phase(Phase::Applying);
        engine.apply(&mut |_, done, total| self.advance(done, total))?;
        Ok(())
    }

    fn automatic(&self, engine: &mut Engine<B>) -> Result<(), UpdateError> {
        let policy = engine.policy();
        let metered = lock(&self.snapshot).metered;
        if policy
            .automatic_allowed(engine.clock_now(), metered)
            .is_err()
        {
            return Ok(());
        }
        match engine.status()?.phase {
            EnginePhase::RebootRequired | EnginePhase::TrialRunning => return Ok(()),
            EnginePhase::Staged => return self.apply(engine),
            EnginePhase::Idle | EnginePhase::Partial => {}
        }
        match self.download(engine, true) {
            Err(UpdateError::NoUpdate) => Ok(()),
            Err(error) => Err(error),
            Ok(()) => self.apply(engine),
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
        policy.store(&self.state_dir.join("policy"))?;
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

    /// Runs the automatic job if no operation is running (the daemon's timer calls this).
    pub fn tick(&self) -> Result<(), UpdateError> {
        match self.accept("", &Job::Automatic) {
            Ok(()) => self.run(Job::Automatic),
            Err(_) => Ok(()),
        }
    }
}
