//! The catalogue data (DESKTOP-SPEC §7.2/§7.3): components from Ultar and their
//! permissions in plain words. The wire format is a TOML document signed as a whole
//! ([`crate::source`] verifies it); this module only knows the data.

use serde::{Deserialize, Serialize};
use ultar_i18n::Lang;

/// Catalogue format this app understands; a newer `format_version` is refused.
pub const FORMAT_VERSION: u16 = 1;

/// Text in the three catalog languages; Arabic is the reference (APP-RULES §1).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Loc {
    /// Arabic (required, the reference text).
    pub ar: String,
    /// English (required).
    pub en: String,
    /// German (optional; falls back to English).
    #[serde(default)]
    pub de: Option<String>,
}

impl Loc {
    /// The text for one language; missing German falls back to English.
    pub fn text(&self, lang: Lang) -> &str {
        match lang {
            Lang::Ar => &self.ar,
            Lang::En => &self.en,
            Lang::De => self.de.as_deref().unwrap_or(&self.en),
        }
    }

    /// All three texts; search matches every language, not only the shown one.
    pub fn texts(&self) -> [&str; 3] {
        [&self.ar, &self.en, self.de.as_deref().unwrap_or(&self.en)]
    }
}

/// Where an app comes from (source shown on the app page, DESKTOP-SPEC §7.3).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// A component of Ultar itself.
    Ultar,
    /// A Linux program (`.deb` area).
    Linux,
    /// A Windows program (`.exe`/`.msi` cell).
    Windows,
    /// An Android app (sealed runtime).
    Android,
}

impl Source {
    /// The stable word for the catalogue file.
    pub const fn id(self) -> &'static str {
        match self {
            Self::Ultar => "ultar",
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Android => "android",
        }
    }

    /// The name as shown (never translated: these are format names, like device names).
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ultar => "Ultar",
            Self::Linux => "Linux",
            Self::Windows => "Windows",
            Self::Android => "Android",
        }
    }
}

/// One permission an app asks for, with the reason in plain words.
/// The name itself comes from the catalog (`store.perm.<id>`); the reason belongs
/// to the catalogue entry (APP-RULES §3: every right needs a justification per app).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Permission {
    /// Permission id (`files`, `network`, `printing`, …).
    pub id: String,
    /// What it is used for, in the user's language.
    pub reason: Loc,
}

/// One app or component in the catalogue.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CatalogApp {
    /// Package id (same character set as `ultar_pkg::valid_id`).
    pub id: String,
    /// Display name.
    pub name: Loc,
    /// One-line summary (cards).
    pub summary: Loc,
    /// Full description (app page).
    pub description: Loc,
    /// Publisher as written by the signer (data, not translated).
    pub publisher: String,
    /// Store category id (`office`, `media`, …).
    pub category: String,
    /// Icon name from the system set (`ultar_ui::icon::IconId::name()`).
    pub icon: String,
    /// Origin of the program.
    pub source: Source,
    /// Version offered by this catalogue (SemVer string; `None` = unknown at run time).
    #[serde(default)]
    pub version: Option<String>,
    /// Download size in bytes (`None` = catalogue does not carry it; shown as `—`).
    #[serde(default)]
    pub size_download: Option<u64>,
    /// Installed size in bytes.
    #[serde(default)]
    pub size_installed: Option<u64>,
    /// File types this app opens (`.xlsx` finds Sheets; searched directly).
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Requested permissions with their plain-word reasons.
    #[serde(default)]
    pub permissions: Vec<Permission>,
    /// Ids of other catalogue entries this one needs (Firefox → Linux programs).
    #[serde(default)]
    pub requires: Vec<String>,
    /// The package file and its pinned digest (`package`, `sha256` in the file).
    #[serde(flatten)]
    pub pin: PackagePin,
}

/// Where an entry's package is and what it must hash to.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PackagePin {
    /// Package file next to the catalogue (`None` = not offered offline).
    #[serde(default)]
    pub package: Option<String>,
    /// SHA-256 of the package file (64 lowercase hex). Pinned by the signed catalogue: a
    /// downloaded package must match it before the launcher sees it (required for the
    /// online source).
    #[serde(default)]
    pub sha256: Option<String>,
}

impl CatalogApp {
    /// True when the version string is higher than `current` (numeric SemVer parts;
    /// unequal length compares element-wise, missing = 0).
    pub fn is_update_for(&self, current: &str) -> bool {
        match &self.version {
            Some(v) => version_lt(current, v),
            None => false,
        }
    }
}

/// `a < b` over dotted numbers (non-numeric tails ignored).
fn version_lt(a: &str, b: &str) -> bool {
    let parts = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|p| p.trim_start_matches('v'))
            .map(|p| p.parse::<u64>().unwrap_or_default())
            .collect()
    };
    let (x, y) = (parts(a), parts(b));
    for i in 0..x.len().max(y.len()) {
        let (xi, yi) = (*x.get(i).unwrap_or(&0), *y.get(i).unwrap_or(&0));
        if xi != yi {
            return xi < yi;
        }
    }
    false
}

/// The whole catalogue file.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Catalog {
    /// Format version of this document.
    pub format_version: u16,
    /// Publication date as text (`YYYY-MM-DD`, data).
    #[serde(default)]
    pub generated: Option<String>,
    /// The entries, in catalogue order.
    #[serde(default, rename = "app")]
    pub apps: Vec<CatalogApp>,
}

impl Catalog {
    /// Parses the TOML body; refuses newer formats and missing halves of an id/name pair.
    ///
    /// [`crate::source`] verifies the signature before this is trusted.
    pub fn parse(text: &str) -> Result<Catalog, crate::error::SourceError> {
        let catalog: Catalog =
            toml::from_str(text).map_err(|_| crate::error::SourceError::Damaged)?;
        if catalog.format_version > FORMAT_VERSION {
            return Err(crate::error::SourceError::TooNew);
        }
        if catalog.apps.iter().any(|a| {
            !valid_catalog_id(&a.id)
                || a.name.ar.is_empty()
                || a.name.en.is_empty()
                || a.pin
                    .package
                    .as_deref()
                    .is_some_and(|p| !valid_package_file(p))
                || a.pin.sha256.as_deref().is_some_and(|h| !valid_sha256(h))
        }) {
            return Err(crate::error::SourceError::Damaged);
        }
        Ok(catalog)
    }

    /// The entry with this id.
    pub fn app(&self, id: &str) -> Option<&CatalogApp> {
        self.apps.iter().find(|a| a.id == id)
    }

    /// Category ids in first-appearance order.
    pub fn categories(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for a in &self.apps {
            if !out.contains(&a.category.as_str()) {
                out.push(&a.category);
            }
        }
        out
    }

    /// All entries of one category.
    pub fn in_category(&self, category: &str) -> Vec<&CatalogApp> {
        self.apps
            .iter()
            .filter(|a| a.category == category)
            .collect()
    }
}

/// Ids in a catalogue must be safe as directory names (same rule as packages).
fn valid_catalog_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    bytes.first().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.len() <= 128
        && !id.contains("..")
        && bytes
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 64 lowercase hex digits.
pub fn valid_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A package must be a plain `.uap` file next to the catalogue: no path
/// separators, no `..`, no hidden names (the path is built from it).
fn valid_package_file(name: &str) -> bool {
    valid_catalog_id(name) && name.ends_with(".uap")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, version: &str) -> CatalogApp {
        CatalogApp {
            id: id.into(),
            name: Loc {
                ar: "a".into(),
                en: "S".into(),
                de: None,
            },
            summary: Loc {
                ar: "a".into(),
                en: "S".into(),
                de: None,
            },
            description: Loc {
                ar: "a".into(),
                en: "S".into(),
                de: None,
            },
            publisher: "p".into(),
            category: "c".into(),
            icon: "office".into(),
            source: Source::Ultar,
            version: Some(version.into()),
            size_download: None,
            size_installed: None,
            file_types: Vec::new(),
            permissions: Vec::new(),
            requires: Vec::new(),
            pin: crate::catalog::PackagePin {
                package: None,
                sha256: None,
            },
        }
    }

    #[test]
    fn version_compare_over_dotted_numbers() {
        assert!(version_lt("1.2.0", "1.10.0"));
        assert!(version_lt("1.2", "1.2.1"));
        assert!(!version_lt("2.0.0", "1.9.9"));
        assert!(!version_lt("1.0.0", "1.0.0"));
        let a = app("x", "2.0.0");
        assert!(a.is_update_for("1.9.9"));
        assert!(!a.is_update_for("2.0.0"));
    }

    #[test]
    fn newer_format_is_refused() {
        let text = "format_version = 2\n";
        assert_eq!(Catalog::parse(text), Err(crate::error::SourceError::TooNew));
    }

    #[test]
    fn damaged_entries_are_refused() {
        let text = "format_version = 1\n\n[[app]]\nid = \"bad/../id\"\n";
        assert_eq!(
            Catalog::parse(text),
            Err(crate::error::SourceError::Damaged)
        );
    }

    #[test]
    fn package_must_be_a_plain_uap_file_name() {
        assert!(valid_package_file("km-office.uap"));
        for bad in ["../x.uap", "/etc/x.uap", "a/b.uap", ".x.uap", "x.tar", ""] {
            assert!(!valid_package_file(bad), "{bad}");
        }
    }
}
