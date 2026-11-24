//! The installation itself: inspect disks (read only), plan the A/B layout of
//! [`ultar_slots::install`], and – only with an [`EraseConfirmation`] – write it.
//!
//! Safety rules (DESKTOP-SPEC §6 no. 14, CLICKS `installer.cancel`: "nothing has been written
//! before step 15"):
//! - every function except [`write::execute`] opens devices read-only;
//! - [`write::execute`] needs an [`EraseConfirmation`], which only
//!   [`crate::wizard::Wizard::confirm_install`] creates – on the summary, with the erase box
//!   ticked, by the Install button – and which names the target it was given for;
//! - the target must not be the installation medium and must still have the size it had when
//!   the user confirmed.

pub mod answers;
pub mod device;
pub mod write;

pub use device::{BlockDevice, FileDevice};

use std::path::{Path, PathBuf};
use ultar_slots::gpt::Header;
use ultar_slots::install::{self, Source, Target, PARTS};

/// Most partitions listed for a disk.
pub const MAX_LISTED: usize = 128;

/// Why the engine refused or failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// Reading or writing a device failed (operation, OS error kind).
    Io(&'static str, std::io::ErrorKind),
    /// The medium has no valid A/B source (missing partition).
    NoSource(&'static str),
    /// The target disk is too small (needed, available bytes).
    TooSmall(u64, u64),
    /// Target and medium are the same device.
    TargetIsSource,
    /// The target changed since the confirmation (size or path).
    TargetChanged,
    /// Read-back after writing differed (partition).
    Verify(&'static str),
    /// The partition table could not be built.
    Gpt,
}

impl EngineError {
    /// i18n key.
    pub fn key(&self) -> &'static str {
        match self {
            EngineError::Io(..) => "installer.error.io",
            EngineError::NoSource(_) => "installer.error.no_source",
            EngineError::TooSmall(..) => "installer.error.too_small",
            EngineError::TargetIsSource => "installer.error.target_is_source",
            EngineError::TargetChanged => "installer.error.target_changed",
            EngineError::Verify(_) => "installer.error.verify",
            EngineError::Gpt => "installer.error.gpt",
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {self:?}", self.key())
    }
}

impl std::error::Error for EngineError {}

/// A disk the user may choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskInfo {
    /// Device path given by the launcher (block device, loop device or image file).
    pub path: PathBuf,
    /// Name to show (model).
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
    /// Names of the partitions on it now (what an erase loses).
    pub partitions: Vec<String>,
}

/// Reads the GPT of a device: header and entry array (primary, else backup).
pub fn read_gpt(dev: &mut dyn BlockDevice) -> Option<(Header, Vec<u8>)> {
    let sectors = dev.len() / 512;
    for lba in [1, sectors.saturating_sub(1)] {
        let mut block = [0u8; 512];
        if lba == 0 || dev.read_at(lba * 512, &mut block).is_err() {
            continue;
        }
        let Ok(h) = Header::parse(&block, lba) else {
            continue;
        };
        let mut entries = vec![0u8; h.entries_len()];
        if dev.read_at(h.entries_lba * 512, &mut entries).is_err() {
            continue;
        }
        if h.check_entries(&entries).is_ok() {
            return Some((h, entries));
        }
    }
    None
}

/// Names of the partitions in a GPT.
pub fn partition_names(h: &Header, entries: &[u8]) -> Vec<String> {
    (0..h.entry_count as usize)
        .filter_map(|i| h.entry(entries, i).ok().flatten())
        .take(MAX_LISTED)
        .map(|e| {
            String::from_utf16_lossy(&e.name)
                .trim_end_matches('\0')
                .to_owned()
        })
        .collect()
}

/// Looks at a disk without writing.
pub fn inspect(path: &Path, name: &str) -> Result<DiskInfo, EngineError> {
    let mut dev = FileDevice::open_read(path)?;
    let partitions = read_gpt(&mut dev)
        .map(|(h, e)| partition_names(&h, &e))
        .unwrap_or_default();
    Ok(DiskInfo {
        path: path.to_path_buf(),
        name: name.to_owned(),
        bytes: dev.len(),
        partitions,
    })
}

/// The installation medium's partitions.
pub fn read_source(dev: &mut dyn BlockDevice) -> Result<Source, EngineError> {
    let (h, entries) = read_gpt(dev).ok_or(EngineError::NoSource("GPT"))?;
    Source::find(|name| {
        h.find(&entries, name)
            .ok()
            .flatten()
            .map(|e| (e.first_lba, e.last_lba))
    })
    .map_err(|e| match e {
        install::SourceError::Missing(n) | install::SourceError::Inverted(n) => {
            EngineError::NoSource(n)
        }
    })
}

/// What will be written where.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Medium path.
    pub source: PathBuf,
    /// Target path.
    pub target: PathBuf,
    /// Target size when planned.
    pub target_bytes: u64,
    /// Layout.
    pub layout: [Target; PARTS],
    /// The user's choices, written for the first start.
    pub answers: answers::Answers,
}

impl Plan {
    /// Plans the installation of `source` onto `target` (both only read).
    pub fn new(
        source: &Path,
        target: &DiskInfo,
        answers: answers::Answers,
    ) -> Result<Plan, EngineError> {
        if same_device(source, &target.path) {
            return Err(EngineError::TargetIsSource);
        }
        let mut src = FileDevice::open_read(source)?;
        let layout = install::plan(&read_source(&mut src)?);
        let need = install::needed(&layout) * 512;
        if target.bytes < need {
            return Err(EngineError::TooSmall(need, target.bytes));
        }
        Ok(Plan {
            source: source.to_path_buf(),
            target: target.path.clone(),
            target_bytes: target.bytes,
            layout,
            answers,
        })
    }

    /// Bytes the installation copies.
    pub fn copy_bytes(&self) -> u64 {
        install::parts(&self.layout)
            .map(|t| match t.fill {
                install::Fill::Copy((f, l)) => (l - f + 1) * 512,
                _ => 0,
            })
            .sum()
    }
}

/// `true` when two paths name the same file or device.
pub fn same_device(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(x), Ok(y)) => {
            (x.dev() == y.dev() && x.ino() == y.ino()) || (x.rdev() != 0 && x.rdev() == y.rdev())
        }
        _ => a == b,
    }
}

/// Proof that the user confirmed erasing exactly this target on the summary. Only
/// [`crate::wizard::Wizard::confirm_install`] creates one.
#[derive(Debug)]
pub struct EraseConfirmation {
    pub(crate) target: PathBuf,
    pub(crate) bytes: u64,
}

impl EraseConfirmation {
    /// The target the confirmation was given for.
    pub fn target(&self) -> &Path {
        &self.target
    }
}
