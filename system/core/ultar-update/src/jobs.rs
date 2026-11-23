//! Running the service's long operations: check, download, install, offline staging and the
//! automatic run. Further `impl` block of [`Core`].

use crate::bootctl::BootControl;
use crate::contract::Phase;
use crate::engine::{Engine, EnginePhase, Offer};
use crate::error::UpdateError;
use crate::fsutil::{read_bounded, write_atomic};
use crate::policy::real_pacer;
use crate::service::{lock, Core, Job};
use crate::source::UpdateSource;

/// A verified offer together with the source it came from.
type Found = (Offer, Box<dyn UpdateSource + Send>);

/// After a failed automatic run the next one is tried after at most this many seconds.
pub const RETRY_AFTER: u64 = 15 * 60;

/// What an automatic run did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Automatic {
    /// The policy does not allow automatic work now (switched off, window, metered).
    Blocked,
    /// The run took place (whether or not there was something to install).
    Ran,
}

impl<B: BootControl> Core<B> {
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
            Job::Automatic => {
                let outcome = self.automatic(engine);
                self.schedule_next(&outcome);
                outcome.map(|_| ())
            }
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

    /// A manual download. The caller's "use the metered connection" counts only for an
    /// administrator; for everyone else the policy decides. A complete staged bundle of
    /// another release is replaced only for an administrator.
    fn download(&self, engine: &mut Engine<B>, allow_metered: bool) -> Result<(), UpdateError> {
        // Every accepted job announces a busy phase first, so that a client that follows the
        // progress topic always sees a beginning and an end – also when the job is refused.
        self.start_phase(Phase::Checking);
        let privileged = lock(&self.guard).privileged;
        let unmetered = lock(&self.snapshot).metered == Some(false);
        let consent = (allow_metered && privileged) || engine.policy().allow_metered;
        if !unmetered && !consent {
            return Err(UpdateError::Metered);
        }
        let (offer, source) = self.check(engine)?.ok_or(UpdateError::NoUpdate)?;
        let other_staged = engine
            .staged()?
            .is_some_and(|staged| staged.digest != offer.verified.digest());
        if other_staged && !privileged {
            return Err(UpdateError::PermissionDenied);
        }
        self.fetch(engine, &offer, source)
    }

    fn fetch(
        &self,
        engine: &mut Engine<B>,
        offer: &Offer,
        mut source: Box<dyn UpdateSource + Send>,
    ) -> Result<(), UpdateError> {
        self.start_phase(Phase::Downloading);
        let mut pacer = real_pacer(engine.policy().rate_limit_kib);
        engine.download(&mut *source, offer, &mut pacer, &mut |done, total| {
            self.advance(done, total)
        })?;
        Ok(())
    }

    fn apply(&self, engine: &mut Engine<B>) -> Result<(), UpdateError> {
        self.start_phase(Phase::Applying);
        engine.apply(&mut |_, done, total| self.advance(done, total))?;
        Ok(())
    }

    /// What the device does by itself: install what is staged, else check, download, install.
    /// It never installs the release the device has given up (see [`crate::reject`]), and a
    /// staged bundle that can no longer be installed does not stop it.
    fn automatic(&self, engine: &mut Engine<B>) -> Result<Automatic, UpdateError> {
        let policy = engine.policy();
        let metered = lock(&self.snapshot).metered;
        if policy
            .automatic_allowed(engine.clock_now(), metered)
            .is_err()
        {
            return Ok(Automatic::Blocked);
        }
        match engine.status()?.phase {
            EnginePhase::RebootRequired | EnginePhase::TrialRunning => return Ok(Automatic::Ran),
            EnginePhase::Staged if self.install_staged(engine)? => return Ok(Automatic::Ran),
            _ => {}
        }
        let Some((offer, source)) = self.check(engine)? else {
            return Ok(Automatic::Ran);
        };
        if engine.is_rejected(&offer.verified.digest()) {
            // Still offered, but this device has already given it up: wait for another one.
            self.remember(None);
            return Ok(Automatic::Ran);
        }
        self.fetch(engine, &offer, source)?;
        self.apply(engine)?;
        Ok(Automatic::Ran)
    }

    /// Handles a staged bundle in the automatic run. `Ok(true)`: nothing more to do in this
    /// run (installed, or left for the administrator). `Ok(false)`: the bundle was refused
    /// and dropped – go on and look for another release.
    fn install_staged(&self, engine: &mut Engine<B>) -> Result<bool, UpdateError> {
        let staged = engine.staged()?;
        if staged.is_some_and(|info| engine.is_rejected(&info.digest)) {
            return Ok(true);
        }
        match self.apply(engine) {
            Ok(()) => Ok(true),
            Err(_) if engine.staged()?.is_none() => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn next_check_path(&self) -> std::path::PathBuf {
        self.state_dir.join("next_check")
    }

    /// Stores when the next automatic run is due: a full interval after a run, a short retry
    /// time after a failure, unchanged when the policy blocked the run (it is then tried at
    /// every wake-up, which costs no network traffic).
    fn schedule_next(&self, outcome: &Result<Automatic, UpdateError>) {
        let interval = lock(&self.guard).interval;
        let wait = match outcome {
            Ok(Automatic::Blocked) => return,
            Ok(Automatic::Ran) => interval,
            Err(_) => interval.min(RETRY_AFTER),
        };
        let due = self.clock.now_unix().saturating_add(wait);
        let _ = write_atomic(&self.next_check_path(), format!("{due}\n").as_bytes());
    }

    /// True when an automatic run is due: none is on record, the recorded time has come, or
    /// the record lies further ahead than one interval (clock was set back, or damage).
    pub fn automatic_due(&self, interval_secs: u64) -> bool {
        let recorded = read_bounded(&self.next_check_path(), 32)
            .ok()
            .flatten()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| text.trim().parse::<u64>().ok());
        let now = self.clock.now_unix();
        recorded.is_none_or(|due| now >= due || due - now > interval_secs)
    }

    /// Runs the automatic job now if no operation is running.
    pub fn tick(&self) -> Result<(), UpdateError> {
        match self.accept("", &Job::Automatic) {
            Ok(()) => self.run(Job::Automatic),
            Err(_) => Ok(()),
        }
    }

    /// The daemon's timer calls this often (every few minutes): the automatic job runs only
    /// when it is due. The due time is stored, so a device that is never up for a whole
    /// interval still checks – at its next start after the interval has passed.
    pub fn tick_if_due(&self, interval_secs: u64) -> Result<(), UpdateError> {
        lock(&self.guard).interval = interval_secs;
        if self.automatic_due(interval_secs) {
            self.tick()
        } else {
            Ok(())
        }
    }
}
