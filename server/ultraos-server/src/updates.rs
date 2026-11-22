//! System updates: channels, the newest release for a device, and the image download.
//!
//! Open channels (default: `stable`, `beta`) are readable without an account. Gated channels
//! (default: `preview`, `lts`) need a token and a `channel` entitlement, for the metadata as well
//! as for the file. Unknown channels do not exist (`not_found`); nothing is open by accident.
//!
//! The response carries the dm-verity root hash and its signature; the device verifies both
//! against its built-in keys. The server is not a trust anchor.

use crate::conditional::{self, CACHE_PRIVATE, CACHE_REVALIDATE};
use crate::entitlements::{require_entitlement, EntitlementKind};
use crate::error::ApiError;
use crate::files::FileTarget;
use crate::search::valid_ident;
use crate::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Wildcard value for architecture and edition.
pub const ANY: &str = "any";

/// A release as reported to devices.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ReleaseInfo {
    /// Numeric release id (used in the download path).
    pub id: i64,
    /// Channel name.
    pub channel: String,
    /// Version string.
    pub version: String,
    /// Target architecture, or `any`.
    pub arch: String,
    /// Target edition, or `any`.
    pub edition: String,
    /// Image size in bytes (0 = unknown, for entries that predate the file store).
    pub size: i64,
    /// SHA-256 of the image (empty when unknown).
    pub sha256: String,
    /// dm-verity root hash of the image.
    pub roothash: String,
    /// Publisher signature (base64), verified on the device.
    pub sig: String,
    /// Download location (absolute, or relative to the server).
    pub url: String,
}

/// Path of the download endpoint for a release.
pub fn file_path(release_id: i64) -> String {
    format!("/v1/update/file/{release_id}")
}

/// Whether the channel is gated; `None` when it does not exist.
pub fn channel_gated(conn: &Connection, channel: &str) -> rusqlite::Result<Option<bool>> {
    conn.query_row(
        "SELECT gated FROM update_channels WHERE name = ?1",
        [channel],
        |r| Ok(r.get::<_, i64>(0)? != 0),
    )
    .optional()
}

/// The newest release of a channel that fits the device (exact match or `any`).
pub fn latest(
    conn: &Connection,
    channel: &str,
    arch: &str,
    edition: &str,
) -> rusqlite::Result<Option<ReleaseInfo>> {
    conn.query_row(
        "SELECT id, channel, version, arch, edition, size, sha256, roothash, sig, url, stored \
         FROM releases WHERE channel = ?1 AND arch IN (?2, 'any') AND edition IN (?3, 'any') \
         ORDER BY created DESC, id DESC LIMIT 1",
        [channel, arch, edition],
        |r| {
            let id: i64 = r.get(0)?;
            let stored = r.get::<_, i64>(10)? != 0;
            Ok(ReleaseInfo {
                id,
                channel: r.get(1)?,
                version: r.get(2)?,
                arch: r.get(3)?,
                edition: r.get(4)?,
                size: r.get(5)?,
                sha256: r.get(6)?,
                roothash: r.get(7)?,
                sig: r.get(8)?,
                url: if stored { file_path(id) } else { r.get(9)? },
            })
        },
    )
    .optional()
}

/// Looks up the file of a release. Returns `(channel, target)`; `target.gated` is not yet set.
pub fn release_target(
    conn: &Connection,
    id: i64,
) -> rusqlite::Result<Option<(String, FileTarget)>> {
    conn.query_row(
        "SELECT channel, sha256, size, stored, url FROM releases WHERE id = ?1",
        [id],
        |r| {
            let target = FileTarget {
                sha256: r.get(1)?,
                size: u64::try_from(r.get::<_, i64>(2)?).unwrap_or(0),
                stored: r.get::<_, i64>(3)? != 0,
                legacy_url: r.get(4)?,
                gated: false,
            };
            Ok((r.get(0)?, target))
        },
    )
    .optional()
}

/// Enforces the channel rule. Returns whether the channel is gated.
fn authorize_channel(st: &AppState, headers: &HeaderMap, channel: &str) -> Result<bool, ApiError> {
    let gated = channel_gated(&st.db.lock(), channel)?.ok_or(ApiError::NotFound)?;
    if gated {
        require_entitlement(st, headers, EntitlementKind::Channel, channel)?;
    }
    Ok(gated)
}

/// Query parameters of `GET /v1/update/latest`.
#[derive(Debug, Deserialize)]
pub struct LatestParams {
    channel: Option<String>,
    arch: Option<String>,
    edition: Option<String>,
}

fn ident_or(value: Option<String>, fallback: &str) -> Result<String, ApiError> {
    let v = value.unwrap_or_else(|| fallback.to_string());
    if valid_ident(&v) && v.len() <= 32 {
        Ok(v)
    } else {
        Err(ApiError::InvalidRequest)
    }
}

/// `GET /v1/update/latest?channel=&arch=&edition=` — newest release, `204` when there is none.
pub async fn update_latest(
    State(st): State<Arc<AppState>>,
    headers: HeaderMap,
    params: Result<Query<LatestParams>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(p) = params.map_err(|_| ApiError::InvalidRequest)?;
    // Older devices send the channel in a header; the query parameter wins.
    let header_channel = headers
        .get("x-ultar-channel")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let channel = ident_or(p.channel.or(header_channel), "stable")?;
    let arch = ident_or(p.arch, ANY)?;
    let edition = ident_or(p.edition, ANY)?;
    let gated = authorize_channel(&st, &headers, &channel)?;
    let release = latest(&st.db.lock(), &channel, &arch, &edition)?;
    match release {
        Some(info) => {
            let cache = if gated {
                CACHE_PRIVATE
            } else {
                CACHE_REVALIDATE
            };
            conditional::json_response(&headers, &info, cache)
        }
        None => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

/// `GET /v1/update/file/{id}` — the image of a release (supports `Range` and `ETag`).
pub async fn update_file(
    State(st): State<Arc<AppState>>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let id = parse_release_id(&id).ok_or(ApiError::NotFound)?;
    let found = release_target(&st.db.lock(), id)?;
    let (channel, mut target) = found.ok_or(ApiError::NotFound)?;
    target.gated = authorize_channel(&st, &headers, &channel)?;
    tracing::info!(
        event = "download",
        kind = "update",
        release = id,
        gated = target.gated
    );
    st.files.deliver(&headers, &target).await
}

/// Release ids are plain decimal numbers of at most 12 digits; anything else does not exist.
fn parse_release_id(text: &str) -> Option<i64> {
    let ok = (1..=12).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_digit());
    if ok {
        text.parse().ok()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_ids_are_strictly_numeric() {
        assert_eq!(parse_release_id("7"), Some(7));
        assert_eq!(parse_release_id("000000000012"), Some(12));
        for bad in [
            "",
            "-1",
            "+1",
            "1.0",
            "1/..",
            "..",
            "0x10",
            "1234567890123",
            " 1",
        ] {
            assert_eq!(parse_release_id(bad), None, "{bad}");
        }
    }

    #[test]
    fn ident_or_applies_fallback_and_limits() {
        assert_eq!(ident_or(None, "stable").unwrap(), "stable");
        assert_eq!(ident_or(Some("lts".into()), "stable").unwrap(), "lts");
        assert_eq!(
            ident_or(Some("../x".into()), "stable"),
            Err(ApiError::InvalidRequest)
        );
        assert_eq!(
            ident_or(Some("a".repeat(33)), "stable"),
            Err(ApiError::InvalidRequest)
        );
        assert_eq!(
            ident_or(Some(String::new()), "stable"),
            Err(ApiError::InvalidRequest)
        );
    }

    #[test]
    fn latest_prefers_newest_matching_release() {
        let db = crate::db::Db::open(":memory:").unwrap();
        let conn = db.lock();
        conn.execute_batch(
            "INSERT INTO releases(channel,version,arch,edition,roothash,sig,created) VALUES
               ('stable','1.0','any','any','r','s',10),
               ('stable','1.1','aarch64','any','r','s',20),
               ('stable','1.2','x86_64','desktop','r','s',30),
               ('lts','0.9','any','any','r','s',40);",
        )
        .unwrap();
        let v = |arch: &str, edition: &str| {
            latest(&conn, "stable", arch, edition)
                .unwrap()
                .map(|r| r.version)
        };
        assert_eq!(v("aarch64", "phone").as_deref(), Some("1.1"));
        assert_eq!(v("x86_64", "desktop").as_deref(), Some("1.2"));
        assert_eq!(v("x86_64", "server").as_deref(), Some("1.0"));
        assert_eq!(v(ANY, ANY).as_deref(), Some("1.0"));
        assert_eq!(latest(&conn, "nope", ANY, ANY).unwrap(), None);
        assert_eq!(channel_gated(&conn, "stable").unwrap(), Some(false));
        assert_eq!(channel_gated(&conn, "lts").unwrap(), Some(true));
        assert_eq!(channel_gated(&conn, "nope").unwrap(), None);
        assert_eq!(release_target(&conn, 999).unwrap(), None);
        let (channel, target) = release_target(&conn, 4).unwrap().unwrap();
        assert_eq!(channel, "lts");
        assert!(!target.stored);
    }
}
