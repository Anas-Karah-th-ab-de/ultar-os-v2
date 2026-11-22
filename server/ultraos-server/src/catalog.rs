//! Store catalogue: read-only queries over apps, versions, categories and file types.
//!
//! Everything here is public information (the catalogue is readable without an account),
//! including whether an app is premium. Prices are not modelled.

use crate::files::FileTarget;
use crate::search;
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;

/// Largest page size a client may request.
pub const MAX_PER_PAGE: u32 = 100;
/// Page size when the client does not ask for one.
pub const DEFAULT_PER_PAGE: u32 = 20;
/// Highest page number accepted.
pub const MAX_PAGE: u32 = 100_000;
/// Row cap of the unpaged listing kept for older devices.
pub const LEGACY_LIST_CAP: u32 = 500;

/// Publisher of an app and whether KM Code Craft verified it.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Publisher {
    /// Display name as given in the package manifest.
    pub name: String,
    /// True when the publisher is marked as verified.
    pub verified: bool,
}

/// One catalogue entry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppSummary {
    /// Unique app id (for example `com.kmcc.pos`).
    pub id: String,
    /// Display name.
    pub name: String,
    /// Description text.
    pub description: String,
    /// Category id.
    pub category: String,
    /// Publisher and verification state.
    pub publisher: Publisher,
    /// True when the download needs an account with an entitlement.
    pub premium: bool,
    /// Current version.
    pub version: String,
    /// Download size of the current version in bytes.
    pub size: i64,
}

/// One entry of an app's version history.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct VersionInfo {
    /// Version string.
    pub version: String,
    /// Download size in bytes.
    pub size: i64,
    /// SHA-256 of the package file.
    pub sha256: String,
    /// What changed in this version.
    pub changelog: String,
    /// Publication time (Unix seconds; 0 for entries that predate the history).
    pub created: i64,
}

/// Full description of one app.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppDetails {
    /// The catalogue entry.
    #[serde(flatten)]
    pub summary: AppSummary,
    /// Size after installation in bytes (0 = unknown).
    pub installed_size: i64,
    /// SHA-256 of the current package file.
    pub sha256: String,
    /// Detached signature field of older entries (the binding signature is inside the package).
    pub sig: String,
    /// Permissions the app asks for, as listed in its manifest.
    pub permissions: Vec<String>,
    /// File types the app opens (lower case, without dot).
    pub file_types: Vec<String>,
    /// Path of the download endpoint, relative to the server.
    pub download: String,
    /// Version history, newest first.
    pub versions: Vec<VersionInfo>,
}

/// A page of catalogue entries.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CatalogPage {
    /// Number of entries matching the query.
    pub total: i64,
    /// Page number (starting at 1).
    pub page: u32,
    /// Page size.
    pub per_page: u32,
    /// The entries of this page.
    pub items: Vec<AppSummary>,
}

/// A category and how many apps it holds.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CategoryCount {
    /// Category id.
    pub id: String,
    /// Number of apps.
    pub count: i64,
}

/// Entry of the unpaged listing kept for older devices.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct LegacyApp {
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
    /// Detached signature field.
    pub sig: String,
    /// Download location (absolute, or relative to the server).
    pub url: String,
    /// True when the download needs an entitlement.
    pub premium: bool,
}

/// A validated catalogue query.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CatalogQuery {
    /// Free text; a leading dot means "file type" (".xlsx").
    pub text: String,
    /// Category id, empty for all.
    pub category: String,
    /// File type (normalised), empty for all.
    pub file_type: String,
    /// Page number, starting at 1.
    pub page: u32,
    /// Page size, 1..=[`MAX_PER_PAGE`].
    pub per_page: u32,
}

const SUMMARY_COLUMNS: &str = "a.id, a.name, a.description, a.category, a.publisher, \
     COALESCE(p.verified, 0), a.premium, a.version, a.size";
const FROM_APPS: &str = "FROM store_apps a LEFT JOIN publishers p ON p.name = a.publisher";
const FILTER: &str = "WHERE (?1 = '' OR a.category = ?1) \
     AND (?2 = 0 OR (?3 = 1 AND a.search_text LIKE ?4 ESCAPE '\\') \
          OR (?5 <> '' AND EXISTS (SELECT 1 FROM store_filetypes f \
                                   WHERE f.app_id = a.id AND f.ext = ?5))) \
     AND (?6 = '' OR EXISTS (SELECT 1 FROM store_filetypes f \
                             WHERE f.app_id = a.id AND f.ext = ?6))";

fn summary_from(row: &rusqlite::Row<'_>) -> rusqlite::Result<AppSummary> {
    Ok(AppSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        category: row.get(3)?,
        publisher: Publisher {
            name: row.get(4)?,
            verified: row.get::<_, i64>(5)? != 0,
        },
        premium: row.get::<_, i64>(6)? != 0,
        version: row.get(7)?,
        size: row.get(8)?,
    })
}

/// Path of the download endpoint for an app.
pub fn download_path(app_id: &str) -> String {
    format!("/v1/store/apps/{app_id}/download")
}

/// Lists one page of apps matching the query, ordered by name.
pub fn list(conn: &Connection, q: &CatalogQuery) -> rusqlite::Result<CatalogPage> {
    let text = search::normalize(&q.text);
    let by_type_only = text.starts_with('.');
    let text_ext = search::file_type(&text).unwrap_or_default();
    let pattern = search::like_pattern(&text);
    let has_text = i64::from(!text.is_empty());
    let use_text = i64::from(!by_type_only);
    let filter: [&dyn rusqlite::ToSql; 6] = [
        &q.category,
        &has_text,
        &use_text,
        &pattern,
        &text_ext,
        &q.file_type,
    ];
    let total: i64 = conn.query_row(
        &format!("SELECT COUNT(*) {FROM_APPS} {FILTER}"),
        filter,
        |r| r.get(0),
    )?;
    let limit = i64::from(q.per_page);
    let offset = i64::from(q.page.saturating_sub(1)) * limit;
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS} {FROM_APPS} {FILTER} \
         ORDER BY a.name COLLATE NOCASE, a.id LIMIT ?7 OFFSET ?8"
    );
    let mut params: Vec<&dyn rusqlite::ToSql> = filter.to_vec();
    params.push(&limit);
    params.push(&offset);
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params.as_slice(), summary_from)?;
    let items = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(CatalogPage {
        total,
        page: q.page,
        per_page: q.per_page,
        items,
    })
}

/// Lists all categories that hold at least one app.
pub fn categories(conn: &Connection) -> rusqlite::Result<Vec<CategoryCount>> {
    let mut stmt = conn.prepare_cached(
        "SELECT category, COUNT(*) FROM store_apps GROUP BY category ORDER BY category",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(CategoryCount {
            id: r.get(0)?,
            count: r.get(1)?,
        })
    })?;
    rows.collect()
}

fn string_column(conn: &Connection, sql: &str, app_id: &str) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt.query_map([app_id], |r| r.get(0))?;
    rows.collect()
}

fn versions(conn: &Connection, app_id: &str) -> rusqlite::Result<Vec<VersionInfo>> {
    let mut stmt = conn.prepare_cached(
        "SELECT version, size, sha256, changelog, created FROM store_versions \
         WHERE app_id = ?1 ORDER BY created DESC, rowid DESC LIMIT 200",
    )?;
    let rows = stmt.query_map([app_id], |r| {
        Ok(VersionInfo {
            version: r.get(0)?,
            size: r.get(1)?,
            sha256: r.get(2)?,
            changelog: r.get(3)?,
            created: r.get(4)?,
        })
    })?;
    rows.collect()
}

/// Full details of one app, or `None` when the id is unknown.
pub fn details(conn: &Connection, app_id: &str) -> rusqlite::Result<Option<AppDetails>> {
    let sql = format!(
        "SELECT {SUMMARY_COLUMNS}, a.installed_size, a.sha256, a.sig, a.permissions \
         {FROM_APPS} WHERE a.id = ?1"
    );
    let head = conn
        .query_row(&sql, [app_id], |r| {
            Ok((
                summary_from(r)?,
                r.get(9)?,
                r.get(10)?,
                r.get(11)?,
                r.get::<_, String>(12)?,
            ))
        })
        .optional()?;
    let Some((summary, installed_size, sha256, sig, permissions_json)) = head else {
        return Ok(None);
    };
    let permissions = serde_json::from_str(&permissions_json).unwrap_or_else(|_| {
        tracing::error!(event = "corrupt_permissions", app = %app_id);
        Vec::new()
    });
    let file_types = string_column(
        conn,
        "SELECT ext FROM store_filetypes WHERE app_id = ?1 ORDER BY ext",
        app_id,
    )?;
    Ok(Some(AppDetails {
        summary,
        installed_size,
        sha256,
        sig,
        permissions,
        file_types,
        download: download_path(app_id),
        versions: versions(conn, app_id)?,
    }))
}

/// The unpaged listing of older servers (capped at [`LEGACY_LIST_CAP`] rows).
pub fn legacy_list(conn: &Connection) -> rusqlite::Result<Vec<LegacyApp>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, version, size, sha256, sig, url, stored, premium \
         FROM store_apps ORDER BY id LIMIT ?1",
    )?;
    let rows = stmt.query_map([LEGACY_LIST_CAP], |r| {
        let id: String = r.get(0)?;
        let stored = r.get::<_, i64>(7)? != 0;
        let url = if stored {
            download_path(&id)
        } else {
            r.get(6)?
        };
        Ok(LegacyApp {
            name: r.get(1)?,
            version: r.get(2)?,
            size: r.get(3)?,
            sha256: r.get(4)?,
            sig: r.get(5)?,
            url,
            premium: r.get::<_, i64>(8)? != 0,
            id,
        })
    })?;
    rows.collect()
}

/// Looks up what a download of `app_id` (current version, or the given one) refers to.
/// Returns `(premium, target)`, or `None` when the app or version is unknown.
pub fn download_target(
    conn: &Connection,
    app_id: &str,
    version: Option<&str>,
) -> rusqlite::Result<Option<(bool, FileTarget)>> {
    conn.query_row(
        "SELECT a.premium, v.sha256, v.size, v.stored, v.url \
         FROM store_apps a JOIN store_versions v ON v.app_id = a.id \
         WHERE a.id = ?1 AND v.version = COALESCE(?2, a.version)",
        rusqlite::params![app_id, version],
        |r| {
            let premium = r.get::<_, i64>(0)? != 0;
            let target = FileTarget {
                sha256: r.get(1)?,
                size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                stored: r.get::<_, i64>(3)? != 0,
                legacy_url: r.get(4)?,
                gated: premium,
            };
            Ok((premium, target))
        },
    )
    .optional()
}
