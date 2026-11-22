//! Checks a directory of `.ftl` catalogs (feature `std`); the logic behind `ultar-i18n-check`.
//!
//! The Arabic file `ar.ftl` is the reference. Every other `<lang>.ftl` must have exactly the same
//! keys with the same placeholders, and plural messages must carry the forms their language needs.

use std::fmt;
use std::io;
use std::path::{Path, PathBuf};
use std::string::{String, ToString};
use std::vec::Vec;

use crate::catalog::Catalog;
use crate::check::{compare_catalogs, incomplete_plurals, PlaceholderMismatch, PluralGap};
use crate::error::ParseError;
use crate::lang::Lang;
use crate::pattern::Arg;
use crate::translator::Translator;

/// Name of the reference catalog inside a locales directory.
pub const REFERENCE_FILE: &str = "ar.ftl";

/// One finding in a catalog file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The file could not be parsed.
    Parse {
        /// File name.
        file: String,
        /// The parse error.
        error: ParseError,
    },
    /// A key of the reference is missing in the file.
    Missing {
        /// File name.
        file: String,
        /// The missing key.
        key: String,
    },
    /// The file has a key that the reference does not have.
    Extra {
        /// File name.
        file: String,
        /// The surplus key.
        key: String,
    },
    /// A message uses other placeholders than the reference.
    Placeholders {
        /// File name.
        file: String,
        /// The difference.
        mismatch: PlaceholderMismatch,
    },
    /// A plural message lacks forms its language needs.
    Plurals {
        /// File name.
        file: String,
        /// The missing forms.
        gap: PluralGap,
    },
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "-".to_string()
    } else {
        items.join(", ")
    }
}

impl Problem {
    /// Describes the finding in the translator's language (catalog keys `check.*`).
    pub fn describe(&self, translator: &Translator) -> String {
        match self {
            Problem::Parse { file, error } => {
                let reason = translator.t(error.kind.message_key());
                let line = i64::try_from(error.line).unwrap_or(i64::MAX);
                translator.t_args(
                    "check.parse",
                    &[
                        ("file", Arg::Str(file)),
                        ("line", Arg::Int(line)),
                        ("reason", Arg::Str(&reason)),
                    ],
                )
            }
            Problem::Missing { file, key } => translator.t_args(
                "check.missing",
                &[("file", Arg::Str(file)), ("key", Arg::Str(key))],
            ),
            Problem::Extra { file, key } => translator.t_args(
                "check.extra",
                &[
                    ("file", Arg::Str(file)),
                    ("key", Arg::Str(key)),
                    ("reference", Arg::Str(REFERENCE_FILE)),
                ],
            ),
            Problem::Placeholders { file, mismatch } => translator.t_args(
                "check.placeholders",
                &[
                    ("file", Arg::Str(file)),
                    ("key", Arg::Str(&mismatch.key)),
                    ("missing", Arg::Str(&list(&mismatch.missing))),
                    ("unexpected", Arg::Str(&list(&mismatch.unexpected))),
                ],
            ),
            Problem::Plurals { file, gap } => {
                let names: Vec<String> = gap
                    .missing
                    .iter()
                    .map(|c| c.keyword().to_string())
                    .collect();
                translator.t_args(
                    "check.plurals",
                    &[
                        ("file", Arg::Str(file)),
                        ("key", Arg::Str(&gap.key)),
                        ("categories", Arg::Str(&list(&names))),
                    ],
                )
            }
        }
    }
}

/// Reasons why a directory could not be checked at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckError {
    /// A directory or file could not be read.
    Io {
        /// The path that failed.
        path: PathBuf,
        /// The kind of I/O error.
        kind: io::ErrorKind,
    },
    /// The directory contains no `.ftl` files.
    NoCatalogs {
        /// The directory.
        dir: PathBuf,
    },
    /// The reference catalog `ar.ftl` is missing.
    NoReference {
        /// The directory.
        dir: PathBuf,
    },
}

impl CheckError {
    /// Describes the error in the translator's language (catalog keys `check.*`).
    pub fn describe(&self, translator: &Translator) -> String {
        match self {
            CheckError::Io { path, .. } => translator.t_args(
                "check.io",
                &[("path", Arg::Str(&path.display().to_string()))],
            ),
            CheckError::NoCatalogs { dir } => translator.t_args(
                "check.no_catalogs",
                &[("dir", Arg::Str(&dir.display().to_string()))],
            ),
            CheckError::NoReference { dir } => translator.t_args(
                "check.no_reference",
                &[
                    ("reference", Arg::Str(REFERENCE_FILE)),
                    ("dir", Arg::Str(&dir.display().to_string())),
                ],
            ),
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckError::Io { path, kind } => write!(f, "cannot read {}: {kind}", path.display()),
            CheckError::NoCatalogs { dir } => write!(f, "no .ftl files in {}", dir.display()),
            CheckError::NoReference { dir } => {
                write!(f, "{REFERENCE_FILE} missing in {}", dir.display())
            }
        }
    }
}

impl std::error::Error for CheckError {}

/// Result of checking a set of catalog files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Names of the checked files, sorted.
    pub files: Vec<String>,
    /// All findings, in file order.
    pub problems: Vec<Problem>,
}

impl Report {
    /// `true` if nothing was found.
    pub fn is_clean(&self) -> bool {
        self.problems.is_empty()
    }
}

fn check_plurals(file: &str, catalog: &Catalog, problems: &mut Vec<Problem>) {
    let Some(lang) = file.strip_suffix(".ftl").and_then(Lang::parse) else {
        return;
    };
    for gap in incomplete_plurals(catalog, lang) {
        problems.push(Problem::Plurals {
            file: file.to_string(),
            gap,
        });
    }
}

fn check_against_reference(
    file: &str,
    reference: &Catalog,
    catalog: &Catalog,
    problems: &mut Vec<Problem>,
) {
    let comparison = compare_catalogs(reference, catalog);
    let name = || file.to_string();
    problems.extend(
        comparison
            .missing
            .into_iter()
            .map(|key| Problem::Missing { file: name(), key }),
    );
    problems.extend(
        comparison
            .extra
            .into_iter()
            .map(|key| Problem::Extra { file: name(), key }),
    );
    problems.extend(
        comparison
            .placeholders
            .into_iter()
            .map(|mismatch| Problem::Placeholders {
                file: name(),
                mismatch,
            }),
    );
}

/// Checks catalog sources given as `(file name, content)` pairs. One of them must be `ar.ftl`.
///
/// Returns `None` if the reference file is not among the sources.
pub fn check_sources(sources: &[(String, String)]) -> Option<Report> {
    let mut sorted: Vec<&(String, String)> = sources.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let reference_source = sorted.iter().find(|(name, _)| name == REFERENCE_FILE)?;
    let mut report = Report {
        files: sorted.iter().map(|(name, _)| name.clone()).collect(),
        problems: Vec::new(),
    };
    let reference = match Catalog::parse(&reference_source.1) {
        Ok(catalog) => Some(catalog),
        Err(error) => {
            report.problems.push(Problem::Parse {
                file: REFERENCE_FILE.to_string(),
                error,
            });
            None
        }
    };
    if let Some(catalog) = &reference {
        check_plurals(REFERENCE_FILE, catalog, &mut report.problems);
    }
    for (file, content) in sorted.iter().filter(|(name, _)| name != REFERENCE_FILE) {
        match Catalog::parse(content) {
            Err(error) => report.problems.push(Problem::Parse {
                file: file.clone(),
                error,
            }),
            Ok(catalog) => {
                if let Some(reference) = &reference {
                    check_against_reference(file, reference, &catalog, &mut report.problems);
                }
                check_plurals(file, &catalog, &mut report.problems);
            }
        }
    }
    Some(report)
}

/// Reads all `*.ftl` files of `dir` (not recursive) and checks them against `ar.ftl`.
pub fn check_directory(dir: &Path) -> Result<Report, CheckError> {
    let io_error = |path: &Path, error: &io::Error| CheckError::Io {
        path: path.to_path_buf(),
        kind: error.kind(),
    };
    let entries = std::fs::read_dir(dir).map_err(|e| io_error(dir, &e))?;
    let mut sources = Vec::new();
    for entry in entries {
        let path = entry.map_err(|e| io_error(dir, &e))?.path();
        let is_catalog = path.is_file() && path.extension().is_some_and(|ext| ext == "ftl");
        let Some(name) = path
            .file_name()
            .and_then(|n| n.to_str())
            .filter(|_| is_catalog)
        else {
            continue;
        };
        let content = std::fs::read_to_string(&path).map_err(|e| io_error(&path, &e))?;
        sources.push((name.to_string(), content));
    }
    if sources.is_empty() {
        return Err(CheckError::NoCatalogs {
            dir: dir.to_path_buf(),
        });
    }
    check_sources(&sources).ok_or_else(|| CheckError::NoReference {
        dir: dir.to_path_buf(),
    })
}

#[cfg(test)]
#[path = "check_dir_tests.rs"]
mod tests;
