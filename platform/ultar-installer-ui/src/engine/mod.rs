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
    /// Installing beside another system needs a GPT partition table on the disk.
    NoTable,
    /// No free region is large enough (needed bytes, bytes of the largest free region).
    NoFreeSpace(u64, u64),
    /// The manual layout is not a valid A/B layout (catalog key of the reason).
    Manual(&'static str),
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
            EngineError::NoTable => "installer.error.no_table",
            EngineError::NoFreeSpace(..) => "installer.error.no_free_space",
            EngineError::Manual(key) => key,
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
    /// Its GPT partitions (`None`: no valid GPT – only "whole disk" is possible).
    pub table: Option<Vec<Existing>>,
}

/// A partition the target disk holds now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Existing {
    /// Index in the partition table.
    pub index: usize,
    /// GPT name.
    pub name: String,
    /// First sector.
    pub first: u64,
    /// Last sector (inclusive).
    pub last: u64,
    /// The raw 128-byte entry (kept bit for bit when the partition stays).
    pub entry: [u8; install::ENTRY_SIZE],
}

impl Existing {
    /// Size in bytes.
    pub fn bytes(&self) -> u64 {
        (self.last - self.first + 1) * 512
    }
}

impl DiskInfo {
    /// Sectors of the disk.
    pub fn sectors(&self) -> u64 {
        self.bytes / 512
    }

    /// Free regions (first sector, sectors) between the partitions in `used`, inside the
    /// usable sectors of a GPT disk, MiB aligned, largest first.
    pub fn free_regions(&self, used: &[(u64, u64)]) -> Vec<(u64, u64)> {
        let n = self.sectors();
        if n < 2 * install::MIB + 68 {
            return Vec::new();
        }
        let mut taken: Vec<(u64, u64)> = used.to_vec();
        taken.sort_unstable();
        let (mut pos, end) = (install::MIB, (n - 34) / install::MIB * install::MIB);
        let mut out = Vec::new();
        for (first, last) in taken.into_iter().chain([(end, end)]) {
            let stop = (first / install::MIB * install::MIB).min(end);
            if stop > pos {
                out.push((pos, stop - pos));
            }
            pos = pos.max((last + 1).div_ceil(install::MIB) * install::MIB);
        }
        out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        out
    }

    /// The ranges of all partitions it holds now.
    pub fn used(&self) -> Vec<(u64, u64)> {
        self.table
            .iter()
            .flatten()
            .map(|e| (e.first, e.last))
            .collect()
    }
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

/// The partitions of a GPT with their raw entries.
pub fn existing(h: &Header, entries: &[u8]) -> Vec<Existing> {
    let size = h.entry_size as usize;
    (0..(h.entry_count as usize).min(install::ENTRY_COUNT))
        .filter_map(|i| {
            let e = h.entry(entries, i).ok().flatten()?;
            let raw = entries.get(i * size..i * size + install::ENTRY_SIZE)?;
            let mut entry = [0u8; install::ENTRY_SIZE];
            entry.copy_from_slice(raw);
            Some(Existing {
                index: i,
                name: String::from_utf16_lossy(&e.name)
                    .trim_end_matches('\0')
                    .to_owned(),
                first: e.first_lba,
                last: e.last_lba,
                entry,
            })
        })
        .take(MAX_LISTED)
        .collect()
}

/// Looks at a disk without writing.
pub fn inspect(path: &Path, name: &str) -> Result<DiskInfo, EngineError> {
    let mut dev = FileDevice::open_read(path)?;
    let table = read_gpt(&mut dev).map(|(h, e)| existing(&h, &e));
    let partitions = table.iter().flatten().map(|e| e.name.clone()).collect();
    Ok(DiskInfo {
        path: path.to_path_buf(),
        name: name.to_owned(),
        bytes: dev.len(),
        partitions,
        table,
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
    /// `None`: the whole disk is erased. Otherwise the partitions that stay, bit for bit
    /// (installing beside another system, manual partitioning).
    pub keep: Option<Vec<Existing>>,
    /// Partitions of the disk that the installation removes (manual partitioning).
    pub removed: Vec<Existing>,
    /// Passphrase of the data encryption (LUKS2, see [`Plan::encrypted`]); `None`: plain data.
    pub encrypt: Option<crate::secret::Secret>,
    /// Cost of the LUKS2 key derivation (tests use [`ultar_luks::Params::TEST`]).
    pub luks: ultar_luks::Params,
}

/// Sectors the LUKS2 metadata takes in front of the encrypted data.
pub const LUKS_RESERVE: u64 = ultar_luks::DATA_OFFSET / 512;

impl Plan {
    /// The data partition encrypted with `pass` (LUKS2): it gets room for the metadata in
    /// front of the copied data – on a whole disk it grows, beside another system or in a
    /// manual layout it must already have the room ([`EngineError::TooSmall`] otherwise).
    pub fn encrypted(mut self, pass: crate::secret::Secret) -> Result<Plan, EngineError> {
        if self.encrypt.is_none() {
            let i = self
                .layout
                .iter()
                .position(|t| t.name == ultar_slots::layout::DATA_PART)
                .ok_or(EngineError::Gpt)?;
            let install::Fill::Copy((first, last)) = self.layout[i].fill else {
                return Err(EngineError::Gpt);
            };
            let need = install::align(last - first + 1) + LUKS_RESERVE;
            if self.keep.is_none() {
                self.layout[i].sectors = self.layout[i].sectors.max(need);
                let total = install::needed(&self.layout) * 512;
                if self.target_bytes < total {
                    return Err(EngineError::TooSmall(total, self.target_bytes));
                }
            } else if self.layout[i].sectors < need {
                return Err(EngineError::TooSmall(
                    need * 512,
                    self.layout[i].sectors * 512,
                ));
            }
        }
        self.encrypt = Some(pass);
        Ok(self)
    }

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
            keep: None,
            removed: Vec::new(),
            encrypt: None,
            luks: ultar_luks::Params::INSTALL,
        })
    }

    /// Plans the installation into the largest free region of `target` (nothing that exists
    /// is changed or moved): `share` percent of that region, at least what the layout needs.
    pub fn beside(
        source: &Path,
        target: &DiskInfo,
        share: u8,
        answers: answers::Answers,
    ) -> Result<Plan, EngineError> {
        let src = Plan::medium(source, target)?;
        let Some(table) = &target.table else {
            return Err(EngineError::NoTable);
        };
        let need: u64 = install::minimum(&src).iter().sum::<u64>() + install::MIB;
        let free = target.free_regions(&target.used());
        let (first, sectors) = free.first().copied().unwrap_or((0, 0));
        let layout = install::plan_beside(&src, first, sectors, share)
            .ok_or(EngineError::NoFreeSpace(need * 512, sectors * 512))?;
        Ok(Plan {
            source: source.to_path_buf(),
            target: target.path.clone(),
            target_bytes: target.bytes,
            layout,
            answers,
            keep: Some(table.clone()),
            removed: Vec::new(),
            encrypt: None,
            luks: ultar_luks::Params::INSTALL,
        })
    }

    /// Plans the installation into the regions of a manual layout.
    pub fn manual(
        source: &Path,
        target: &DiskInfo,
        parts: &[ManualPart],
        answers: answers::Answers,
    ) -> Result<Plan, EngineError> {
        let src = Plan::medium(source, target)?;
        let table = target.table.clone().unwrap_or_default();
        let regions = manual_regions(parts)?;
        let layout = install::plan_in(&src, &regions).map_err(|e| match e {
            install::RegionError::TooSmall(..) => EngineError::Manual("installer.manual.too_small"),
            install::RegionError::Overlap(..) => EngineError::Manual("installer.manual.overlap"),
            install::RegionError::Unaligned(_) => EngineError::Manual("installer.manual.unaligned"),
        })?;
        let n = target.sectors();
        if install::parts(&layout).any(|t| t.first < 34 || t.first + t.sectors > n - 34) {
            return Err(EngineError::Manual("installer.manual.outside"));
        }
        let kept: Vec<Existing> = table
            .iter()
            .filter(|e| {
                parts
                    .iter()
                    .any(|p| p.existing == Some(e.index) && p.role == PartRole::Keep)
            })
            .cloned()
            .collect();
        let removed: Vec<Existing> = table
            .iter()
            .filter(|e| !kept.iter().any(|k| k.index == e.index))
            .cloned()
            .collect();
        for t in install::parts(&layout) {
            let (a, b) = (t.first, t.first + t.sectors - 1);
            if kept.iter().any(|k| a <= k.last && k.first <= b) {
                return Err(EngineError::Manual("installer.manual.overlap"));
            }
        }
        Ok(Plan {
            source: source.to_path_buf(),
            target: target.path.clone(),
            target_bytes: target.bytes,
            layout,
            answers,
            keep: Some(kept),
            removed,
            encrypt: None,
            luks: ultar_luks::Params::INSTALL,
        })
    }

    fn medium(source: &Path, target: &DiskInfo) -> Result<Source, EngineError> {
        if same_device(source, &target.path) {
            return Err(EngineError::TargetIsSource);
        }
        let mut dev = FileDevice::open_read(source)?;
        read_source(&mut dev)
    }

    /// What each role needs at least, in sectors (ESP, slot A, slot B, data).
    pub fn minimum(source: &Path) -> Result<[u64; 4], EngineError> {
        let mut dev = FileDevice::open_read(source)?;
        Ok(install::minimum(&read_source(&mut dev)?))
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

/// What a partition of a manual layout is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PartRole {
    /// An existing partition that stays as it is.
    #[default]
    Keep,
    /// Removed (an existing partition) – its space becomes free.
    Delete,
    /// The EFI system partition of the installation.
    Esp,
    /// Slot A (boot control, kernel A, system A).
    SlotA,
    /// Slot B (kernel B, system B).
    SlotB,
    /// The data partition.
    Data,
}

impl PartRole {
    /// Every role in the order of the role select.
    pub const ALL: [PartRole; 6] = [
        PartRole::Keep,
        PartRole::Delete,
        PartRole::Esp,
        PartRole::SlotA,
        PartRole::SlotB,
        PartRole::Data,
    ];

    /// Catalog key.
    pub fn key(self) -> &'static str {
        match self {
            PartRole::Keep => "installer.manual.role.keep",
            PartRole::Delete => "installer.manual.role.delete",
            PartRole::Esp => "installer.manual.role.esp",
            PartRole::SlotA => "installer.manual.role.slot_a",
            PartRole::SlotB => "installer.manual.role.slot_b",
            PartRole::Data => "installer.manual.role.data",
        }
    }
}

/// A partition of a manual layout: an existing one (with its table index) or a new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManualPart {
    /// Index in the disk's table, `None` for a new partition.
    pub existing: Option<usize>,
    /// First sector.
    pub first: u64,
    /// Length in sectors.
    pub sectors: u64,
    /// What it is for.
    pub role: PartRole,
}

/// The regions of the four roles; each role exactly once.
pub fn manual_regions(parts: &[ManualPart]) -> Result<install::Regions, EngineError> {
    for role in [
        PartRole::Esp,
        PartRole::SlotA,
        PartRole::SlotB,
        PartRole::Data,
    ] {
        if parts.iter().filter(|p| p.role == role).count() > 1 {
            return Err(EngineError::Manual("installer.manual.twice"));
        }
    }
    let one = |role: PartRole, missing: &'static str| -> Result<(u64, u64), EngineError> {
        let mut it = parts.iter().filter(|p| p.role == role);
        let first = it.next().ok_or(EngineError::Manual(missing))?;
        if it.next().is_some() {
            return Err(EngineError::Manual("installer.manual.twice"));
        }
        Ok((first.first, first.sectors))
    };
    Ok(install::Regions {
        esp: one(PartRole::Esp, "installer.manual.need_esp")?,
        slot_a: one(PartRole::SlotA, "installer.manual.need_slot_a")?,
        slot_b: one(PartRole::SlotB, "installer.manual.need_slot_b")?,
        data: one(PartRole::Data, "installer.manual.need_data")?,
    })
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
