//! The file-backed [`ChoiceStore`] of the daemon: choices, roll-back targets and the
//! quarantine in one file `state.toml` inside the state directory.
//!
//! File layout (version 1):
//!
//! ```text
//! # ultar-devd state v1 sha256=<64 hex digits> bytes=<length of the body>
//! <body: StoredState::to_toml()>
//! ```
//!
//! * **Atomic:** the new content is written to `state.toml.tmp`, synced, renamed over
//!   `state.toml`, and the directory is synced. A power loss leaves the old or the new
//!   file, never a mixture; a left-over temporary file is never read.
//! * **Checked:** length and SHA-256 of the body are verified on load, so a truncated or
//!   bit-flipped file is detected even when it still parses as TOML.
//! * **Recovery:** a file that fails any check is moved to `state.toml.corrupt` (one
//!   generation is kept for diagnosis), the daemon starts with an empty state and a
//!   [`StoreEvent`] reports what happened. A newer format version is handled the same way
//!   with its own problem code.

use super::StoredState;
use crate::traits::{ChoiceStore, StoreError};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use ultar_driverpkg::keys::sha256_hex;

/// Name of the state file inside the state directory.
pub const STATE_FILE: &str = "state.toml";
/// Name of the temporary file used while writing.
pub const TEMP_FILE: &str = "state.toml.tmp";
/// Name under which a rejected state file is kept.
pub const CORRUPT_FILE: &str = "state.toml.corrupt";
/// Version of the file layout (header); the body carries its own `format` number.
pub const FILE_VERSION: u32 = 1;
/// Largest accepted state file in bytes.
pub const MAX_STATE_BYTES: u64 = 4 * 1024 * 1024;
/// Most events kept until [`FileStore::take_events`] is called.
pub const MAX_EVENTS: usize = 16;

const HEADER_PREFIX: &str = "# ultar-devd state v";

/// What was wrong with a state file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreProblem {
    /// The header line is missing or malformed.
    Header,
    /// The body is shorter or longer than the header says (truncated file).
    Length,
    /// The checksum of the body does not match.
    Checksum,
    /// Header and checksum are fine but the body is not a valid state.
    Body,
    /// The file was written by a newer daemon.
    UnsupportedVersion(u32),
    /// The file is larger than [`MAX_STATE_BYTES`].
    TooLarge,
}

impl StoreProblem {
    /// Stable message key.
    pub fn key(self) -> &'static str {
        match self {
            Self::Header => "devd.store.problem.header",
            Self::Length => "devd.store.problem.length",
            Self::Checksum => "devd.store.problem.checksum",
            Self::Body => "devd.store.problem.body",
            Self::UnsupportedVersion(_) => "devd.store.problem.version",
            Self::TooLarge => "devd.store.problem.too-large",
        }
    }
}

/// Something the store did on its own and the operator should learn about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreEvent {
    /// The state file was unusable; the daemon continued with an empty state.
    Recovered {
        /// What was wrong.
        problem: StoreProblem,
        /// `true` when the rejected file was kept as [`CORRUPT_FILE`].
        kept: bool,
    },
}

/// Renders the complete file content for a state.
pub fn encode(state: &StoredState) -> Result<String, StoreError> {
    let body = state.to_toml()?;
    Ok(format!(
        "{HEADER_PREFIX}{FILE_VERSION} sha256={} bytes={}\n{body}",
        sha256_hex(body.as_bytes()),
        body.len()
    ))
}

/// Parses and verifies a complete file content.
pub fn decode(bytes: &[u8]) -> Result<StoredState, StoreProblem> {
    let text = std::str::from_utf8(bytes).map_err(|_| StoreProblem::Header)?;
    let (header, body) = text.split_once('\n').ok_or(StoreProblem::Header)?;
    let mut fields = header
        .strip_prefix(HEADER_PREFIX)
        .ok_or(StoreProblem::Header)?
        .split(' ');
    let version: u32 = fields
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or(StoreProblem::Header)?;
    if version != FILE_VERSION {
        return Err(StoreProblem::UnsupportedVersion(version));
    }
    let sum = fields
        .next()
        .and_then(|f| f.strip_prefix("sha256="))
        .ok_or(StoreProblem::Header)?;
    let length: usize = fields
        .next()
        .and_then(|f| f.strip_prefix("bytes="))
        .and_then(|n| n.parse().ok())
        .ok_or(StoreProblem::Header)?;
    if fields.next().is_some() {
        return Err(StoreProblem::Header);
    }
    if body.len() != length {
        return Err(StoreProblem::Length);
    }
    if sha256_hex(body.as_bytes()) != sum {
        return Err(StoreProblem::Checksum);
    }
    StoredState::from_toml(body).map_err(|_| StoreProblem::Body)
}

/// The daemon's persistent store below a state directory.
#[derive(Debug)]
pub struct FileStore {
    dir: PathBuf,
    events: Vec<StoreEvent>,
    saves: u64,
}

impl FileStore {
    /// A store using `dir` (created on the first save).
    pub fn new(dir: &Path) -> Self {
        Self {
            dir: dir.to_path_buf(),
            events: Vec::new(),
            saves: 0,
        }
    }

    /// Path of the state file.
    pub fn path(&self) -> PathBuf {
        self.dir.join(STATE_FILE)
    }

    /// Events since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<StoreEvent> {
        std::mem::take(&mut self.events)
    }

    /// Events not yet taken.
    pub fn events(&self) -> &[StoreEvent] {
        &self.events
    }

    /// Number of successful saves.
    pub fn saves(&self) -> u64 {
        self.saves
    }

    fn record(&mut self, event: StoreEvent) {
        if self.events.len() >= MAX_EVENTS {
            self.events.remove(0);
        }
        self.events.push(event);
    }

    /// Moves the rejected file aside and notes the recovery.
    fn recover(&mut self, problem: StoreProblem) -> StoredState {
        let kept = fs::rename(self.path(), self.dir.join(CORRUPT_FILE)).is_ok();
        if !kept {
            // Not movable (read-only directory …): at least do not read it again as valid.
            let _ = fs::remove_file(self.path());
        }
        self.record(StoreEvent::Recovered { problem, kept });
        StoredState::default()
    }

    fn write_atomically(&self, content: &str) -> std::io::Result<()> {
        let temp = self.dir.join(TEMP_FILE);
        let mut file = File::create(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, self.path())?;
        // Make the rename itself durable.
        File::open(&self.dir)?.sync_all()
    }
}

impl ChoiceStore for FileStore {
    fn load(&mut self) -> Result<StoredState, StoreError> {
        let file = match File::open(self.path()) {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(StoredState::default())
            }
            Err(_) => return Err(StoreError::Unavailable),
        };
        let mut bytes = Vec::new();
        if file
            .take(MAX_STATE_BYTES + 1)
            .read_to_end(&mut bytes)
            .is_err()
        {
            return Err(StoreError::Unavailable);
        }
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Ok(self.recover(StoreProblem::TooLarge));
        }
        match decode(&bytes) {
            Ok(state) => Ok(state),
            Err(problem) => Ok(self.recover(problem)),
        }
    }

    fn save(&mut self, state: &StoredState) -> Result<(), StoreError> {
        let content = encode(state)?;
        if content.len() as u64 > MAX_STATE_BYTES {
            return Err(StoreError::WriteFailed);
        }
        fs::create_dir_all(&self.dir).map_err(|_| StoreError::Unavailable)?;
        match self.write_atomically(&content) {
            Ok(()) => {
                self.saves += 1;
                Ok(())
            }
            Err(_) => {
                let _ = fs::remove_file(self.dir.join(TEMP_FILE));
                Err(StoreError::WriteFailed)
            }
        }
    }
}
