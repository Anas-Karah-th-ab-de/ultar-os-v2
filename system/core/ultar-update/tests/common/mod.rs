//! Shared test kit: a publisher that builds signed bundles, and a simulated device.
#![allow(dead_code)]

pub mod rig;

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;
use ultar_update::bootctl::{BootControl, Slot};
use ultar_update::bundle::{write_bundle, Header};
use ultar_update::clock::ManualClock;
use ultar_update::engine::{Engine, EngineConfig, Environment};
use ultar_update::error::{NetworkError, UpdateError};
use ultar_update::fault::{Checkpoint, InterruptAfter};
use ultar_update::filectl::FileBootControl;
use ultar_update::health::{FixedSpace, HealthCheck};
use ultar_update::keys::KEY_FILE_SUFFIX;
use ultar_update::manifest::{Manifest, Payload, Role};
use ultar_update::sign::{public_of, seal, MANIFEST_DOMAIN, MANIFEST_MAGIC};
use ultar_update::source::{Query, Remote, UpdateSource};
use ultar_update::verify::DeviceIdentity;
use ultar_update::version::Version;

pub const PUBLISHER: [u8; 32] = [0x51; 32];
pub const STRANGER: [u8; 32] = [0x52; 32];
pub const NOW: u64 = 1_796_000_000;
pub const ROLES: [Role; 2] = [Role::Kernel, Role::System];
pub const KERNEL_LEN: usize = 100 * 1024 + 7;
pub const SYSTEM_LEN: usize = 200 * 1024 + 3;

/// Deterministic pseudo-random bytes (xorshift64*), fixed by `seed`.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    pub fn bytes(&mut self, len: usize) -> Vec<u8> {
        (0..len).map(|_| self.next() as u8).collect()
    }
}

pub fn sha(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

/// A signed release with its bundle bytes.
#[derive(Clone)]
pub struct Release {
    pub manifest: Manifest,
    pub signed: Vec<u8>,
    pub header: Header,
    pub payloads: Vec<Vec<u8>>,
    pub bundle: Vec<u8>,
}

pub fn sign_manifest(manifest: &Manifest, secret: &[u8; 32]) -> Vec<u8> {
    seal(
        &MANIFEST_MAGIC,
        MANIFEST_DOMAIN,
        &manifest.encode().unwrap(),
        secret,
    )
    .unwrap()
}

pub fn assemble(
    manifest: Manifest,
    payloads: Vec<Vec<u8>>,
    rotations: &[Vec<u8>],
    secret: &[u8; 32],
) -> Release {
    let signed = sign_manifest(&manifest, secret);
    let header = Header::new(rotations, &signed).unwrap();
    let mut bundle = Vec::new();
    let mut readers: Vec<&[u8]> = payloads.iter().map(Vec::as_slice).collect();
    let mut dyn_readers: Vec<&mut dyn Read> =
        readers.iter_mut().map(|r| r as &mut dyn Read).collect();
    write_bundle(&mut bundle, &header, &mut dyn_readers).unwrap();
    Release {
        manifest,
        signed,
        header,
        payloads,
        bundle,
    }
}

/// A release `version` with rollback index `index`; `edit` may change the manifest before it
/// is signed with `secret`.
pub fn release_with(
    version: (u32, u32, u32),
    index: u64,
    secret: &[u8; 32],
    edit: impl FnOnce(&mut Manifest),
) -> Release {
    let seed = u64::from(version.0) << 40
        | u64::from(version.1) << 20
        | u64::from(version.2)
        | 0x9E37_0000_0000_0000;
    let mut rng = Rng(seed);
    let payloads = vec![rng.bytes(KERNEL_LEN), rng.bytes(SYSTEM_LEN)];
    let mut manifest = Manifest {
        version: Version::new(version.0, version.1, version.2),
        rollback_index: index,
        channel: "stable".into(),
        edition: "desktop".into(),
        arch: "x86_64".into(),
        devices: vec!["pc-uefi".into()],
        min_version: Version::new(0, 7, 0),
        notes_key: "update.notes.test".into(),
        created: NOW - 1000,
        expires: NOW + 30 * 86_400,
        critical: false,
        payloads: vec![
            Payload {
                role: Role::Kernel,
                size: KERNEL_LEN as u64,
                sha256: sha(&payloads[0]),
                verity_root: None,
            },
            Payload {
                role: Role::System,
                size: SYSTEM_LEN as u64,
                sha256: sha(&payloads[1]),
                verity_root: Some([0xAB; 32]),
            },
        ],
    };
    edit(&mut manifest);
    assemble(manifest, payloads, &[], secret)
}

pub fn release(version: (u32, u32, u32), index: u64) -> Release {
    release_with(version, index, &PUBLISHER, |_| {})
}

/// A health check with a fixed verdict.
pub struct Health(pub bool);

impl HealthCheck for Health {
    fn check(&self) -> Result<(), UpdateError> {
        if self.0 {
            Ok(())
        } else {
            Err(UpdateError::Unhealthy)
        }
    }
}

/// A simulated device: state directory, key directory, file-based slots with a factory system
/// in slot A.
pub struct Device {
    pub dir: tempfile::TempDir,
    pub boot: FileBootControl,
    pub clock: Arc<ManualClock>,
    pub config: EngineConfig,
    pub factory: Vec<Vec<u8>>,
    pub free_space: u64,
}

impl Device {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let keys = dir.path().join("update-keys");
        std::fs::create_dir_all(&keys).unwrap();
        std::fs::write(
            keys.join(format!("release{KEY_FILE_SUFFIX}")),
            hex::encode(public_of(&PUBLISHER)),
        )
        .unwrap();
        let boot = FileBootControl::create(&dir.path().join("boot"), &ROLES).unwrap();
        let mut rng = Rng(0xFAC7_0127);
        let factory = vec![rng.bytes(5000), rng.bytes(9000)];
        for (role, bytes) in ROLES.iter().zip(&factory) {
            let path = boot.image_path(Slot::A, *role);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, bytes).unwrap();
        }
        let config = EngineConfig {
            state_dir: dir.path().join("state"),
            keys_dir: keys,
            device: DeviceIdentity {
                arch: "x86_64".into(),
                edition: "desktop".into(),
                device: "pc-uefi".into(),
            },
            factory_version: Version::new(0, 7, 0),
            factory_rollback_index: 1,
            trial_tries: 3,
            reserve_bytes: 1024,
            expiry_tolerance: 86_400,
        };
        Self {
            dir,
            boot,
            clock: Arc::new(ManualClock::new(NOW)),
            config,
            factory,
            free_space: u64::MAX,
        }
    }

    pub fn engine_with(
        &self,
        control: Arc<dyn Checkpoint>,
    ) -> Result<Engine<FileBootControl>, UpdateError> {
        let env = Environment {
            clock: self.clock.clone(),
            space: Arc::new(FixedSpace(self.free_space)),
            control,
        };
        Engine::open(self.config.clone(), env, self.boot.clone())
    }

    pub fn engine(&self) -> Engine<FileBootControl> {
        self.engine_with(Arc::new(InterruptAfter::never())).unwrap()
    }

    pub fn images(&self, slot: Slot) -> Vec<Vec<u8>> {
        ROLES
            .iter()
            .map(|role| std::fs::read(self.boot.image_path(slot, *role)).unwrap_or_default())
            .collect()
    }

    /// "Restarts" the device: lets the boot code choose, and checks that the chosen slot holds
    /// exactly one complete known system. Returns the slot and the index of the matching
    /// candidate (0 = factory, then `releases` in order).
    pub fn reboot(&self, releases: &[&Release]) -> (Slot, usize) {
        let slot = self
            .boot
            .simulate_boot()
            .expect("a slot must always be startable");
        let mut candidates = vec![&self.factory];
        candidates.extend(releases.iter().map(|r| &r.payloads));
        let system_in = |slot: Slot| {
            let images = self.images(slot);
            candidates
                .iter()
                .position(|c| **c == images)
                .unwrap_or_else(|| panic!("slot {slot:?} holds an incomplete or unknown system"))
        };
        // Not only the slot that starts now: every slot the boot code *could* start (fallback,
        // "previous version") must be complete at all times.
        let other = slot.other();
        if self.boot.slot_state(other).unwrap().bootable {
            system_in(other);
        }
        (slot, system_in(slot))
    }

    pub fn slot_b_untouched(&self) -> bool {
        !self.boot.image_path(Slot::B, Role::Kernel).exists()
            && !self.boot.image_path(Slot::B, Role::System).exists()
            && !self.boot.slot_state(Slot::B).unwrap().bootable
    }

    pub fn write_bundle(&self, name: &str, release: &Release) -> PathBuf {
        let dir = self.dir.path().join("usb");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, &release.bundle).unwrap();
        path
    }
}

/// A source that serves one bundle from memory and can misbehave.
pub struct MemSource {
    pub bundle: Vec<u8>,
    /// Honour offsets (like a server with Range support).
    pub resumable: bool,
    /// Serve at most this many bytes per opened stream, then end it.
    pub cut_after: Option<usize>,
    /// Offsets of all `open` calls.
    pub opens: Vec<u64>,
    /// Answer `latest` with nothing.
    pub empty: bool,
}

impl MemSource {
    pub fn new(bundle: &[u8]) -> Self {
        Self {
            bundle: bundle.to_vec(),
            resumable: true,
            cut_after: None,
            opens: Vec::new(),
            empty: false,
        }
    }
}

impl UpdateSource for MemSource {
    fn latest(&mut self, _query: &Query<'_>) -> Result<Option<Remote>, UpdateError> {
        Ok((!self.empty).then(|| Remote {
            locator: "mem".into(),
            size_hint: self.bundle.len() as u64,
        }))
    }

    fn open(
        &mut self,
        _remote: &Remote,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<(u64, Box<dyn Read + Send>), UpdateError> {
        self.opens.push(offset);
        let start = if self.resumable { offset as usize } else { 0 };
        if start > self.bundle.len() {
            return Err(UpdateError::Network(NetworkError::Status(416)));
        }
        let mut data = self.bundle[start..].to_vec();
        // The header probe (offset 0) is never cut short; only body streams are.
        if let (Some(cut), true) = (self.cut_after, limit.is_none()) {
            data.truncate(cut);
        }
        if let Some(limit) = limit {
            data.truncate(limit as usize);
        }
        Ok((start as u64, Box::new(std::io::Cursor::new(data))))
    }

    fn is_network(&self) -> bool {
        true
    }
}

/// Check + download + apply in one go.
pub fn install(engine: &mut Engine<FileBootControl>, release: &Release) -> Result<(), UpdateError> {
    let mut source = MemSource::new(&release.bundle);
    let offer = engine
        .check(&mut source, "stable")?
        .ok_or(UpdateError::NoUpdate)?;
    engine.download(
        &mut source,
        &offer,
        &mut ultar_update::stage::NoPacing,
        &mut |_, _| {},
    )?;
    engine.apply(&mut |_, _, _| {})?;
    Ok(())
}
