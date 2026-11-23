//! Helpers shared by the integration tests of the log service.
#![allow(dead_code)]

pub mod world;

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ultar_logd::chain::Chain;
use ultar_logd::clock::ManualClock;
use ultar_logd::core::{Core, CoreConfig};
use ultar_logd::storage::MemStorage;
use ultar_logd::store::{FsyncPolicy, StoreConfig};
use ultar_service::log::wire::{decode_body, encode_body};
use ultar_service::log::{Level, OwnedRecord, RateLimit, Record};

/// Upper bound for anything that should happen at once.
pub const WAIT: Duration = Duration::from_secs(30);
/// Wall time the manual clocks start at (2027-01-15).
pub const START_MS: u64 = 1_800_000_000_000;

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn temp_dir(tag: &str) -> PathBuf {
    let number = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ulogd-{}-{number}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A record as it arrives from a client.
pub fn owned(record: &Record) -> OwnedRecord {
    decode_body(&encode_body(record).unwrap()).unwrap()
}

/// A typical record with a running number.
pub fn numbered(n: u64) -> OwnedRecord {
    owned(
        &Record::new(Level::Info, "test.event")
            .count("n", n)
            .word("phase", "run"),
    )
}

/// Small limits that make rotation and retention happen quickly.
pub fn small_config() -> CoreConfig {
    CoreConfig {
        ring_records: 64,
        ring_bytes: 32 * 1024,
        rate: RateLimit {
            burst: u32::MAX,
            per_second: 0,
        },
        max_sources: 8,
        store: StoreConfig {
            segment_bytes: 8 * 1024,
            total_bytes: 32 * 1024,
            max_age_ms: 0,
            fsync: FsyncPolicy::Always,
            retry_every: 4,
        },
    }
}

pub struct Rig {
    pub core: Core,
    pub disk: MemStorage,
    pub clock: Arc<ManualClock>,
    pub chain: Chain,
    pub config: CoreConfig,
}

impl Rig {
    pub fn new(config: CoreConfig, chain: Chain) -> Self {
        Self::on(MemStorage::with_capacity(u64::MAX / 2), config, chain)
    }

    pub fn on(disk: MemStorage, config: CoreConfig, chain: Chain) -> Self {
        let clock = Arc::new(ManualClock::at(START_MS));
        let core = Core::open(config, Box::new(disk.clone()), chain.clone(), clock.clone());
        Self {
            core,
            disk,
            clock,
            chain,
            config,
        }
    }

    /// Closes the core and opens the same disk again (a restart).
    pub fn reopen(self) -> Self {
        let Rig {
            core,
            disk,
            clock,
            chain,
            config,
        } = self;
        drop(core);
        let core = Core::open(config, Box::new(disk.clone()), chain.clone(), clock.clone());
        Self {
            core,
            disk,
            clock,
            chain,
            config,
        }
    }

    /// Bytes of all segment files on the disk.
    pub fn segment_bytes(&self) -> u64 {
        self.disk
            .names()
            .iter()
            .filter(|name| name.starts_with("seg-"))
            .map(|name| self.disk.file(name).unwrap().len() as u64)
            .sum()
    }

    pub fn segments(&self) -> Vec<String> {
        self.disk
            .names()
            .into_iter()
            .filter(|name| name.starts_with("seg-"))
            .collect()
    }
}
