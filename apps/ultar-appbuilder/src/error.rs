//! Errors and project findings; every variant names an i18n key (APP-RULES §8).

use std::fmt;

/// A problem found in a project by [`crate::project::check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Issue {
    /// No `ultar-app.toml`.
    NoManifest,
    /// The manifest is not valid TOML.
    ManifestDamaged,
    /// The id is not `<publisher>.<app>` or does not match the publisher.
    BadId,
    /// The version is not `x.y.z`.
    BadVersion,
    /// No publisher.
    NoPublisher,
    /// A kind other than `native` or `data`.
    BadKind,
    /// Name or description are plain text instead of `app.` keys.
    NameNotKey,
    /// `arch` is missing or unknown.
    BadArch,
    /// The icon file does not exist.
    NoIcon,
    /// `payload/` is missing or empty.
    NoPayload,
    /// The `exec` program is not in the payload.
    ExecMissing,
    /// `network` has an unknown value.
    BadNetwork,
    /// A required section is missing (`form`, `permissions`, `limits`).
    MissingSection(&'static str),
}

impl Issue {
    /// The i18n key (with the `section` argument for [`Issue::MissingSection`]).
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::NoManifest => "appbuilder.issue.no-manifest",
            Self::ManifestDamaged => "appbuilder.issue.manifest-damaged",
            Self::BadId => "appbuilder.issue.bad-id",
            Self::BadVersion => "appbuilder.issue.bad-version",
            Self::NoPublisher => "appbuilder.issue.no-publisher",
            Self::BadKind => "appbuilder.issue.bad-kind",
            Self::NameNotKey => "appbuilder.issue.name-not-key",
            Self::BadArch => "appbuilder.issue.bad-arch",
            Self::NoIcon => "appbuilder.issue.no-icon",
            Self::NoPayload => "appbuilder.issue.no-payload",
            Self::ExecMissing => "appbuilder.issue.exec-missing",
            Self::BadNetwork => "appbuilder.issue.bad-network",
            Self::MissingSection(_) => "appbuilder.issue.missing-section",
        }
    }
}

/// What can go wrong while building or installing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppError {
    /// The project has problems; nothing was built.
    Issues(Vec<Issue>),
    /// A file could not be read or written.
    Io,
    /// The developer key could not be created or read.
    Key,
    /// Packing failed.
    Pack,
    /// The built package did not verify against the developer key (deleted).
    Verify,
    /// Install was asked before a package was built.
    NotChecked,
    /// No launcher on the bus.
    NoLauncher,
    /// The launcher refused the package (untrusted key or missing right).
    Refused,
}

impl AppError {
    /// The i18n key of this error (for [`AppError::Issues`]: the first issue's key).
    pub fn message_key(&self) -> &'static str {
        match self {
            Self::Issues(list) => list
                .first()
                .map_or("appbuilder.error.pack", Issue::message_key),
            Self::Io => "appbuilder.error.io",
            Self::Key => "appbuilder.error.key",
            Self::Pack => "appbuilder.error.pack",
            Self::Verify => "appbuilder.error.verify",
            Self::NotChecked => "appbuilder.error.not-checked",
            Self::NoLauncher => "appbuilder.error.no-launcher",
            Self::Refused => "appbuilder.error.refused",
        }
    }
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message_key())
    }
}

impl std::error::Error for AppError {}

impl From<std::io::Error> for AppError {
    fn from(_: std::io::Error) -> Self {
        Self::Io
    }
}
