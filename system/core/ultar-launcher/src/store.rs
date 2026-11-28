//! The launcher's memory: which app has which user id, profile, grants and
//! verified manifest. Kept in its own directory (only the launcher can
//! read or write it), written atomically, read strictly.
//!
//! User ids come from a reserved range (`rootfs/etc/ultar/ids.toml`,
//! `[ranges] apps`). An app keeps its id for ever: uninstalling marks the
//! entry as not installed, a reinstall gets the same id back, and the
//! counter of the next id only grows – no id is ever given to another app.
//!
//! ```text
//! ultar-launcher-state 3
//! range 20000 49999
//! next 20002
//! app camera 20000 app.system 1 1.4.0 <sha-256 of the manifest> network,ui.capture <signer>
//! app acme.game 20001 app.standard 0 1.0 <sha-256> - <signer>
//! ```
//!
//! The signer is the full SHA-256 of the signing key (64 hex digits, format
//! 3). Files of format 2 hold the 16-digit short id; such an entry keeps it
//! until the app is next installed, which writes the full one (R2-2).

use crate::appid::AppId;
use crate::atomic;
use crate::perm::PermissionSet;
use crate::profile::ProfileName;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

/// First line of the state file.
const MAGIC: &str = "ultar-launcher-state 3";
/// First line of the format before full signer identities (still read).
const MAGIC_2: &str = "ultar-launcher-state 2";
/// Most apps the state holds (installed and uninstalled); `list` shows all
/// of them (`ultar_bus::messages::launcher::MAX_APPS` is the same number).
pub const MAX_APPS: usize = ultar_bus::messages::launcher::MAX_APPS;
/// Largest state file.
const MAX_STATE_BYTES: u64 = 1 << 20;
/// Name of the state file in the launcher's directory.
const STATE_FILE: &str = "apps.state";
/// Directory of the stored manifests.
const MANIFESTS: &str = "manifests";

/// The reserved range of app user ids (inclusive).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct UidRange {
    /// First id.
    pub first: u32,
    /// Last id.
    pub last: u32,
}

impl UidRange {
    /// Whether `uid` lies in the range.
    pub const fn contains(self, uid: u32) -> bool {
        uid >= self.first && uid <= self.last
    }
}

/// Why the state could not be used.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StateError {
    /// Reading or writing failed.
    Io(std::io::ErrorKind),
    /// The state file is damaged (line number).
    Corrupt(usize),
    /// The state was made for another id range.
    Range,
    /// Every id of the range was given out.
    Exhausted,
    /// [`MAX_APPS`] apps are known.
    TooMany,
    /// A stored manifest does not match its digest.
    Digest,
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "launcher.state.io ({kind})"),
            Self::Corrupt(line) => write!(f, "launcher.state.corrupt (line {line})"),
            Self::Range => f.write_str("launcher.state.range"),
            Self::Exhausted => f.write_str("launcher.state.uids_exhausted"),
            Self::TooMany => f.write_str("launcher.state.too_many"),
            Self::Digest => f.write_str("launcher.state.digest"),
        }
    }
}

impl std::error::Error for StateError {}

impl From<std::io::Error> for StateError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

/// What the launcher keeps about one app.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Entry {
    /// Its user (and group) id.
    pub uid: u32,
    /// Its profile.
    pub profile: ProfileName,
    /// Whether it is installed now.
    pub installed: bool,
    /// Version of the installed manifest.
    pub version: String,
    /// SHA-256 of the stored manifest (hex).
    pub digest: String,
    /// What is granted.
    pub granted: PermissionSet,
    /// SHA-256 of the key that signed it (64 hex digits; 16 in entries of
    /// format 2): the app's identity together with its id – kept after
    /// uninstalling, so the id, user id and kept data only ever go back to
    /// the same signer.
    pub signer: String,
}

/// The ids and apps.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct State {
    range: UidRange,
    next: u32,
    apps: BTreeMap<AppId, Entry>,
}

/// SHA-256 of `bytes` as lower-case hex.
pub fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn version_ok(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 32
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
}

fn parse_entry(words: &[&str]) -> Option<(AppId, Entry)> {
    let [id, uid, profile, installed, version, digest, granted, signer] = words else {
        return None;
    };
    let signer_ok = matches!(signer.len(), 16 | 64)
        && signer
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    let granted = if *granted == "-" { "" } else { granted };
    let hex = digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase());
    let entry = Entry {
        uid: uid.parse().ok()?,
        profile: ProfileName::from_word(profile)?,
        installed: match *installed {
            "1" => true,
            "0" => false,
            _ => return None,
        },
        version: version_ok(version).then(|| (*version).to_owned())?,
        digest: hex.then(|| (*digest).to_owned())?,
        granted: PermissionSet::parse_names(granted)?,
        signer: signer_ok.then(|| (*signer).to_owned())?,
    };
    Some((AppId::new(id).ok()?, entry))
}

impl State {
    /// An empty state for `range`.
    pub fn new(range: UidRange) -> Self {
        Self {
            range,
            next: range.first,
            apps: BTreeMap::new(),
        }
    }

    /// Reads the text of a state file made for `range`.
    pub fn parse(text: &str, range: UidRange) -> Result<Self, StateError> {
        let mut lines = text.lines().enumerate();
        let mut header = |want: &str| -> Result<Vec<u32>, StateError> {
            let (n, line) = lines.next().ok_or(StateError::Corrupt(0))?;
            let rest = line.strip_prefix(want).ok_or(StateError::Corrupt(n + 1))?;
            rest.split_whitespace()
                .map(|w| w.parse().map_err(|_| StateError::Corrupt(n + 1)))
                .collect()
        };
        // format 2 is read as well (short signer ids); it is written as 3
        let first = text.lines().next().unwrap_or_default();
        let magic = if first == MAGIC_2 { MAGIC_2 } else { MAGIC };
        if !header(magic)?.is_empty() {
            return Err(StateError::Corrupt(1));
        }
        if header("range ")? != [range.first, range.last] {
            return Err(StateError::Range);
        }
        let next = match header("next ")?[..] {
            [next] if next >= range.first && next <= range.last.saturating_add(1) => next,
            _ => return Err(StateError::Corrupt(3)),
        };
        let mut state = Self {
            range,
            next,
            apps: BTreeMap::new(),
        };
        for (n, line) in lines {
            let words: Vec<&str> = line.split(' ').collect();
            let (id, entry) = match words.split_first() {
                Some((&"app", rest)) => parse_entry(rest).ok_or(StateError::Corrupt(n + 1))?,
                _ => return Err(StateError::Corrupt(n + 1)),
            };
            let taken = state.apps.values().any(|e| e.uid == entry.uid);
            if !range.contains(entry.uid)
                || entry.uid >= next
                || taken
                || state.apps.len() >= MAX_APPS
            {
                return Err(StateError::Corrupt(n + 1));
            }
            if state.apps.insert(id, entry).is_some() {
                return Err(StateError::Corrupt(n + 1));
            }
        }
        Ok(state)
    }

    /// The text of the state file.
    pub fn render(&self) -> String {
        let mut text = format!(
            "{MAGIC}\nrange {} {}\nnext {}\n",
            self.range.first, self.range.last, self.next
        );
        for (id, e) in &self.apps {
            let granted = if e.granted.is_empty() {
                "-".to_owned()
            } else {
                e.granted.names()
            };
            text.push_str(&format!(
                "app {id} {} {} {} {} {} {granted} {}\n",
                e.uid,
                e.profile.word(),
                u8::from(e.installed),
                e.version,
                e.digest,
                e.signer
            ));
        }
        text
    }

    /// The user id of `id`: its own if it ever had one, else the next free
    /// one of the range.
    pub fn uid_for(&mut self, id: &AppId) -> Result<u32, StateError> {
        if let Some(entry) = self.apps.get(id) {
            return Ok(entry.uid);
        }
        if self.apps.len() >= MAX_APPS {
            return Err(StateError::TooMany);
        }
        if !self.range.contains(self.next) {
            return Err(StateError::Exhausted);
        }
        let uid = self.next;
        self.next += 1;
        Ok(uid)
    }

    /// Records `entry` for `id`.
    pub fn put(&mut self, id: AppId, entry: Entry) {
        self.apps.insert(id, entry);
    }

    /// The entry of `id`.
    pub fn get(&self, id: &AppId) -> Option<&Entry> {
        self.apps.get(id)
    }

    /// The entry of `id`, to change.
    pub fn get_mut(&mut self, id: &AppId) -> Option<&mut Entry> {
        self.apps.get_mut(id)
    }

    /// Every app the launcher knows (installed or not), by id.
    pub fn apps(&self) -> impl Iterator<Item = (&AppId, &Entry)> {
        self.apps.iter()
    }

    /// The id range.
    pub fn range(&self) -> UidRange {
        self.range
    }
}

/// The state on disk.
#[derive(Debug)]
pub struct Store {
    dir: PathBuf,
    /// What is stored.
    pub state: State,
}

impl Store {
    /// Opens the store in `dir` (created `0700` if missing).
    pub fn open(dir: &Path, range: UidRange) -> Result<Self, StateError> {
        crate::datadir::ensure_private_dir(dir)
            .map_err(|_| StateError::Io(std::io::ErrorKind::PermissionDenied))?;
        crate::datadir::ensure_private_dir(&dir.join(MANIFESTS))
            .map_err(|_| StateError::Io(std::io::ErrorKind::PermissionDenied))?;
        let file = dir.join(STATE_FILE);
        let state = match atomic::read(&file, MAX_STATE_BYTES) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| StateError::Corrupt(0))?;
                State::parse(text, range)?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => State::new(range),
            Err(error) => return Err(error.into()),
        };
        Ok(Self {
            dir: dir.to_owned(),
            state,
        })
    }

    /// Writes the state.
    pub fn save(&self) -> Result<(), StateError> {
        atomic::write(
            &self.dir.join(STATE_FILE),
            self.state.render().as_bytes(),
            0o600,
        )
        .map_err(Into::into)
    }

    fn manifest_path(&self, id: &AppId) -> PathBuf {
        self.dir.join(MANIFESTS).join(format!("{id}.toml"))
    }

    /// Stores the verified manifest of `id`; returns its digest.
    pub fn save_manifest(&self, id: &AppId, bytes: &[u8]) -> Result<String, StateError> {
        atomic::write(&self.manifest_path(id), bytes, 0o600)?;
        Ok(digest(bytes))
    }

    /// The stored manifest of `id`, checked against the recorded digest.
    pub fn load_manifest(&self, id: &AppId) -> Result<Vec<u8>, StateError> {
        let entry = self.state.get(id).ok_or(StateError::Digest)?;
        let bytes = atomic::read(&self.manifest_path(id), crate::manifest::MAX_BYTES as u64)?;
        if digest(&bytes) != entry.digest {
            return Err(StateError::Digest);
        }
        Ok(bytes)
    }

    /// Removes the stored manifest of `id`.
    pub fn remove_manifest(&self, id: &AppId) -> Result<(), StateError> {
        match std::fs::remove_file(self.manifest_path(id)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
