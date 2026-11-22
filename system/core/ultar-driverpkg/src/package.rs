//! The `.utr` container: packing, reading and signature verification.
//!
//! Layout (outer tar, uncompressed):
//! ```text
//! ultar-driver.toml   manifest
//! payload.tar.gz      module / firmware / HAL configuration
//! sig.ed25519         "<key id>\n<base64 signature>"
//! ```

use crate::error::Coded;
use crate::keys::{sha256_hex, PrivateKey, TrustedKeys};
use crate::manifest::{Manifest, ManifestError};
use crate::payload::{list_payload, read_payload_file, unpack_payload, PayloadEntry};
use crate::sign::{package_message, SignatureBlob, SignatureError};
use std::fmt;
use std::io::Read;
use std::path::Path;

/// Name of the manifest inside a package.
pub const MANIFEST_NAME: &str = "ultar-driver.toml";
/// Name of the payload inside a package.
pub const PAYLOAD_NAME: &str = "payload.tar.gz";
/// Name of the signature inside a package.
pub const SIGNATURE_NAME: &str = "sig.ed25519";
/// File extension of driver packages.
pub const EXTENSION: &str = "utr";

/// Size limits applied while reading and unpacking a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limits {
    /// Largest accepted manifest in bytes.
    pub max_manifest: u64,
    /// Largest accepted compressed payload in bytes.
    pub max_payload: u64,
    /// Largest accepted signature blob in bytes.
    pub max_signature: u64,
    /// Largest accepted single unpacked file in bytes.
    pub max_unpacked_file: u64,
    /// Largest accepted sum of all unpacked files in bytes.
    pub max_unpacked_total: u64,
    /// Largest accepted number of payload entries.
    pub max_entries: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_manifest: 64 * 1024,
            max_payload: 256 * 1024 * 1024,
            max_signature: 1024,
            max_unpacked_file: 256 * 1024 * 1024,
            max_unpacked_total: 1024 * 1024 * 1024,
            max_entries: 4096,
        }
    }
}

/// The part of a package a size limit refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// The manifest.
    Manifest,
    /// The compressed payload.
    Payload,
    /// The signature blob.
    Signature,
    /// One unpacked payload file.
    UnpackedFile,
    /// All unpacked payload files together.
    UnpackedTotal,
}

/// Why a package was rejected or could not be built.
#[derive(Debug, Clone, PartialEq)]
pub enum PackageError {
    /// A file-system operation failed.
    Io {
        /// The file or directory involved (empty for in-memory work).
        path: String,
        /// The kind of failure.
        kind: std::io::ErrorKind,
    },
    /// The outer container is not a readable tar archive.
    Archive,
    /// The package has no manifest.
    MissingManifest,
    /// The package has no payload.
    MissingPayload,
    /// The package has no signature – unsigned drivers are never loaded.
    Unsigned,
    /// The container holds an entry that does not belong there.
    UnexpectedEntry(String),
    /// An entry appears twice.
    DuplicateEntry(String),
    /// A part exceeds its size limit.
    TooLarge {
        /// Which part is too large.
        part: Part,
        /// The limit in bytes.
        limit: u64,
    },
    /// The manifest is invalid.
    Manifest(ManifestError),
    /// The signature was rejected (foreign key, modified content, malformed).
    Signature(SignatureError),
    /// The payload is not a readable gzip-compressed tar archive.
    PayloadCorrupt,
    /// A payload entry has an absolute path.
    AbsolutePath(String),
    /// A payload entry tries to leave the target directory with `..`.
    PathTraversal(String),
    /// A payload entry has an otherwise invalid path.
    BadPath(String),
    /// A payload entry is a link, device node or other non-regular file.
    UnsafeEntryType(String),
    /// The payload has more entries than allowed.
    TooManyEntries {
        /// The limit.
        limit: u64,
    },
    /// The manifest names a file the payload does not contain.
    MissingContent(String),
}

impl Coded for PackageError {
    fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "driverpkg.package.io",
            Self::Archive => "driverpkg.package.archive",
            Self::MissingManifest => "driverpkg.package.missing-manifest",
            Self::MissingPayload => "driverpkg.package.missing-payload",
            Self::Unsigned => "driverpkg.package.unsigned",
            Self::UnexpectedEntry(_) => "driverpkg.package.unexpected-entry",
            Self::DuplicateEntry(_) => "driverpkg.package.duplicate-entry",
            Self::TooLarge { .. } => "driverpkg.package.too-large",
            Self::Manifest(e) => e.code(),
            Self::Signature(e) => e.code(),
            Self::PayloadCorrupt => "driverpkg.package.payload-corrupt",
            Self::AbsolutePath(_) => "driverpkg.package.absolute-path",
            Self::PathTraversal(_) => "driverpkg.package.path-traversal",
            Self::BadPath(_) => "driverpkg.package.bad-path",
            Self::UnsafeEntryType(_) => "driverpkg.package.unsafe-entry-type",
            Self::TooManyEntries { .. } => "driverpkg.package.too-many-entries",
            Self::MissingContent(_) => "driverpkg.package.missing-content",
        }
    }
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::Manifest(e) => e.fmt(f),
            Self::Signature(e) => e.fmt(f),
            Self::Io { path, kind } => write!(f, "{code}: {path}: {kind:?}"),
            Self::TooLarge { part, limit } => write!(f, "{code}: {part:?} > {limit}"),
            Self::TooManyEntries { limit } => write!(f, "{code}: > {limit}"),
            Self::UnexpectedEntry(s)
            | Self::DuplicateEntry(s)
            | Self::AbsolutePath(s)
            | Self::PathTraversal(s)
            | Self::BadPath(s)
            | Self::UnsafeEntryType(s)
            | Self::MissingContent(s) => write!(f, "{code}: {s}"),
            Self::Archive
            | Self::MissingManifest
            | Self::MissingPayload
            | Self::Unsigned
            | Self::PayloadCorrupt => f.write_str(code),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<ManifestError> for PackageError {
    fn from(e: ManifestError) -> Self {
        Self::Manifest(e)
    }
}

impl From<SignatureError> for PackageError {
    fn from(e: SignatureError) -> Self {
        Self::Signature(e)
    }
}

/// The three raw parts of a package, not yet trusted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawPackage {
    /// Manifest bytes.
    pub manifest: Vec<u8>,
    /// Compressed payload bytes.
    pub payload: Vec<u8>,
    /// Signature blob, `None` for an unsigned package.
    pub signature: Option<Vec<u8>>,
}

/// A package whose signature was verified against the trusted keys.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPackage {
    /// The validated manifest.
    pub manifest: Manifest,
    /// Identifier of the trusted key that signed the package.
    pub key_id: String,
    /// Validated payload entries.
    pub entries: Vec<PayloadEntry>,
    payload: Vec<u8>,
    limits: Limits,
}

/// Display information about a package; nothing here implies trust.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageInfo {
    /// The validated manifest.
    pub manifest: Manifest,
    /// Key identifier stated in the signature blob, `None` when unsigned or malformed.
    pub key_id: Option<String>,
    /// SHA-256 of the compressed payload.
    pub payload_sha256: String,
    /// Payload entries.
    pub entries: Vec<PayloadEntry>,
}

fn append(
    builder: &mut tar::Builder<Vec<u8>>,
    name: &str,
    data: &[u8],
) -> Result<(), PackageError> {
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_mtime(0);
    builder
        .append_data(&mut header, name, data)
        .map_err(|_| PackageError::Archive)
}

fn check_size(part: Part, len: usize, limit: u64) -> Result<(), PackageError> {
    if len as u64 > limit {
        Err(PackageError::TooLarge { part, limit })
    } else {
        Ok(())
    }
}

fn check_content(manifest: &Manifest, entries: &[PayloadEntry]) -> Result<(), PackageError> {
    for path in manifest.content.paths() {
        if !entries.iter().any(|e| !e.is_dir && e.path == path) {
            return Err(PackageError::MissingContent(path.to_string()));
        }
    }
    Ok(())
}

/// Assembles the container from its parts (used by [`pack`]; exposed for tests of rejection paths).
pub fn assemble(raw: &RawPackage) -> Result<Vec<u8>, PackageError> {
    let mut builder = tar::Builder::new(Vec::new());
    append(&mut builder, MANIFEST_NAME, &raw.manifest)?;
    append(&mut builder, PAYLOAD_NAME, &raw.payload)?;
    if let Some(signature) = &raw.signature {
        append(&mut builder, SIGNATURE_NAME, signature)?;
    }
    builder.into_inner().map_err(|_| PackageError::Archive)
}

/// Validates manifest and payload, signs them and returns the `.utr` bytes.
pub fn pack(
    manifest: &[u8],
    payload: &[u8],
    key: &PrivateKey,
    limits: &Limits,
) -> Result<Vec<u8>, PackageError> {
    check_size(Part::Manifest, manifest.len(), limits.max_manifest)?;
    check_size(Part::Payload, payload.len(), limits.max_payload)?;
    let parsed = Manifest::from_bytes(manifest)?;
    check_content(&parsed, &list_payload(payload, limits)?)?;
    let blob = SignatureBlob::create(&package_message(manifest, payload), key);
    assemble(&RawPackage {
        manifest: manifest.to_vec(),
        payload: payload.to_vec(),
        signature: Some(blob.encode().into_bytes()),
    })
}

fn read_part<R: Read>(entry: &mut R, part: Part, limit: u64) -> Result<Vec<u8>, PackageError> {
    let mut data = Vec::new();
    entry
        .take(limit.saturating_add(1))
        .read_to_end(&mut data)
        .map_err(|_| PackageError::Archive)?;
    check_size(part, data.len(), limit)?;
    Ok(data)
}

fn store(slot: &mut Option<Vec<u8>>, name: &str, data: Vec<u8>) -> Result<(), PackageError> {
    if slot.is_some() {
        return Err(PackageError::DuplicateEntry(name.to_string()));
    }
    *slot = Some(data);
    Ok(())
}

/// Splits a container into its raw parts, enforcing the size limits. Establishes no trust.
pub fn read_package(bytes: &[u8], limits: &Limits) -> Result<RawPackage, PackageError> {
    let mut archive = tar::Archive::new(bytes);
    let (mut manifest, mut payload, mut signature) = (None, None, None);
    for entry in archive.entries().map_err(|_| PackageError::Archive)? {
        let mut entry = entry.map_err(|_| PackageError::Archive)?;
        let name = String::from_utf8_lossy(&entry.path_bytes()).into_owned();
        match name.as_str() {
            MANIFEST_NAME => {
                let data = read_part(&mut entry, Part::Manifest, limits.max_manifest)?;
                store(&mut manifest, &name, data)?;
            }
            PAYLOAD_NAME => {
                let data = read_part(&mut entry, Part::Payload, limits.max_payload)?;
                store(&mut payload, &name, data)?;
            }
            SIGNATURE_NAME => {
                let data = read_part(&mut entry, Part::Signature, limits.max_signature)?;
                store(&mut signature, &name, data)?;
            }
            _ => return Err(PackageError::UnexpectedEntry(name)),
        }
    }
    Ok(RawPackage {
        manifest: manifest.ok_or(PackageError::MissingManifest)?,
        payload: payload.ok_or(PackageError::MissingPayload)?,
        signature,
    })
}

/// Verifies a package. Only a package signed by a trusted key over unmodified content is returned.
pub fn verify(
    bytes: &[u8],
    trusted: &TrustedKeys,
    limits: &Limits,
) -> Result<VerifiedPackage, PackageError> {
    let raw = read_package(bytes, limits)?;
    let blob = SignatureBlob::parse(&raw.signature.ok_or(PackageError::Unsigned)?)?;
    blob.check(&package_message(&raw.manifest, &raw.payload), trusted)?;
    let manifest = Manifest::from_bytes(&raw.manifest)?;
    let entries = list_payload(&raw.payload, limits)?;
    check_content(&manifest, &entries)?;
    Ok(VerifiedPackage {
        manifest,
        key_id: blob.key_id,
        entries,
        payload: raw.payload,
        limits: limits.clone(),
    })
}

/// Reads display information without checking the signature against any trust.
pub fn info(bytes: &[u8], limits: &Limits) -> Result<PackageInfo, PackageError> {
    let raw = read_package(bytes, limits)?;
    let key_id = raw
        .signature
        .as_deref()
        .and_then(|s| SignatureBlob::parse(s).ok())
        .map(|blob| blob.key_id);
    Ok(PackageInfo {
        manifest: Manifest::from_bytes(&raw.manifest)?,
        key_id,
        payload_sha256: sha256_hex(&raw.payload),
        entries: list_payload(&raw.payload, limits)?,
    })
}

impl VerifiedPackage {
    /// The verified compressed payload.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Reads one file of the verified payload into memory; `None` when it is not in the package.
    pub fn read_file(&self, path: &str) -> Result<Option<Vec<u8>>, PackageError> {
        read_payload_file(&self.payload, path, &self.limits)
    }

    /// Unpacks the verified payload below `dest` (paths are confined to `dest`).
    pub fn unpack(&self, dest: &Path) -> Result<Vec<PayloadEntry>, PackageError> {
        unpack_payload(&self.payload, dest, &self.limits)
    }
}
