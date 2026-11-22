//! Data the server sends: catalogue entries, app details, releases, entitlements.

use serde::Deserialize;

/// Publisher of an app.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Publisher {
    /// Display name.
    pub name: String,
    /// True when KM Code Craft verified the publisher.
    #[serde(default)]
    pub verified: bool,
}

/// One catalogue entry.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppSummary {
    /// Unique app id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description text.
    #[serde(default)]
    pub description: String,
    /// Category id.
    #[serde(default)]
    pub category: String,
    /// Publisher and verification state.
    pub publisher: Publisher,
    /// True when the download needs an account whose access includes the app.
    #[serde(default)]
    pub premium: bool,
    /// Current version.
    pub version: String,
    /// Download size in bytes.
    pub size: u64,
}

/// One entry of an app's version history.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct VersionInfo {
    /// Version string.
    pub version: String,
    /// Download size in bytes.
    pub size: u64,
    /// SHA-256 of the package.
    pub sha256: String,
    /// What changed.
    #[serde(default)]
    pub changelog: String,
    /// Publication time (Unix seconds, 0 = unknown).
    #[serde(default)]
    pub created: i64,
}

/// Full description of one app.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppDetails {
    /// The catalogue entry.
    #[serde(flatten)]
    pub summary: AppSummary,
    /// Size after installation in bytes (0 = unknown).
    #[serde(default)]
    pub installed_size: u64,
    /// SHA-256 of the current package.
    pub sha256: String,
    /// Permissions the app asks for.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// File types the app opens.
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Download path, relative to the server.
    pub download: String,
    /// Version history, newest first.
    #[serde(default)]
    pub versions: Vec<VersionInfo>,
}

/// A page of catalogue entries.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct CatalogPage {
    /// Number of entries matching the query.
    pub total: u64,
    /// Page number, starting at 1.
    pub page: u32,
    /// Page size.
    pub per_page: u32,
    /// Entries of this page.
    pub items: Vec<AppSummary>,
}

/// A category with its app count.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Category {
    /// Category id.
    pub id: String,
    /// Number of apps.
    pub count: u64,
}

/// A catalogue query. Empty fields mean "no filter".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogQuery {
    /// Free text: name, description; a leading dot searches by file type (".xlsx").
    pub text: String,
    /// Category id.
    pub category: String,
    /// File type without dot.
    pub file_type: String,
    /// Page number, starting at 1.
    pub page: u32,
    /// Page size (the server accepts 1–100).
    pub per_page: u32,
}

impl Default for CatalogQuery {
    fn default() -> Self {
        Self {
            text: String::new(),
            category: String::new(),
            file_type: String::new(),
            page: 1,
            per_page: 20,
        }
    }
}

/// Entry of the unpaged store listing (kept for existing callers).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct AppMeta {
    /// App id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Current version.
    pub version: String,
    /// Download size in bytes.
    pub size: i64,
    /// SHA-256 of the package.
    pub sha256: String,
    /// Detached signature field (the binding signature is inside the package).
    pub sig: String,
    /// Download location (absolute, or relative to the server).
    pub url: String,
    /// True when the download needs access.
    #[serde(default)]
    pub premium: bool,
}

/// A system release offered by an update channel.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Release {
    /// Numeric release id (0 from servers that predate ids).
    #[serde(default)]
    pub id: i64,
    /// Channel name.
    pub channel: String,
    /// Version string.
    pub version: String,
    /// Target architecture or `any`.
    #[serde(default)]
    pub arch: String,
    /// Target edition or `any`.
    #[serde(default)]
    pub edition: String,
    /// Image size in bytes (0 = unknown).
    #[serde(default)]
    pub size: u64,
    /// SHA-256 of the image (empty = unknown).
    #[serde(default)]
    pub sha256: String,
    /// Download location (absolute, or relative to the server).
    pub url: String,
    /// dm-verity root hash, verified by the updater against the signature.
    pub roothash: String,
    /// Publisher signature (base64).
    pub sig: String,
}

/// What the signed-in account has access to.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Entitlements {
    /// Ids of premium apps.
    #[serde(default)]
    pub apps: Vec<String>,
    /// Names of gated update channels.
    #[serde(default)]
    pub channels: Vec<String>,
}

/// How the store presents an app's availability.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAccess {
    /// Free: can be installed without an account.
    Free,
    /// Premium and nobody is signed in.
    PremiumSignInRequired,
    /// Premium, signed in, but not part of the account's access.
    PremiumNotIncluded,
    /// Premium and unlocked for the signed-in account.
    Unlocked,
}

impl AppAccess {
    /// Derives the state. `entitlements` is `None` when nobody is signed in.
    pub fn of(app: &AppSummary, entitlements: Option<&Entitlements>) -> Self {
        match (app.premium, entitlements) {
            (false, _) => AppAccess::Free,
            (true, None) => AppAccess::PremiumSignInRequired,
            (true, Some(e)) if e.apps.iter().any(|id| id == &app.id) => AppAccess::Unlocked,
            (true, Some(_)) => AppAccess::PremiumNotIncluded,
        }
    }

    /// Message key of the state label.
    pub fn message_key(self) -> &'static str {
        match self {
            AppAccess::Free => "cloud.store.state.free",
            AppAccess::PremiumSignInRequired => "cloud.store.state.premium_sign_in",
            AppAccess::PremiumNotIncluded => "cloud.store.state.premium_not_included",
            AppAccess::Unlocked => "cloud.store.state.unlocked",
        }
    }

    /// True when the install button can work right now.
    pub fn can_install(self) -> bool {
        matches!(self, AppAccess::Free | AppAccess::Unlocked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, premium: bool) -> AppSummary {
        AppSummary {
            id: id.into(),
            name: "N".into(),
            description: String::new(),
            category: "other".into(),
            publisher: Publisher {
                name: "P".into(),
                verified: false,
            },
            premium,
            version: "1".into(),
            size: 1,
        }
    }

    #[test]
    fn access_states() {
        let mine = Entitlements {
            apps: vec!["a.pro".into()],
            channels: vec![],
        };
        assert_eq!(AppAccess::of(&app("a.free", false), None), AppAccess::Free);
        assert_eq!(
            AppAccess::of(&app("a.free", false), Some(&mine)),
            AppAccess::Free
        );
        assert_eq!(
            AppAccess::of(&app("a.pro", true), None),
            AppAccess::PremiumSignInRequired
        );
        assert_eq!(
            AppAccess::of(&app("a.pro", true), Some(&mine)),
            AppAccess::Unlocked
        );
        assert_eq!(
            AppAccess::of(&app("a.other", true), Some(&mine)),
            AppAccess::PremiumNotIncluded
        );
        assert!(AppAccess::Free.can_install() && AppAccess::Unlocked.can_install());
        assert!(!AppAccess::PremiumSignInRequired.can_install());
        assert!(!AppAccess::PremiumNotIncluded.can_install());
    }

    #[test]
    fn details_parse_with_flattened_summary_and_defaults() {
        let json = r#"{"id":"a","name":"A","publisher":{"name":"P"},"version":"1","size":5,
                       "sha256":"ab","download":"/v1/store/apps/a/download"}"#;
        let d: AppDetails = serde_json::from_str(json).unwrap();
        assert_eq!(d.summary.id, "a");
        assert!(!d.summary.premium && !d.summary.publisher.verified);
        assert!(d.versions.is_empty() && d.permissions.is_empty());
        let old = r#"{"channel":"stable","version":"1","url":"u","roothash":"r","sig":"s"}"#;
        let r: Release = serde_json::from_str(old).unwrap();
        assert_eq!((r.id, r.size, r.sha256.as_str()), (0, 0, ""));
        assert_eq!(CatalogQuery::default().page, 1);
    }
}
