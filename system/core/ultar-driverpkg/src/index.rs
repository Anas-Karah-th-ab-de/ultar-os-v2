//! The depot index (`index.toml`): every package of a depot with its manifest and hash,
//! signed like a package, plus a lookup structure for fast matching.

use crate::error::Coded;
use crate::hardware::HardwareId;
use crate::keys::{sha256_hex, PrivateKey, TrustedKeys};
use crate::manifest::{Manifest, ManifestError, RawManifest};
use crate::matching::Specificity;
use crate::package::{self, Limits, PackageError, EXTENSION};
use crate::pattern::{ClassPrefix, IdPattern};
use crate::sign::{index_message, SignatureBlob, SignatureError};
use crate::version::Version;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use std::fmt;

/// File name of the index inside a depot directory.
pub const INDEX_NAME: &str = "index.toml";
/// File name of the detached index signature.
pub const INDEX_SIGNATURE_NAME: &str = "index.toml.sig";
/// Index format version written by this crate.
pub const INDEX_FORMAT: u32 = 1;

/// Why an index was rejected or could not be built.
#[derive(Debug, Clone, PartialEq)]
pub enum IndexError {
    /// A file-system operation failed.
    Io {
        /// The file or directory involved.
        path: String,
        /// The kind of failure.
        kind: std::io::ErrorKind,
    },
    /// The index is not valid UTF-8.
    NotUtf8,
    /// The index TOML is malformed.
    Syntax(Box<toml::de::Error>),
    /// The index could not be serialized.
    Encode,
    /// The index was written in a format this version does not know.
    UnsupportedFormat(u32),
    /// An entry's file name is not a plain `*.utr` name.
    BadFileName(String),
    /// An entry's hash is not 64 hexadecimal digits.
    BadHash(String),
    /// An entry's manifest is invalid.
    BadEntry {
        /// Package file of the entry.
        file: String,
        /// What is wrong with its manifest.
        problem: ManifestError,
    },
    /// Two entries name the same file.
    DuplicateFile(String),
    /// Two entries carry the same driver id and version.
    DuplicateDriver {
        /// Driver identifier.
        id: String,
        /// Driver version.
        version: String,
    },
    /// A package found while scanning a depot directory was rejected.
    Package {
        /// The package file.
        file: String,
        /// Why it was rejected.
        problem: PackageError,
    },
    /// The index has no signature file.
    Unsigned,
    /// The index signature was rejected.
    Signature(SignatureError),
    /// A package file does not have the size or hash recorded in the index.
    FileMismatch(String),
}

impl Coded for IndexError {
    fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "driverpkg.index.io",
            Self::NotUtf8 => "driverpkg.index.not-utf8",
            Self::Syntax(_) => "driverpkg.index.syntax",
            Self::Encode => "driverpkg.index.encode",
            Self::UnsupportedFormat(_) => "driverpkg.index.unsupported-format",
            Self::BadFileName(_) => "driverpkg.index.bad-file-name",
            Self::BadHash(_) => "driverpkg.index.bad-hash",
            Self::BadEntry { .. } => "driverpkg.index.bad-entry",
            Self::DuplicateFile(_) => "driverpkg.index.duplicate-file",
            Self::DuplicateDriver { .. } => "driverpkg.index.duplicate-driver",
            Self::Package { .. } => "driverpkg.index.package",
            Self::Unsigned => "driverpkg.index.unsigned",
            Self::Signature(e) => e.code(),
            Self::FileMismatch(_) => "driverpkg.index.file-mismatch",
        }
    }
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::Signature(e) => e.fmt(f),
            Self::Io { path, kind } => write!(f, "{code}: {path}: {kind:?}"),
            Self::Syntax(e) => write!(f, "{code}: {}", e.message()),
            Self::UnsupportedFormat(n) => write!(f, "{code}: {n}"),
            Self::BadEntry { file, problem } => write!(f, "{code}: {file}: {problem}"),
            Self::DuplicateDriver { id, version } => write!(f, "{code}: {id} {version}"),
            Self::Package { file, problem } => write!(f, "{code}: {file}: {problem}"),
            Self::BadFileName(s)
            | Self::BadHash(s)
            | Self::DuplicateFile(s)
            | Self::FileMismatch(s) => {
                write!(f, "{code}: {s}")
            }
            Self::NotUtf8 | Self::Encode | Self::Unsigned => f.write_str(code),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<SignatureError> for IndexError {
    fn from(e: SignatureError) -> Self {
        Self::Signature(e)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawIndex {
    format: u32,
    #[serde(default)]
    driver: Vec<RawEntry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    file: String,
    sha256: String,
    size: u64,
    key_id: String,
    manifest: RawManifest,
}

/// One package of the depot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    /// Package file name inside the depot directory.
    pub file: String,
    /// SHA-256 of the whole package file.
    pub sha256: String,
    /// Size of the package file in bytes.
    pub size: u64,
    /// Identifier of the key that signed the package.
    pub key_id: String,
    /// The package's manifest.
    pub manifest: Manifest,
}

fn check_file_name(file: &str) -> Result<(), IndexError> {
    let plain = !file.contains(['/', '\\'])
        && !file.starts_with('.')
        && !file.chars().any(char::is_control);
    let stem = file
        .strip_suffix(EXTENSION)
        .and_then(|s| s.strip_suffix('.'));
    if plain && stem.is_some_and(|s| !s.is_empty()) {
        Ok(())
    } else {
        Err(IndexError::BadFileName(file.to_string()))
    }
}

impl IndexEntry {
    /// Verifies a package against the trusted keys and describes it for the index.
    pub fn from_package(
        file: &str,
        bytes: &[u8],
        trusted: &TrustedKeys,
        limits: &Limits,
    ) -> Result<Self, IndexError> {
        check_file_name(file)?;
        let verified =
            package::verify(bytes, trusted, limits).map_err(|problem| IndexError::Package {
                file: file.to_string(),
                problem,
            })?;
        Ok(Self {
            file: file.to_string(),
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
            key_id: verified.key_id,
            manifest: verified.manifest,
        })
    }

    /// Checks that package bytes are exactly the file recorded in the (signed) index.
    pub fn check_file(&self, bytes: &[u8]) -> Result<(), IndexError> {
        if bytes.len() as u64 == self.size && sha256_hex(bytes) == self.sha256 {
            Ok(())
        } else {
            Err(IndexError::FileMismatch(self.file.clone()))
        }
    }

    fn from_raw(raw: &RawEntry) -> Result<Self, IndexError> {
        check_file_name(&raw.file)?;
        if raw.sha256.len() != 64 || !raw.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(IndexError::BadHash(raw.file.clone()));
        }
        let manifest =
            Manifest::from_raw(&raw.manifest).map_err(|problem| IndexError::BadEntry {
                file: raw.file.clone(),
                problem,
            })?;
        Ok(Self {
            file: raw.file.clone(),
            sha256: raw.sha256.to_ascii_lowercase(),
            size: raw.size,
            key_id: raw.key_id.clone(),
            manifest,
        })
    }
}

type Slots = Vec<usize>;

fn push(out: &mut Vec<(usize, Specificity)>, slots: Option<&Slots>, specificity: Specificity) {
    out.extend(slots.into_iter().flatten().map(|slot| (*slot, specificity)));
}

/// Lookup tables of one bus (PCI or USB).
#[derive(Debug, Clone, Default)]
struct BusLookup {
    exact: HashMap<(u16, u16), Slots>,
    vendor: HashMap<u16, Slots>,
    class: HashMap<ClassPrefix, Slots>,
}

impl BusLookup {
    fn add(&mut self, slot: usize, ids: &[IdPattern], classes: &[ClassPrefix]) {
        for pattern in ids {
            match pattern.device {
                Some(device) => self
                    .exact
                    .entry((pattern.vendor, device))
                    .or_default()
                    .push(slot),
                None => self.vendor.entry(pattern.vendor).or_default().push(slot),
            }
        }
        for prefix in classes {
            self.class.entry(*prefix).or_default().push(slot);
        }
    }

    fn hits(&self, out: &mut Vec<(usize, Specificity)>, id: (u16, u16), class: Option<u32>) {
        push(out, self.exact.get(&id), Specificity::EXACT);
        push(out, self.vendor.get(&id.0), Specificity::WILDCARD);
        for digits in [6u8, 4, 2] {
            if let Some(prefix) = class.and_then(|c| ClassPrefix::of_class(c, digits)) {
                push(out, self.class.get(&prefix), Specificity::class(digits));
            }
        }
    }
}

/// Hash maps from hardware keys to entry positions.
#[derive(Debug, Clone, Default)]
struct Lookup {
    pci: BusLookup,
    usb: BusLookup,
    dt: HashMap<String, Slots>,
    acpi: HashMap<String, Slots>,
    platform: HashMap<String, Slots>,
}

impl Lookup {
    fn build(entries: &[IndexEntry]) -> Self {
        let mut lookup = Self::default();
        for (slot, entry) in entries.iter().enumerate() {
            let rules = &entry.manifest.rules;
            lookup.pci.add(slot, &rules.pci, &rules.pci_class);
            lookup.usb.add(slot, &rules.usb, &rules.usb_class);
            for name in &rules.dt {
                lookup.dt.entry(name.clone()).or_default().push(slot);
            }
            for name in &rules.acpi {
                lookup.acpi.entry(name.clone()).or_default().push(slot);
            }
            for name in &rules.platform {
                lookup.platform.entry(name.clone()).or_default().push(slot);
            }
        }
        lookup
    }
}

/// A validated depot index with its lookup structure.
#[derive(Debug, Clone)]
pub struct DriverIndex {
    entries: Vec<IndexEntry>,
    lookup: Lookup,
}

impl DriverIndex {
    /// Builds an index; entries are sorted by driver id, version and file name.
    pub fn new(mut entries: Vec<IndexEntry>) -> Result<Self, IndexError> {
        entries.sort_by(|a, b| {
            (&a.manifest.id, &a.manifest.version, &a.file).cmp(&(
                &b.manifest.id,
                &b.manifest.version,
                &b.file,
            ))
        });
        let mut files = BTreeSet::new();
        let mut drivers = BTreeSet::new();
        for entry in &entries {
            check_file_name(&entry.file)?;
            if !files.insert(entry.file.as_str()) {
                return Err(IndexError::DuplicateFile(entry.file.clone()));
            }
            if !drivers.insert((entry.manifest.id.as_str(), &entry.manifest.version)) {
                let (id, version) = (
                    entry.manifest.id.clone(),
                    entry.manifest.version.to_string(),
                );
                return Err(IndexError::DuplicateDriver { id, version });
            }
        }
        let lookup = Lookup::build(&entries);
        Ok(Self { entries, lookup })
    }

    /// All entries in index order.
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    /// The entry at a position returned by [`DriverIndex::hits`].
    pub fn get(&self, position: usize) -> Option<&IndexEntry> {
        self.entries.get(position)
    }

    /// Finds a driver by id and version.
    pub fn find(&self, id: &str, version: &Version) -> Option<&IndexEntry> {
        self.entries
            .iter()
            .find(|e| e.manifest.id == id && &e.manifest.version == version)
    }

    /// Number of packages.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` for an empty depot.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// A copy limited to the given package files (e.g. the drivers copied to the target disk).
    pub fn retain_files(&self, files: &[String]) -> Self {
        let entries: Vec<IndexEntry> = self
            .entries
            .iter()
            .filter(|e| files.contains(&e.file))
            .cloned()
            .collect();
        let lookup = Lookup::build(&entries);
        Self { entries, lookup }
    }

    /// Entry positions whose rules match the identifier, with the specificity of each hit.
    pub fn hits(&self, id: &HardwareId) -> Vec<(usize, Specificity)> {
        let lookup = &self.lookup;
        let mut out = Vec::new();
        match id {
            HardwareId::Pci {
                vendor,
                device,
                class,
                ..
            } => {
                lookup.pci.hits(&mut out, (*vendor, *device), Some(*class));
            }
            HardwareId::Usb {
                vendor,
                product,
                class,
            } => {
                lookup.usb.hits(
                    &mut out,
                    (*vendor, *product),
                    (*class != 0).then_some(*class),
                );
            }
            HardwareId::Dt(name) => push(&mut out, lookup.dt.get(name), Specificity::EXACT),
            HardwareId::Acpi(name) => push(
                &mut out,
                lookup.acpi.get(&name.to_ascii_uppercase()),
                Specificity::EXACT,
            ),
            HardwareId::Platform(name) => {
                push(&mut out, lookup.platform.get(name), Specificity::EXACT)
            }
        }
        out
    }

    /// Serializes the index to TOML (deterministic for a given set of entries).
    pub fn to_toml(&self) -> Result<String, IndexError> {
        let raw = RawIndex {
            format: INDEX_FORMAT,
            driver: self
                .entries
                .iter()
                .map(|e| RawEntry {
                    file: e.file.clone(),
                    sha256: e.sha256.clone(),
                    size: e.size,
                    key_id: e.key_id.clone(),
                    manifest: e.manifest.to_raw(),
                })
                .collect(),
        };
        toml::to_string(&raw).map_err(|_| IndexError::Encode)
    }

    /// Parses index text **without** checking a signature (use [`DriverIndex::parse_verified`] on devices).
    pub fn parse(text: &str) -> Result<Self, IndexError> {
        let raw: RawIndex = toml::from_str(text).map_err(|e| IndexError::Syntax(Box::new(e)))?;
        if raw.format != INDEX_FORMAT {
            return Err(IndexError::UnsupportedFormat(raw.format));
        }
        Self::new(
            raw.driver
                .iter()
                .map(IndexEntry::from_raw)
                .collect::<Result<_, _>>()?,
        )
    }

    /// Signs index text and returns the content of `index.toml.sig`.
    pub fn sign(index_text: &str, key: &PrivateKey) -> String {
        SignatureBlob::create(&index_message(index_text.as_bytes()), key).encode()
    }

    /// Verifies the detached signature against the trusted keys, then parses the index.
    pub fn parse_verified(
        index: &[u8],
        signature: Option<&[u8]>,
        trusted: &TrustedKeys,
    ) -> Result<Self, IndexError> {
        let blob = SignatureBlob::parse(signature.ok_or(IndexError::Unsigned)?)?;
        blob.check(&index_message(index), trusted)?;
        Self::parse(std::str::from_utf8(index).map_err(|_| IndexError::NotUtf8)?)
    }
}
