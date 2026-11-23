//! The update engine: trust, verification, staging and the view on the device's state.
//!
//! The engine is synchronous and owns no threads. Installing is in [`crate::apply`], what
//! happens after a restart in [`crate::commit`]; both are further `impl` blocks of [`Engine`].

use crate::bootctl::{BootControl, Slot};
use crate::bundle::Header;
use crate::clock::{Clock, TimeFloor};
use crate::error::UpdateError;
use crate::fault::Checkpoint;
use crate::fsutil::{check_private_dir, create_private_dir};
use crate::health::SpaceProbe;
use crate::history::{self, HistoryEntry, HistoryKind};
use crate::journal::Journal;
use crate::keys::TrustStore;
use crate::manifest::{Manifest, MAX_MANIFEST};
use crate::monotonic::MonotonicStore;
use crate::policy::Policy;
use crate::sign;
use crate::source::Remote;
use crate::stage::Staging;
use crate::verify::{self, Admission, DeviceIdentity, VerifiedManifest};
use crate::version::Version;
use std::path::PathBuf;
use std::sync::Arc;

/// Fixed facts about the device and where the updater keeps its files.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// The updater's state directory (root only).
    pub state_dir: PathBuf,
    /// Directory of the burned-in update keys (part of the system image).
    pub keys_dir: PathBuf,
    /// What this device is.
    pub device: DeviceIdentity,
    /// Version of the running image when no signed manifest is stored for its slot.
    pub factory_version: Version,
    /// Rollback index the running image was built with (lower bound of the floor).
    pub factory_rollback_index: u64,
    /// How often the boot code tries a new slot before it falls back.
    pub trial_tries: u8,
    /// Free space that must remain after a download, in bytes.
    pub reserve_bytes: u64,
    /// Grace period after a manifest's expiry, in seconds.
    pub expiry_tolerance: u64,
    /// When the running image was built (seconds since 1970; 0 = unknown). A lower bound for
    /// "now" on devices whose clock is not set.
    pub image_time: u64,
}

/// Default free-space reserve: 64 MiB.
pub const DEFAULT_RESERVE: u64 = 64 * 1024 * 1024;
/// Default number of start attempts for a new slot.
pub const DEFAULT_TRIAL_TRIES: u8 = 3;

/// The engine's connections to the outside world (all replaceable in tests).
#[derive(Clone)]
pub struct Environment {
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// Free-space probe.
    pub space: Arc<dyn SpaceProbe>,
    /// Stop points: cancel flag in production, fault injection in tests.
    pub control: Arc<dyn Checkpoint>,
}

/// A release a source offers and this device may install.
#[derive(Debug, Clone)]
pub struct Offer {
    /// The channel that was asked for; `None` for an offline bundle.
    pub channel: Option<String>,
    /// Where the bundle is.
    pub remote: Remote,
    /// The bundle header as fetched.
    pub header: Header,
    /// The verified manifest.
    pub verified: VerifiedManifest,
}

impl Offer {
    /// Exact size of the bundle: header plus signed payload sizes.
    pub fn bundle_size(&self) -> u64 {
        self.header.len() + self.verified.manifest().payload_bytes()
    }
}

/// Where the device stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnginePhase {
    /// Nothing pending.
    Idle,
    /// A download was started and can be continued.
    Partial,
    /// A complete verified bundle waits to be installed.
    Staged,
    /// A release is installed in the other slot; a restart starts it.
    RebootRequired,
    /// The running system is a new release that has not been confirmed yet.
    TrialRunning,
}

/// Snapshot of the engine's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineStatus {
    /// Running version.
    pub current_version: Version,
    /// Running slot.
    pub active_slot: Slot,
    /// Phase.
    pub phase: EnginePhase,
    /// Version that is staged or waits for the restart.
    pub pending_version: Option<Version>,
    /// Stored rollback floor.
    pub rollback_floor: u64,
    /// The previous version can be started.
    pub previous_startable: bool,
    /// Version in the other slot, when known.
    pub previous_version: Option<Version>,
    /// Serial of the trusted key set.
    pub key_serial: u64,
}

/// The update engine over a boot control backend.
pub struct Engine<B: BootControl> {
    pub(crate) config: EngineConfig,
    pub(crate) env: Environment,
    pub(crate) boot: B,
    pub(crate) trust: TrustStore,
    pub(crate) floor: MonotonicStore,
    pub(crate) time: TimeFloor,
    pub(crate) staging: Staging,
}

impl<B: BootControl> Engine<B> {
    /// Opens the engine: creates the state directories, loads and verifies the trust state,
    /// and tidies up after an interrupted slot write (the slot stays unbootable).
    pub fn open(config: EngineConfig, env: Environment, boot: B) -> Result<Self, UpdateError> {
        let state = config.state_dir.join("state");
        for dir in [state.clone(), config.state_dir.join("slots")] {
            create_private_dir(&dir)?;
        }
        // Everything below is trusted because nobody else can write here.
        check_private_dir(&config.state_dir).map_err(|e| match e.kind() {
            std::io::ErrorKind::PermissionDenied => UpdateError::StateCorrupt,
            _ => UpdateError::Io(e),
        })?;
        let trust = TrustStore::open(&config.keys_dir, &config.state_dir)?;
        let mut engine = Self {
            floor: MonotonicStore::new(&state, "rollback"),
            time: TimeFloor::new(&state),
            staging: Staging::new(&config.state_dir.join("staging")),
            trust,
            config,
            env,
            boot,
        };
        engine.recover_interrupted_write()?;
        engine.record_factory_slot()?;
        Ok(engine)
    }

    /// The boot control backend.
    pub fn boot(&self) -> &B {
        &self.boot
    }

    /// The configuration the engine was opened with.
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    pub(crate) fn journal_path(&self) -> PathBuf {
        self.config.state_dir.join("journal")
    }

    fn history_path(&self) -> PathBuf {
        self.config.state_dir.join("history")
    }

    fn policy_path(&self) -> PathBuf {
        self.config.state_dir.join("policy")
    }

    pub(crate) fn slot_manifest_path(&self, slot: Slot) -> PathBuf {
        let name = format!("{}.manifest", slot.letter());
        self.config.state_dir.join("slots").join(name)
    }

    pub(crate) fn journal(&self) -> Result<Journal, UpdateError> {
        Ok(Journal::load(&self.journal_path())?.0)
    }

    /// The manifest stored for `slot` when the slot was written. Its signature was verified at
    /// that time against the keys then in force; it is deliberately **not** verified again
    /// against today's keys, so that retiring a signing key does not make the device forget
    /// which version and rollback index its own slots hold. (The file lives in the updater's
    /// root-only state directory, like the rollback floor itself.)
    pub fn slot_manifest(&self, slot: Slot) -> Option<Manifest> {
        let path = self.slot_manifest_path(slot);
        let bytes = crate::fsutil::read_bounded(&path, 32 * 1024)
            .ok()
            .flatten()?;
        let envelope = sign::open_exact(&sign::MANIFEST_MAGIC, &bytes, MAX_MANIFEST).ok()?;
        Manifest::parse(envelope.body).ok()
    }

    fn factory_record_path(&self, slot: Slot) -> PathBuf {
        let name = format!("{}.factory", slot.letter());
        self.config.state_dir.join("slots").join(name)
    }

    /// Remembers version and rollback index of a running, confirmed slot that has no signed
    /// manifest (the factory installation), so they are still known when another slot runs.
    fn record_factory_slot(&self) -> Result<(), UpdateError> {
        let active = self.boot.active_slot()?;
        let path = self.factory_record_path(active);
        let known = self.slot_manifest(active).is_some() || path.exists();
        if !known && self.boot.slot_state(active)?.successful {
            let text = format!(
                "{} {}\n",
                self.config.factory_version, self.config.factory_rollback_index
            );
            crate::fsutil::write_atomic(&path, text.as_bytes())?;
        }
        Ok(())
    }

    /// Forgets what was known about `slot` (before it is rewritten).
    pub(crate) fn forget_slot(&self, slot: Slot) -> Result<(), UpdateError> {
        crate::fsutil::remove_if_present(&self.factory_record_path(slot))?;
        let manifest = self.slot_manifest_path(slot);
        crate::fsutil::remove_if_present(&manifest)?;
        Ok(crate::fsutil::sync_parent(&manifest)?)
    }

    /// Version and rollback index of what is in `slot`: from its signed manifest, else from
    /// the factory record, else unknown.
    pub fn slot_facts(&self, slot: Slot) -> Option<(Version, u64)> {
        if let Some(m) = self.slot_manifest(slot) {
            return Some((m.version, m.rollback_index));
        }
        let path = self.factory_record_path(slot);
        let bytes = crate::fsutil::read_bounded(&path, 64).ok().flatten()?;
        let text = String::from_utf8(bytes).ok()?;
        let (version, index) = text.strip_suffix('\n')?.split_once(' ')?;
        Some((Version::parse(version)?, index.parse().ok()?))
    }

    /// Rollback index of what is in `slot`; unknown content counts as 0.
    pub(crate) fn slot_index(&self, slot: Slot) -> u64 {
        self.slot_facts(slot).map_or(0, |(_, index)| index)
    }

    /// Version of the running system.
    pub fn current_version(&self) -> Result<Version, UpdateError> {
        let active = self.boot.active_slot()?;
        Ok(self
            .slot_facts(active)
            .map_or(self.config.factory_version, |(version, _)| version))
    }

    /// What the device's clock shows (for the daily window of automatic updates; expiry is
    /// judged with [`Engine::now`]).
    pub fn clock_now(&self) -> u64 {
        self.env.clock.now_unix()
    }

    /// The time expiry is judged with: the clock, but never before the stored time floor
    /// and never before the build time of the running image.
    pub fn now(&self) -> Result<u64, UpdateError> {
        let now = self.time.effective_now(&*self.env.clock)?;
        Ok(now.max(self.config.image_time))
    }

    /// The engine's time source (shared with the service for rate limits and timers).
    pub fn clock(&self) -> Arc<dyn Clock> {
        self.env.clock.clone()
    }

    /// The rollback floor in force: the largest of the stored value, the platform's value
    /// (firmware/TPM, when the boot control has one) and the running image's own index.
    pub fn rollback_floor(&self) -> Result<u64, UpdateError> {
        let stored = self.floor.get()?;
        let firmware = self.boot.firmware_rollback_index()?.unwrap_or(0);
        Ok(stored.max(firmware).max(self.config.factory_rollback_index))
    }

    /// Appends to the history; a failure to write the history never fails an operation.
    pub(crate) fn log(&self, kind: HistoryKind, version: Option<Version>, detail: Option<&str>) {
        let entry = HistoryEntry {
            time: self.env.clock.now_unix(),
            kind,
            version,
            detail_key: detail.map(str::to_string),
        };
        let _ = history::append(&self.history_path(), &entry);
    }

    /// The history, oldest entry first.
    pub fn history(&self) -> Vec<HistoryEntry> {
        history::load(&self.history_path())
    }

    /// The stored policy (see [`Policy::load`] for missing and damaged files).
    pub fn policy(&self) -> Policy {
        Policy::load(&self.policy_path()).0
    }

    /// Validates and stores a new policy. A bundle that was fetched for another channel than
    /// the new one is dropped from the staging area.
    pub fn set_policy(&self, policy: &Policy) -> Result<(), UpdateError> {
        policy.store(&self.policy_path())?;
        let other_channel = self
            .staging
            .staged()?
            .and_then(|info| info.channel)
            .is_some_and(|channel| channel != policy.channel);
        if other_channel {
            self.staging.purge()?;
        }
        Ok(())
    }

    /// Full verification of a bundle header: rotations are offered to the trust store, the
    /// manifest is authenticated, the time floor learns its creation time, and the manifest is
    /// judged for this device. `channel`: the channel asked for, `None` for offline bundles.
    pub fn inspect(
        &mut self,
        header: &Header,
        channel: Option<&str>,
        allow_downgrade: bool,
    ) -> Result<VerifiedManifest, UpdateError> {
        for rotation in &header.rotations {
            if self.trust.accept(rotation)? {
                self.log(HistoryKind::KeysRotated, None, None);
            }
        }
        let authentic = verify::authenticate(&header.signed_manifest, self.trust.keys())?;
        let created = authentic.manifest().created;
        let roles = self.boot.roles();
        let facts = Admission {
            device: &self.config.device,
            roles: &roles,
            floor: self.rollback_floor()?,
            current: self.current_version()?,
            now: self.now()?,
            tolerance: self.config.expiry_tolerance,
            channel,
            allow_downgrade,
        };
        let verified = verify::admit(authentic, &facts)?;
        // Only a manifest this device accepts teaches it the time: a release for another
        // board (or a publisher's slip in one) must not be able to expire everything here.
        self.time.witness(created)?;
        Ok(verified)
    }

    /// True when the other slot may be started by hand: bootable, proven, not below the
    /// rollback floor, and no installation is pending.
    pub fn previous_startable(&self) -> Result<bool, UpdateError> {
        let other = self.boot.active_slot()?.other();
        let state = self.boot.slot_state(other)?;
        Ok(state.bootable
            && state.successful
            && self.journal()? == Journal::Idle
            && self.slot_index(other) >= self.rollback_floor()?)
    }

    /// Where the device stands.
    pub fn status(&self) -> Result<EngineStatus, UpdateError> {
        let active = self.boot.active_slot()?;
        let other = active.other();
        let version_of = |slot| self.slot_facts(slot).map(|(version, _)| version);
        let (phase, pending_version) = match self.journal()? {
            Journal::Trial { slot, .. }
                if slot == other && self.boot.slot_state(other)?.bootable =>
            {
                (EnginePhase::RebootRequired, version_of(other))
            }
            _ if !self.boot.slot_state(active)?.successful => {
                (EnginePhase::TrialRunning, version_of(active))
            }
            _ if self.staging.staged()?.is_some() => (EnginePhase::Staged, self.staged_version()),
            _ if self.staging.partial_bytes() > 0 => (EnginePhase::Partial, None),
            _ => (EnginePhase::Idle, None),
        };
        Ok(EngineStatus {
            current_version: self.current_version()?,
            active_slot: active,
            phase,
            pending_version,
            rollback_floor: self.rollback_floor()?,
            previous_startable: self.previous_startable()?,
            previous_version: version_of(other),
            key_serial: self.trust.serial(),
        })
    }
}
