//! Administrative operations: publishing packages and releases, channel and publisher settings,
//! granting and revoking entitlements.
//!
//! These functions are not reachable over HTTP. They are called by the `ultraos-server admin`
//! command on the server itself (see [`crate::cli`]) and write an audit row for every change.
//! There is no payment logic: an entitlement exists because an administrator recorded it.

use crate::db::{self, Db};
use crate::entitlements::{self, EntitlementKind};
use crate::files::{FileError, FileStore, StoredFile};
use crate::search::{self, valid_ident};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use std::fmt;
use std::path::Path;

/// Errors of administrative operations.
#[derive(Debug)]
pub enum AdminError {
    /// No account has this username.
    UnknownAccount,
    /// The app or channel does not exist.
    UnknownItem,
    /// A field of the input is missing, too long or malformed.
    InvalidField(&'static str),
    /// This version of the app (or this release) was already published; versions are immutable.
    AlreadyPublished,
    /// An app can only be premium when its package is in the server's file store.
    PremiumNeedsStoredFile,
    /// Database failure.
    Db(rusqlite::Error),
    /// File store failure.
    File(FileError),
}

impl AdminError {
    /// Message key for the command-line output (see [`crate::msg`]).
    pub fn message_key(&self) -> &'static str {
        match self {
            AdminError::UnknownAccount => "admin.error.unknown_account",
            AdminError::UnknownItem => "admin.error.unknown_item",
            AdminError::InvalidField(_) => "admin.error.invalid_field",
            AdminError::AlreadyPublished => "admin.error.already_published",
            AdminError::PremiumNeedsStoredFile => "admin.error.premium_needs_file",
            AdminError::Db(_) => "admin.error.database",
            AdminError::File(_) => "admin.error.file",
        }
    }

    /// Technical detail for the message (field name or underlying error), not translated.
    pub fn detail(&self) -> String {
        match self {
            AdminError::InvalidField(field) => (*field).to_string(),
            AdminError::Db(e) => e.to_string(),
            AdminError::File(e) => e.to_string(),
            _ => String::new(),
        }
    }
}

impl fmt::Display for AdminError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.message_key(), self.detail())
    }
}

impl std::error::Error for AdminError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AdminError::Db(e) => Some(e),
            AdminError::File(e) => Some(e),
            _ => None,
        }
    }
}

impl From<rusqlite::Error> for AdminError {
    fn from(e: rusqlite::Error) -> Self {
        AdminError::Db(e)
    }
}

impl From<FileError> for AdminError {
    fn from(e: FileError) -> Self {
        AdminError::File(e)
    }
}

const WHO: &str = "admin";

fn account_id(conn: &Connection, username: &str) -> Result<i64, AdminError> {
    conn.query_row(
        "SELECT id FROM accounts WHERE username = ?1",
        [username],
        |r| r.get(0),
    )
    .optional()?
    .ok_or(AdminError::UnknownAccount)
}

fn item_exists(conn: &Connection, kind: EntitlementKind, item: &str) -> rusqlite::Result<bool> {
    let sql = match kind {
        EntitlementKind::App => "SELECT COUNT(*) FROM store_apps WHERE id = ?1",
        EntitlementKind::Channel => "SELECT COUNT(*) FROM update_channels WHERE name = ?1",
    };
    Ok(conn.query_row(sql, [item], |r| r.get::<_, i64>(0))? > 0)
}

/// Grants `username` access to a premium app or gated channel. `Ok(false)`: already granted.
pub fn grant(
    db: &Db,
    username: &str,
    kind: EntitlementKind,
    item: &str,
) -> Result<bool, AdminError> {
    let conn = db.lock();
    let account = account_id(&conn, username)?;
    if !item_exists(&conn, kind, item)? {
        return Err(AdminError::UnknownItem);
    }
    let changed = entitlements::grant(&conn, account, kind, item)?;
    if changed {
        db::audit(
            &conn,
            WHO,
            "entitlement.grant",
            &format!("{account} {} {item}", kind.as_str()),
        )?;
    }
    Ok(changed)
}

/// Revokes an entitlement. `Ok(false)`: there was none.
pub fn revoke(
    db: &Db,
    username: &str,
    kind: EntitlementKind,
    item: &str,
) -> Result<bool, AdminError> {
    let conn = db.lock();
    let account = account_id(&conn, username)?;
    let changed = entitlements::revoke(&conn, account, kind, item)?;
    if changed {
        db::audit(
            &conn,
            WHO,
            "entitlement.revoke",
            &format!("{account} {} {item}", kind.as_str()),
        )?;
    }
    Ok(changed)
}

/// Lists the entitlements of an account as `(kind, item)`, sorted.
pub fn entitlements_of(
    db: &Db,
    username: &str,
) -> Result<Vec<(EntitlementKind, String)>, AdminError> {
    let conn = db.lock();
    let account = account_id(&conn, username)?;
    let mut stmt = conn
        .prepare("SELECT kind, item FROM entitlements WHERE account_id = ?1 ORDER BY kind, item")?;
    let rows = stmt.query_map([account], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (kind, item) = row?;
        if let Some(kind) = EntitlementKind::parse(&kind) {
            out.push((kind, item));
        }
    }
    Ok(out)
}

/// Creates a channel or changes whether it is gated.
pub fn set_channel(db: &Db, name: &str, gated: bool) -> Result<(), AdminError> {
    if !valid_ident(name) || name.len() > 32 {
        return Err(AdminError::InvalidField("channel"));
    }
    let conn = db.lock();
    conn.execute(
        "INSERT INTO update_channels(name, gated) VALUES(?1, ?2) \
         ON CONFLICT(name) DO UPDATE SET gated = excluded.gated",
        rusqlite::params![name, i64::from(gated)],
    )?;
    db::audit(&conn, WHO, "channel.set", &format!("{name} gated={gated}"))?;
    Ok(())
}

/// Marks a publisher as verified or not (shown on the app page).
pub fn set_publisher(db: &Db, name: &str, verified: bool) -> Result<(), AdminError> {
    if name.trim().is_empty() || name.chars().count() > 100 {
        return Err(AdminError::InvalidField("publisher"));
    }
    let conn = db.lock();
    conn.execute(
        "INSERT INTO publishers(name, verified) VALUES(?1, ?2) \
         ON CONFLICT(name) DO UPDATE SET verified = excluded.verified",
        rusqlite::params![name, i64::from(verified)],
    )?;
    db::audit(
        &conn,
        WHO,
        "publisher.set",
        &format!("{name} verified={verified}"),
    )?;
    Ok(())
}

/// Makes an app premium or free. Premium needs the current package in the file store, because
/// only files delivered by the server itself can be gated.
pub fn set_premium(db: &Db, app_id: &str, premium: bool) -> Result<(), AdminError> {
    let conn = db.lock();
    let stored: Option<i64> = conn
        .query_row(
            "SELECT stored FROM store_apps WHERE id = ?1",
            [app_id],
            |r| r.get(0),
        )
        .optional()?;
    match stored {
        None => return Err(AdminError::UnknownItem),
        Some(0) if premium => return Err(AdminError::PremiumNeedsStoredFile),
        Some(_) => {}
    }
    conn.execute(
        "UPDATE store_apps SET premium = ?1 WHERE id = ?2",
        rusqlite::params![i64::from(premium), app_id],
    )?;
    db::audit(
        &conn,
        WHO,
        "app.premium",
        &format!("{app_id} premium={premium}"),
    )?;
    Ok(())
}

/// Description of an app version to publish (read from a JSON file by the command line).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AppPublication {
    /// Unique app id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Version being published.
    pub version: String,
    /// Description text.
    #[serde(default)]
    pub description: String,
    /// Category id (default `other`).
    #[serde(default = "default_category")]
    pub category: String,
    /// Publisher name as in the manifest.
    #[serde(default)]
    pub publisher: String,
    /// True for a premium app.
    #[serde(default)]
    pub premium: bool,
    /// Permissions from the manifest.
    #[serde(default)]
    pub permissions: Vec<String>,
    /// File types the app opens.
    #[serde(default)]
    pub file_types: Vec<String>,
    /// Size after installation in bytes.
    #[serde(default)]
    pub installed_size: u64,
    /// What changed in this version.
    #[serde(default)]
    pub changelog: String,
    /// Detached signature field kept for older devices (may be empty).
    #[serde(default)]
    pub sig: String,
}

fn default_category() -> String {
    "other".to_string()
}

fn check(ok: bool, field: &'static str) -> Result<(), AdminError> {
    if ok {
        Ok(())
    } else {
        Err(AdminError::InvalidField(field))
    }
}

fn validate_app(p: &AppPublication) -> Result<Vec<String>, AdminError> {
    check(valid_ident(&p.id), "id")?;
    check(valid_ident(&p.version) && p.version.len() <= 64, "version")?;
    check(
        valid_ident(&p.category) && p.category.len() <= 32,
        "category",
    )?;
    check(
        !p.name.trim().is_empty() && p.name.chars().count() <= 100,
        "name",
    )?;
    check(p.description.chars().count() <= 4000, "description")?;
    check(p.publisher.chars().count() <= 100, "publisher")?;
    check(p.changelog.chars().count() <= 4000, "changelog")?;
    check(p.sig.len() <= 200, "sig")?;
    check(
        p.permissions.len() <= 32 && p.permissions.iter().all(|x| valid_ident(x)),
        "permissions",
    )?;
    check(i64::try_from(p.installed_size).is_ok(), "installed_size")?;
    check(p.file_types.len() <= 64, "file_types")?;
    let mut types = Vec::with_capacity(p.file_types.len());
    for t in &p.file_types {
        types.push(search::file_type(t).ok_or(AdminError::InvalidField("file_types"))?);
    }
    types.sort();
    types.dedup();
    Ok(types)
}

/// Publishes a new version of an app: copies the package into the file store and records it as
/// the current version. Published versions are immutable.
///
/// The package is not opened or verified here; the device verifies its signature.
pub fn publish_app(
    db: &Db,
    files: &FileStore,
    p: &AppPublication,
    package: &Path,
) -> Result<StoredFile, AdminError> {
    let types = validate_app(p)?;
    let exists: i64 = db.lock().query_row(
        "SELECT COUNT(*) FROM store_versions WHERE app_id = ?1 AND version = ?2",
        [&p.id, &p.version],
        |r| r.get(0),
    )?;
    if exists > 0 {
        return Err(AdminError::AlreadyPublished);
    }
    let stored = files.import(package)?;
    let mut conn = db.lock();
    let tx = conn.transaction()?;
    write_app(&tx, p, &stored, &types)?;
    db::audit(
        &tx,
        WHO,
        "app.publish",
        &format!("{} {} {}", p.id, p.version, stored.sha256),
    )?;
    tx.commit()?;
    Ok(stored)
}

const UPSERT_APP: &str = "INSERT INTO store_apps(id, name, version, size, sha256, sig, url, \
         description, category, publisher, premium, permissions, installed_size, search_text, \
         stored) \
     VALUES(?1,?2,?3,?4,?5,?6,'',?7,?8,?9,?10,?11,?12,?13,1) \
     ON CONFLICT(id) DO UPDATE SET name=excluded.name, version=excluded.version, \
         size=excluded.size, sha256=excluded.sha256, sig=excluded.sig, url='', \
         description=excluded.description, category=excluded.category, \
         publisher=excluded.publisher, premium=excluded.premium, \
         permissions=excluded.permissions, installed_size=excluded.installed_size, \
         search_text=excluded.search_text, stored=1";

fn write_app(
    tx: &Connection,
    p: &AppPublication,
    stored: &StoredFile,
    types: &[String],
) -> Result<(), AdminError> {
    let size = i64::try_from(stored.size).map_err(|_| AdminError::InvalidField("file"))?;
    let installed =
        i64::try_from(p.installed_size).map_err(|_| AdminError::InvalidField("installed_size"))?;
    let permissions = serde_json::to_string(&p.permissions)
        .map_err(|_| AdminError::InvalidField("permissions"))?;
    let search_text = search::search_text(&p.name, &p.description);
    let premium = i64::from(p.premium);
    let app: [&dyn rusqlite::ToSql; 13] = [
        &p.id,
        &p.name,
        &p.version,
        &size,
        &stored.sha256,
        &p.sig,
        &p.description,
        &p.category,
        &p.publisher,
        &premium,
        &permissions,
        &installed,
        &search_text,
    ];
    tx.execute(UPSERT_APP, app)?;
    tx.execute(
        "INSERT INTO store_versions(app_id, version, size, sha256, sig, stored, changelog, created) \
         VALUES(?1,?2,?3,?4,?5,1,?6,?7)",
        rusqlite::params![p.id, p.version, size, stored.sha256, p.sig, p.changelog, db::now()],
    )?;
    tx.execute("DELETE FROM store_filetypes WHERE app_id = ?1", [&p.id])?;
    for ext in types {
        tx.execute(
            "INSERT INTO store_filetypes(app_id, ext) VALUES(?1, ?2)",
            [&p.id, ext],
        )?;
    }
    Ok(())
}

/// Description of a system release to publish (read from a JSON file by the command line).
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ReleasePublication {
    /// Channel name; the channel must exist.
    pub channel: String,
    /// Version being published.
    pub version: String,
    /// Target architecture (default `any`).
    #[serde(default = "default_any")]
    pub arch: String,
    /// Target edition (default `any`).
    #[serde(default = "default_any")]
    pub edition: String,
    /// dm-verity root hash of the image.
    pub roothash: String,
    /// Publisher signature (base64) over the root hash, verified on the device.
    pub sig: String,
}

fn default_any() -> String {
    crate::updates::ANY.to_string()
}

fn short_ident(text: &str) -> bool {
    valid_ident(text) && text.len() <= 32
}

/// Publishes a system image into a channel. Returns the release id and the stored file.
pub fn publish_release(
    db: &Db,
    files: &FileStore,
    p: &ReleasePublication,
    image: &Path,
) -> Result<(i64, StoredFile), AdminError> {
    check(short_ident(&p.channel), "channel")?;
    check(valid_ident(&p.version) && p.version.len() <= 64, "version")?;
    check(short_ident(&p.arch), "arch")?;
    check(short_ident(&p.edition), "edition")?;
    let hex_hash = !p.roothash.is_empty() && p.roothash.bytes().all(|b| b.is_ascii_hexdigit());
    check(hex_hash && p.roothash.len() <= 128, "roothash")?;
    check(!p.sig.is_empty() && p.sig.len() <= 200, "sig")?;
    {
        let conn = db.lock();
        if !item_exists(&conn, EntitlementKind::Channel, &p.channel)? {
            return Err(AdminError::UnknownItem);
        }
        let exists: i64 = conn.query_row(
            "SELECT COUNT(*) FROM releases WHERE channel=?1 AND version=?2 AND arch=?3 AND edition=?4",
            [&p.channel, &p.version, &p.arch, &p.edition],
            |r| r.get(0),
        )?;
        if exists > 0 {
            return Err(AdminError::AlreadyPublished);
        }
    }
    let stored = files.import(image)?;
    let size = i64::try_from(stored.size).map_err(|_| AdminError::InvalidField("file"))?;
    let conn = db.lock();
    conn.execute(
        "INSERT INTO releases(channel, version, arch, edition, roothash, sig, size, sha256, stored, created) \
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,1,?9)",
        rusqlite::params![
            p.channel, p.version, p.arch, p.edition, p.roothash, p.sig, size, stored.sha256, db::now()
        ],
    )?;
    let id = conn.last_insert_rowid();
    db::audit(
        &conn,
        WHO,
        "release.publish",
        &format!("{id} {} {}", p.channel, p.version),
    )?;
    Ok((id, stored))
}

#[cfg(test)]
#[path = "admin_tests.rs"]
mod tests;
