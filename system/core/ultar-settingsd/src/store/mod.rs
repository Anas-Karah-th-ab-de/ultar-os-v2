//! The store: values persisted as TOML per scope, written atomically and
//! coalesced, with schema versions, migrations and quarantine of damaged
//! files.

mod disk;
mod format;
mod load;

pub use disk::{write_atomic, MAX_FILE_BYTES, QUARANTINE_DIR, QUARANTINE_KEEP, TEMP_SUFFIX};
pub use format::{crc32, decode, encode, ComboData, DecodeError, FileContent};

use crate::clock::Clock;
use crate::schema::{is_valid_segment, Registry};
use crate::value::Value;
use load::{load_file, Notices};
use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use ultar_bus::messages::settings::StoreNoticeKind;

/// Most notices collected while loading.
pub const MAX_NOTICES: usize = 64;
/// Most display combinations remembered; the least recently used goes first.
pub const MAX_COMBOS: usize = 64;
/// Most values one scope may hold.
pub const MAX_VALUES: usize = 8192;
/// Longest pause before a failed write is tried again.
pub const MAX_RETRY: Duration = Duration::from_secs(30);
/// Shortest pause before a failed write is tried again, so that a write
/// delay of zero cannot turn a full disk into a busy loop.
pub const MIN_RETRY: Duration = Duration::from_millis(50);

/// Something worth telling the user that happened to a settings file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    /// File name inside the settings directory.
    pub file: String,
    /// What happened.
    pub kind: StoreNoticeKind,
    /// The affected key, the copy that was put aside, or the versions.
    pub detail: String,
}

/// A failure of the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// The user name cannot be part of a file name.
    InvalidUser,
    /// A file or directory operation failed.
    Io {
        /// File name inside the settings directory (empty: the directory).
        file: String,
        /// Kind of the failure.
        kind: io::ErrorKind,
    },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::InvalidUser => f.write_str("invalid user name"),
            StoreError::Io { file, kind } => write!(f, "settings file {file:?}: {kind}"),
        }
    }
}

impl std::error::Error for StoreError {}

/// The files of the store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FileId {
    System = 0,
    User = 1,
    Displays = 2,
    Journal = 3,
}

/// Where a value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Slot<'a> {
    /// The device-wide file.
    System,
    /// The file of the current user.
    User,
    /// One display combination in the display file.
    Combo(&'a str),
}

/// How the store is set up.
#[derive(Clone, Debug)]
pub struct StoreConfig {
    /// Directory that holds the settings files.
    pub dir: PathBuf,
    /// Name of the user whose file is used (one key segment).
    pub user: String,
    /// Longest time a change waits for further changes before it is written.
    pub write_delay: Duration,
}

/// Counters of the store.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoreStats {
    /// Files written since the store was opened.
    pub writes: u64,
    /// Changes accepted since the store was opened.
    pub changes: u64,
    /// Writes that failed.
    pub failed_writes: u64,
}

/// Persistent values of all scopes.
pub struct Store {
    dir: PathBuf,
    names: [String; 4],
    files: [FileContent; 4],
    dirty: [bool; 3],
    clock: Arc<dyn Clock>,
    delay: Duration,
    retry: Duration,
    flush_at: Option<Duration>,
    stats: StoreStats,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store")
            .field("dir", &self.dir)
            .field("dirty", &self.dirty)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

fn io_error(file: &str) -> impl Fn(io::Error) -> StoreError + '_ {
    move |err| StoreError::Io {
        file: file.to_owned(),
        kind: err.kind(),
    }
}

impl Store {
    /// Opens the store: creates the directory, removes temporary files of
    /// an interrupted write and loads every file. Damaged files never make
    /// this fail; they are reported in the returned notices.
    pub fn open(
        config: StoreConfig,
        registry: &Registry,
        clock: Arc<dyn Clock>,
    ) -> Result<(Store, Vec<Notice>), StoreError> {
        if !is_valid_segment(&config.user) {
            return Err(StoreError::InvalidUser);
        }
        std::fs::create_dir_all(&config.dir).map_err(io_error(""))?;
        disk::remove_stale_temp(&config.dir).map_err(io_error(""))?;
        let names = [
            "system.toml".to_owned(),
            format!("user-{}.toml", config.user),
            "displays.toml".to_owned(),
            "pending.toml".to_owned(),
        ];
        let ids = [
            FileId::System,
            FileId::User,
            FileId::Displays,
            FileId::Journal,
        ];
        let mut notices = Vec::new();
        let mut files: [FileContent; 4] = Default::default();
        let mut dirty = [false; 3];
        for id in ids {
            let mut collector = Notices {
                file: &names[id as usize],
                list: &mut notices,
            };
            let loaded = load_file(&config.dir, id, registry, &mut collector);
            files[id as usize] = loaded.content;
            if let Some(flag) = dirty.get_mut(id as usize) {
                *flag = loaded.rewrite;
            }
        }
        let mut store = Store {
            dir: config.dir,
            names,
            files,
            dirty,
            clock,
            delay: config.write_delay,
            retry: config.write_delay,
            flush_at: None,
            stats: StoreStats::default(),
        };
        if store.dirty.contains(&true) {
            store.schedule();
        }
        Ok((store, notices))
    }

    fn schedule(&mut self) {
        if self.flush_at.is_none() {
            self.flush_at = Some(self.clock.now().saturating_add(self.delay));
        }
    }

    fn values(&self, slot: Slot<'_>) -> Option<&BTreeMap<String, Value>> {
        match slot {
            Slot::System => Some(&self.files[FileId::System as usize].values),
            Slot::User => Some(&self.files[FileId::User as usize].values),
            Slot::Combo(id) => self.files[FileId::Displays as usize]
                .combos
                .get(id)
                .map(|combo| &combo.values),
        }
    }

    fn values_mut(&mut self, slot: Slot<'_>) -> Option<(&mut BTreeMap<String, Value>, usize)> {
        match slot {
            Slot::System => Some((&mut self.files[0].values, 0)),
            Slot::User => Some((&mut self.files[1].values, 1)),
            Slot::Combo(id) => self.files[2]
                .combos
                .get_mut(id)
                .map(|combo| (&mut combo.values, 2)),
        }
    }

    /// The stored value, if one is stored.
    pub fn get(&self, slot: Slot<'_>, key: &str) -> Option<&Value> {
        self.values(slot)?.get(key)
    }

    /// Number of values stored in a slot.
    pub fn len(&self, slot: Slot<'_>) -> usize {
        self.values(slot).map_or(0, BTreeMap::len)
    }

    /// Whether a slot holds no values.
    pub fn is_empty(&self, slot: Slot<'_>) -> bool {
        self.len(slot) == 0
    }

    /// The keys stored in a slot, sorted.
    pub fn keys<'s>(&'s self, slot: Slot<'_>) -> impl Iterator<Item = &'s str> {
        self.values(slot)
            .into_iter()
            .flat_map(|values| values.keys().map(String::as_str))
    }

    fn touched(&mut self, file: usize) {
        if let Some(flag) = self.dirty.get_mut(file) {
            *flag = true;
        }
        self.stats.changes += 1;
        self.schedule();
    }

    /// Stores a value. Returns `false` when nothing changed: the same value
    /// was already stored, or the slot is an unknown display combination.
    pub fn set(&mut self, slot: Slot<'_>, key: &str, value: Value) -> bool {
        let Some((values, file)) = self.values_mut(slot) else {
            return false;
        };
        if values.get(key) == Some(&value) {
            return false;
        }
        values.insert(key.to_owned(), value);
        self.touched(file);
        true
    }

    /// Forgets a stored value. Returns whether one was stored.
    pub fn remove(&mut self, slot: Slot<'_>, key: &str) -> bool {
        let Some((values, file)) = self.values_mut(slot) else {
            return false;
        };
        if values.remove(key).is_none() {
            return false;
        }
        self.touched(file);
        true
    }

    /// The remembered data of a display combination.
    pub fn combo(&self, id: &str) -> Option<&ComboData> {
        self.files[FileId::Displays as usize].combos.get(id)
    }

    /// The display combination used last, if any is remembered.
    pub fn latest_combo(&self) -> Option<(&str, &ComboData)> {
        self.files[FileId::Displays as usize]
            .combos
            .iter()
            .max_by_key(|(_, combo)| combo.used)
            .map(|(id, combo)| (id.as_str(), combo))
    }

    /// Marks a display combination as the one in use, creating it when it
    /// is new (`true` is returned then). The least recently used
    /// combination is forgotten beyond [`MAX_COMBOS`]. A stored combination
    /// with the same identifier but other displays is replaced.
    pub fn use_combo(&mut self, id: &str, displays: &[String]) -> bool {
        let combos = &mut self.files[FileId::Displays as usize].combos;
        let newest = combos.values().map(|combo| combo.used).max().unwrap_or(0);
        let known = combos
            .get(id)
            .is_some_and(|combo| combo.displays == displays);
        if known
            && combos
                .get(id)
                .is_some_and(|c| c.used == newest && newest > 0)
        {
            return false;
        }
        let entry = combos.entry(id.to_owned()).or_default();
        if !known {
            entry.displays = displays.to_vec();
            entry.values.clear();
        }
        entry.used = newest.saturating_add(1);
        while combos.len() > MAX_COMBOS {
            let oldest = combos
                .iter()
                .min_by_key(|(_, combo)| combo.used)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(oldest) => combos.remove(&oldest),
                None => break,
            };
        }
        self.touched(FileId::Displays as usize);
        !known
    }

    /// Keys with a provisional change that was not confirmed yet.
    pub fn journal(&self) -> impl Iterator<Item = &str> {
        self.files[FileId::Journal as usize]
            .values
            .keys()
            .map(String::as_str)
    }

    fn write_journal(&mut self) -> Result<(), StoreError> {
        self.write_file(FileId::Journal as usize)
    }

    /// Notes, durably and at once, that `key` has a provisional change.
    pub fn journal_add(&mut self, key: &str) -> Result<(), StoreError> {
        let journal = &mut self.files[FileId::Journal as usize].values;
        if journal.insert(key.to_owned(), Value::Bool(true)).is_some() {
            return Ok(());
        }
        let result = self.write_journal();
        if result.is_err() {
            self.files[FileId::Journal as usize].values.remove(key);
        }
        result
    }

    /// Notes, durably and at once, that the keys are settled.
    pub fn journal_remove(&mut self, keys: &[String]) -> Result<(), StoreError> {
        let journal = &mut self.files[FileId::Journal as usize].values;
        let before = journal.len();
        for key in keys {
            journal.remove(key);
        }
        if journal.len() == before {
            return Ok(());
        }
        self.write_journal()
    }

    fn write_file(&mut self, file: usize) -> Result<(), StoreError> {
        let (Some(content), Some(name)) = (self.files.get(file), self.names.get(file)) else {
            return Ok(());
        };
        let text = encode(content);
        match write_atomic(&self.dir, name, text.as_bytes()) {
            Ok(()) => {
                self.stats.writes += 1;
                Ok(())
            }
            Err(err) => {
                self.stats.failed_writes += 1;
                Err(io_error(name)(err))
            }
        }
    }

    /// Writes every changed file now. Returns how many files were written.
    /// After a failure the changes stay queued and the next attempt is due
    /// after a pause that doubles from [`MIN_RETRY`] up to [`MAX_RETRY`].
    pub fn flush(&mut self) -> Result<usize, StoreError> {
        let mut written = 0;
        let mut failure = None;
        for file in 0..self.dirty.len() {
            if !self.dirty[file] {
                continue;
            }
            match self.write_file(file) {
                Ok(()) => {
                    self.dirty[file] = false;
                    written += 1;
                }
                Err(err) => failure = failure.or(Some(err)),
            }
        }
        match failure {
            None => {
                self.flush_at = None;
                self.retry = self.delay;
                Ok(written)
            }
            Some(err) => {
                let doubled = self.retry.saturating_mul(2).max(MIN_RETRY);
                self.retry = doubled.min(MAX_RETRY).max(self.delay);
                self.flush_at = Some(self.clock.now().saturating_add(self.retry));
                Err(err)
            }
        }
    }

    /// When the next write is due, if changes are waiting.
    pub fn next_deadline(&self) -> Option<Duration> {
        self.flush_at
    }

    /// Writes the waiting changes when their time has come.
    pub fn tick(&mut self) -> Result<usize, StoreError> {
        match self.flush_at {
            Some(at) if self.clock.now() >= at => self.flush(),
            _ => Ok(0),
        }
    }

    /// Whether changes are waiting to be written.
    pub fn is_dirty(&self) -> bool {
        self.dirty.contains(&true)
    }

    /// Counters since the store was opened.
    pub fn stats(&self) -> StoreStats {
        self.stats
    }

    /// The settings directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Schema version of the loaded data.
    pub fn schema(&self) -> u32 {
        self.files[FileId::System as usize].schema
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
