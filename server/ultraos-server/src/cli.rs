//! The `ultraos-server admin …` command line: parses arguments and calls [`crate::admin`].
//!
//! It works directly on the database file and the file store, so it runs on the server host
//! only; there is no administrative HTTP interface. Results and errors are message keys that
//! [`crate::msg`] renders in Arabic, English or German.

use crate::admin::{self, AdminError, AppPublication, ReleasePublication};
use crate::db::Db;
use crate::entitlements::EntitlementKind;
use crate::files::FileStore;
use serde::de::DeserializeOwned;
use std::fmt;
use std::path::Path;

/// A successful command: message key, its arguments and extra plain lines (lists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    /// Message key (see [`crate::msg::MESSAGES`]).
    pub key: &'static str,
    /// Arguments of the message.
    pub args: Vec<String>,
    /// Additional untranslated lines (identifiers), printed after the message.
    pub lines: Vec<String>,
}

impl Outcome {
    fn new(key: &'static str, args: &[&str]) -> Self {
        Self {
            key,
            args: args.iter().map(|a| (*a).to_string()).collect(),
            lines: Vec::new(),
        }
    }
}

/// A failed command.
#[derive(Debug)]
pub enum CliError {
    /// Wrong command or arguments.
    Usage,
    /// The administrative operation failed.
    Admin(AdminError),
    /// The description file could not be read.
    ReadMeta(std::io::Error),
    /// The description file is not valid JSON of the expected shape.
    ParseMeta(serde_json::Error),
}

impl CliError {
    /// Message key for the output.
    pub fn message_key(&self) -> &'static str {
        match self {
            CliError::Usage => "admin.usage",
            CliError::Admin(e) => e.message_key(),
            CliError::ReadMeta(_) => "cli.error.read_meta",
            CliError::ParseMeta(_) => "cli.error.parse_meta",
        }
    }

    /// Untranslated technical detail (argument `{0}` of the message).
    pub fn detail(&self) -> String {
        match self {
            CliError::Usage => String::new(),
            CliError::Admin(e) => e.detail(),
            CliError::ReadMeta(e) => e.to_string(),
            CliError::ParseMeta(e) => e.to_string(),
        }
    }

    /// Process exit code: 2 for usage errors, 1 otherwise.
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage => 2,
            _ => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.message_key(), self.detail())
    }
}

impl std::error::Error for CliError {}

impl From<AdminError> for CliError {
    fn from(e: AdminError) -> Self {
        CliError::Admin(e)
    }
}

fn read_meta<T: DeserializeOwned>(path: &str) -> Result<T, CliError> {
    let text = std::fs::read_to_string(path).map_err(CliError::ReadMeta)?;
    serde_json::from_str(&text).map_err(CliError::ParseMeta)
}

fn switch(word: &str, on: &str, off: &str) -> Result<bool, CliError> {
    if word == on {
        Ok(true)
    } else if word == off {
        Ok(false)
    } else {
        Err(CliError::Usage)
    }
}

fn entitlement(
    db: &Db,
    grant: bool,
    user: &str,
    kind: &str,
    item: &str,
) -> Result<Outcome, CliError> {
    let kind = EntitlementKind::parse(kind).ok_or(CliError::Usage)?;
    let changed = if grant {
        admin::grant(db, user, kind, item)?
    } else {
        admin::revoke(db, user, kind, item)?
    };
    let key = match (grant, changed) {
        (true, true) => "admin.granted",
        (true, false) => "admin.already_granted",
        (false, true) => "admin.revoked",
        (false, false) => "admin.not_granted",
    };
    Ok(Outcome::new(key, &[user, kind.as_str(), item]))
}

fn list(db: &Db, user: &str) -> Result<Outcome, CliError> {
    let all = admin::entitlements_of(db, user)?;
    if all.is_empty() {
        return Ok(Outcome::new("admin.entitlements.none", &[user]));
    }
    let mut out = Outcome::new("admin.entitlements.header", &[user]);
    out.lines = all
        .iter()
        .map(|(kind, item)| format!("{} {item}", kind.as_str()))
        .collect();
    Ok(out)
}

fn publish_app(db: &Db, files: &FileStore, meta: &str, file: &str) -> Result<Outcome, CliError> {
    let p: AppPublication = read_meta(meta)?;
    let stored = admin::publish_app(db, files, &p, Path::new(file))?;
    let size = stored.size.to_string();
    Ok(Outcome::new(
        "admin.published.app",
        &[&p.id, &p.version, &size, &stored.sha256],
    ))
}

fn publish_release(
    db: &Db,
    files: &FileStore,
    meta: &str,
    file: &str,
) -> Result<Outcome, CliError> {
    let p: ReleasePublication = read_meta(meta)?;
    let (id, stored) = admin::publish_release(db, files, &p, Path::new(file))?;
    let (id, size) = (id.to_string(), stored.size.to_string());
    Ok(Outcome::new(
        "admin.published.release",
        &[&id, &size, &stored.sha256],
    ))
}

/// Runs one admin command. `args` are the words after `admin`.
pub fn run(args: &[String], db: &Db, files: &FileStore) -> Result<Outcome, CliError> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["grant", user, kind, item] => entitlement(db, true, user, kind, item),
        ["revoke", user, kind, item] => entitlement(db, false, user, kind, item),
        ["entitlements", user] => list(db, user),
        ["channel", name, mode] => {
            let gated = switch(mode, "gated", "open")?;
            admin::set_channel(db, name, gated)?;
            Ok(Outcome::new(
                if gated {
                    "admin.channel.gated"
                } else {
                    "admin.channel.open"
                },
                &[name],
            ))
        }
        ["publisher", name, mode] => {
            let verified = switch(mode, "verified", "unverified")?;
            admin::set_publisher(db, name, verified)?;
            let key = if verified {
                "admin.publisher.verified"
            } else {
                "admin.publisher.unverified"
            };
            Ok(Outcome::new(key, &[name]))
        }
        ["premium", app, mode] => {
            let premium = switch(mode, "on", "off")?;
            admin::set_premium(db, app, premium)?;
            Ok(Outcome::new(
                if premium {
                    "admin.premium.on"
                } else {
                    "admin.premium.off"
                },
                &[app],
            ))
        }
        ["publish-app", meta, file] => publish_app(db, files, meta, file),
        ["publish-release", meta, file] => publish_release(db, files, meta, file),
        _ => Err(CliError::Usage),
    }
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
