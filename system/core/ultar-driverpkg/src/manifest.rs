//! The driver manifest `ultar-driver.toml`: typed model and strict validation.

use crate::class::{DriverClass, Origin};
use crate::error::Coded;
use crate::pattern::{check_name, ClassPrefix, IdPattern, PatternError};
use crate::payload::normalize_path;
use crate::version::{Version, VersionError};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Lowest and highest accepted `rank`.
pub const RANK_RANGE: std::ops::RangeInclusive<i32> = -1000..=1000;

/// Why a manifest was rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum ManifestError {
    /// The manifest is not valid UTF-8.
    NotUtf8,
    /// The TOML is malformed, a key is missing or an unknown key is present.
    Syntax(Box<toml::de::Error>),
    /// `id` is not 1–64 characters of `[a-z0-9_-]` starting with a letter or digit.
    BadId(String),
    /// `version` is not a semantic version.
    BadVersion(VersionError),
    /// `class` is not one of the known classes.
    UnknownClass(String),
    /// `origin` is not `hand`, `workshop` or `vendor`.
    UnknownOrigin(String),
    /// `kernel_abi` is empty.
    NoKernelAbi,
    /// A `kernel_abi` entry has invalid characters or a misplaced `*`.
    BadKernelAbi(String),
    /// `rank` is outside [`RANK_RANGE`].
    RankOutOfRange(i32),
    /// `name_key` / `description_key` is not an i18n key (lower-case, dotted, no spaces).
    BadMessageKey(String),
    /// `[match]` contains no rule that can select hardware (`dmi` alone only refines).
    NoMatchRule,
    /// A pattern in the named `[match]` field is invalid.
    BadPattern {
        /// Name of the manifest field.
        field: &'static str,
        /// What is wrong with the pattern.
        problem: PatternError,
    },
    /// The same pattern appears twice in the named `[match]` field.
    DuplicatePattern {
        /// Name of the manifest field.
        field: &'static str,
        /// The repeated pattern.
        value: String,
    },
    /// `[content]` names neither a module nor firmware nor a HAL configuration.
    NoContent,
    /// A `[content]` path is absolute, leaves the payload or is otherwise invalid.
    BadContentPath(String),
}

impl Coded for ManifestError {
    fn code(&self) -> &'static str {
        match self {
            Self::NotUtf8 => "driverpkg.manifest.not-utf8",
            Self::Syntax(_) => "driverpkg.manifest.syntax",
            Self::BadId(_) => "driverpkg.manifest.bad-id",
            Self::BadVersion(_) => "driverpkg.manifest.bad-version",
            Self::UnknownClass(_) => "driverpkg.manifest.unknown-class",
            Self::UnknownOrigin(_) => "driverpkg.manifest.unknown-origin",
            Self::NoKernelAbi => "driverpkg.manifest.no-kernel-abi",
            Self::BadKernelAbi(_) => "driverpkg.manifest.bad-kernel-abi",
            Self::RankOutOfRange(_) => "driverpkg.manifest.rank-out-of-range",
            Self::BadMessageKey(_) => "driverpkg.manifest.bad-message-key",
            Self::NoMatchRule => "driverpkg.manifest.no-match-rule",
            Self::BadPattern { .. } => "driverpkg.manifest.bad-pattern",
            Self::DuplicatePattern { .. } => "driverpkg.manifest.duplicate-pattern",
            Self::NoContent => "driverpkg.manifest.no-content",
            Self::BadContentPath(_) => "driverpkg.manifest.bad-content-path",
        }
    }
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let code = self.code();
        match self {
            Self::NotUtf8 | Self::NoKernelAbi | Self::NoMatchRule | Self::NoContent => {
                f.write_str(code)
            }
            Self::Syntax(e) => write!(f, "{code}: {}", e.message()),
            Self::BadVersion(e) => write!(f, "{code}: {e}"),
            Self::RankOutOfRange(rank) => write!(f, "{code}: {rank}"),
            Self::BadPattern { field, problem } => write!(f, "{code}: {field}: {problem}"),
            Self::DuplicatePattern { field, value } => write!(f, "{code}: {field}: {value}"),
            Self::BadId(s)
            | Self::UnknownClass(s)
            | Self::UnknownOrigin(s)
            | Self::BadKernelAbi(s)
            | Self::BadMessageKey(s)
            | Self::BadContentPath(s) => write!(f, "{code}: {s}"),
        }
    }
}

impl std::error::Error for ManifestError {}

/// Serialized form of a manifest (plain strings, exactly the TOML layout).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawManifest {
    id: String,
    version: String,
    class: String,
    kernel_abi: Vec<String>,
    origin: String,
    #[serde(default)]
    rank: i32,
    name_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description_key: Option<String>,
    #[serde(rename = "match")]
    rules: RawMatch,
    content: RawContent,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawMatch {
    pci: Vec<String>,
    pci_class: Vec<String>,
    usb: Vec<String>,
    usb_class: Vec<String>,
    dt: Vec<String>,
    acpi: Vec<String>,
    dmi: Vec<String>,
    platform: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawContent {
    module: String,
    firmware: Vec<String>,
    hal_config: String,
}

/// Hardware selection rules of a driver (`[match]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchRules {
    /// PCI `vendor:device` patterns, device may be `*`.
    pub pci: Vec<IdPattern>,
    /// PCI class code prefixes (2, 4 or 6 hex digits).
    pub pci_class: Vec<ClassPrefix>,
    /// USB `vid:pid` patterns, product may be `*`.
    pub usb: Vec<IdPattern>,
    /// USB class/subclass/protocol prefixes.
    pub usb_class: Vec<ClassPrefix>,
    /// Device-tree `compatible` strings.
    pub dt: Vec<String>,
    /// ACPI hardware ids (stored upper-case).
    pub acpi: Vec<String>,
    /// Machine model patterns (`*` globs). If present, the driver only applies to matching machines.
    pub dmi: Vec<String>,
    /// Names of platform devices that have no bus id (e.g. `ps2`, `uefi-gop`).
    pub platform: Vec<String>,
}

/// What a package ships (`[content]`); all paths are relative to the payload root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Content {
    /// Kernel module.
    pub module: Option<String>,
    /// Firmware files.
    pub firmware: Vec<String>,
    /// HAL backend configuration.
    pub hal_config: Option<String>,
}

impl Content {
    /// All payload paths the manifest refers to.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.module
            .iter()
            .chain(self.firmware.iter())
            .chain(self.hal_config.iter())
            .map(String::as_str)
    }
}

/// A validated driver manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Unique driver identifier, e.g. `gen-intel-eth`.
    pub id: String,
    /// Driver version.
    pub version: Version,
    /// Functional class.
    pub class: DriverClass,
    /// Kernel ABIs the driver is built for; an entry may end in `*`.
    pub kernel_abi: Vec<String>,
    /// Where the driver comes from.
    pub origin: Origin,
    /// Preference among candidates of equal specificity; higher wins.
    pub rank: i32,
    /// i18n key of the display name (never literal text).
    pub name_key: String,
    /// Optional i18n key of a longer description.
    pub description_key: Option<String>,
    /// Hardware selection rules.
    pub rules: MatchRules,
    /// Shipped files.
    pub content: Content,
}

fn check_id(id: &str) -> Result<(), ManifestError> {
    let body = id
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_');
    let start = id
        .bytes()
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit());
    if body && start && id.len() <= 64 {
        Ok(())
    } else {
        Err(ManifestError::BadId(id.to_string()))
    }
}

fn check_message_key(key: &str) -> Result<(), ManifestError> {
    let chars = key
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'_'));
    let dotted =
        key.contains('.') && !key.starts_with('.') && !key.ends_with('.') && !key.contains("..");
    if chars && dotted && key.len() <= 128 {
        Ok(())
    } else {
        Err(ManifestError::BadMessageKey(key.to_string()))
    }
}

fn check_kernel_abi(entries: &[String]) -> Result<(), ManifestError> {
    if entries.is_empty() {
        return Err(ManifestError::NoKernelAbi);
    }
    for entry in entries {
        let stem = entry.strip_suffix('*').unwrap_or(entry);
        let chars = stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'));
        if stem.is_empty() || !chars || entry.len() > 64 {
            return Err(ManifestError::BadKernelAbi(entry.clone()));
        }
    }
    Ok(())
}

fn convert<T: PartialEq>(
    field: &'static str,
    values: &[String],
    parse: impl Fn(&str) -> Result<T, PatternError>,
) -> Result<Vec<T>, ManifestError> {
    let mut out: Vec<T> = Vec::with_capacity(values.len());
    for value in values {
        let parsed =
            parse(value).map_err(|problem| ManifestError::BadPattern { field, problem })?;
        if out.contains(&parsed) {
            return Err(ManifestError::DuplicatePattern {
                field,
                value: value.clone(),
            });
        }
        out.push(parsed);
    }
    Ok(out)
}

fn name(text: &str) -> Result<String, PatternError> {
    check_name(text).map(|()| text.to_string())
}

fn acpi_name(text: &str) -> Result<String, PatternError> {
    check_name(text).map(|()| text.to_ascii_uppercase())
}

fn content_path(path: &str) -> Result<String, ManifestError> {
    normalize_path(path).map_err(|_| ManifestError::BadContentPath(path.to_string()))
}

impl MatchRules {
    fn from_raw(raw: &RawMatch) -> Result<Self, ManifestError> {
        let rules = Self {
            pci: convert("pci", &raw.pci, IdPattern::parse)?,
            pci_class: convert("pci_class", &raw.pci_class, ClassPrefix::parse)?,
            usb: convert("usb", &raw.usb, IdPattern::parse)?,
            usb_class: convert("usb_class", &raw.usb_class, ClassPrefix::parse)?,
            dt: convert("dt", &raw.dt, name)?,
            acpi: convert("acpi", &raw.acpi, acpi_name)?,
            dmi: convert("dmi", &raw.dmi, name)?,
            platform: convert("platform", &raw.platform, name)?,
        };
        let selects = !(rules.pci.is_empty()
            && rules.pci_class.is_empty()
            && rules.usb.is_empty()
            && rules.usb_class.is_empty()
            && rules.dt.is_empty()
            && rules.acpi.is_empty()
            && rules.platform.is_empty());
        if selects {
            Ok(rules)
        } else {
            Err(ManifestError::NoMatchRule)
        }
    }

    fn to_raw(&self) -> RawMatch {
        fn texts<T: ToString>(items: &[T]) -> Vec<String> {
            items.iter().map(ToString::to_string).collect()
        }
        RawMatch {
            pci: texts(&self.pci),
            pci_class: texts(&self.pci_class),
            usb: texts(&self.usb),
            usb_class: texts(&self.usb_class),
            dt: self.dt.clone(),
            acpi: self.acpi.clone(),
            dmi: self.dmi.clone(),
            platform: self.platform.clone(),
        }
    }
}

impl Content {
    fn from_raw(raw: &RawContent) -> Result<Self, ManifestError> {
        let optional = |path: &str| {
            if path.is_empty() {
                Ok(None)
            } else {
                content_path(path).map(Some)
            }
        };
        let content = Self {
            module: optional(&raw.module)?,
            firmware: raw
                .firmware
                .iter()
                .map(|p| content_path(p))
                .collect::<Result<_, _>>()?,
            hal_config: optional(&raw.hal_config)?,
        };
        if content.paths().next().is_none() {
            return Err(ManifestError::NoContent);
        }
        Ok(content)
    }
}

impl Manifest {
    /// Parses and validates manifest bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ManifestError> {
        Self::parse(std::str::from_utf8(bytes).map_err(|_| ManifestError::NotUtf8)?)
    }

    /// Parses and validates manifest text.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let raw: RawManifest =
            toml::from_str(text).map_err(|e| ManifestError::Syntax(Box::new(e)))?;
        Self::from_raw(&raw)
    }

    pub(crate) fn from_raw(raw: &RawManifest) -> Result<Self, ManifestError> {
        check_id(&raw.id)?;
        check_kernel_abi(&raw.kernel_abi)?;
        if !RANK_RANGE.contains(&raw.rank) {
            return Err(ManifestError::RankOutOfRange(raw.rank));
        }
        check_message_key(&raw.name_key)?;
        if let Some(key) = &raw.description_key {
            check_message_key(key)?;
        }
        Ok(Self {
            id: raw.id.clone(),
            version: Version::parse(&raw.version).map_err(ManifestError::BadVersion)?,
            class: DriverClass::parse(&raw.class)
                .ok_or_else(|| ManifestError::UnknownClass(raw.class.clone()))?,
            kernel_abi: raw.kernel_abi.clone(),
            origin: Origin::parse(&raw.origin)
                .ok_or_else(|| ManifestError::UnknownOrigin(raw.origin.clone()))?,
            rank: raw.rank,
            name_key: raw.name_key.clone(),
            description_key: raw.description_key.clone(),
            rules: MatchRules::from_raw(&raw.rules)?,
            content: Content::from_raw(&raw.content)?,
        })
    }

    pub(crate) fn to_raw(&self) -> RawManifest {
        RawManifest {
            id: self.id.clone(),
            version: self.version.to_string(),
            class: self.class.as_str().to_string(),
            kernel_abi: self.kernel_abi.clone(),
            origin: self.origin.as_str().to_string(),
            rank: self.rank,
            name_key: self.name_key.clone(),
            description_key: self.description_key.clone(),
            rules: self.rules.to_raw(),
            content: RawContent {
                module: self.content.module.clone().unwrap_or_default(),
                firmware: self.content.firmware.clone(),
                hal_config: self.content.hal_config.clone().unwrap_or_default(),
            },
        }
    }

    /// Returns `true` when the driver is built for the running kernel ABI.
    pub fn supports_kernel(&self, kernel_abi: &str) -> bool {
        self.kernel_abi
            .iter()
            .any(|entry| match entry.strip_suffix('*') {
                Some(prefix) => kernel_abi.starts_with(prefix),
                None => entry == kernel_abi,
            })
    }
}
