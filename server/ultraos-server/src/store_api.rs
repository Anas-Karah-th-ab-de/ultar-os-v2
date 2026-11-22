//! HTTP handlers of the store: catalogue, categories, app details and package download.
//!
//! All of them work without an account. Only the download of a premium app asks for a token and
//! an entitlement. Clients never name files: a download is addressed by app id (and optionally a
//! version), both validated and then looked up in the database.

use crate::catalog::{self, CatalogQuery, DEFAULT_PER_PAGE, MAX_PAGE, MAX_PER_PAGE};
use crate::conditional::{json_response, CACHE_REVALIDATE};
use crate::entitlements::{require_entitlement, EntitlementKind};
use crate::error::ApiError;
use crate::search::{self, valid_ident, MAX_QUERY_CHARS};
use crate::AppState;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Deserialize;
use std::sync::Arc;

type Shared = Arc<AppState>;

/// Query parameters of `GET /v1/store/catalog`.
#[derive(Debug, Default, Deserialize)]
pub struct ListParams {
    q: Option<String>,
    category: Option<String>,
    filetype: Option<String>,
    page: Option<String>,
    per_page: Option<String>,
}

fn number_in(value: Option<&str>, default: u32, max: u32) -> Result<u32, ApiError> {
    let Some(text) = value else {
        return Ok(default);
    };
    let digits = !text.is_empty() && text.len() <= 6 && text.bytes().all(|b| b.is_ascii_digit());
    match text.parse::<u32>() {
        Ok(n) if digits && (1..=max).contains(&n) => Ok(n),
        _ => Err(ApiError::InvalidRequest),
    }
}

/// Validates the raw parameters into a [`CatalogQuery`].
pub fn parse_list_params(p: &ListParams) -> Result<CatalogQuery, ApiError> {
    let text = p.q.clone().unwrap_or_default();
    if text.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::InvalidRequest);
    }
    let category = p.category.clone().unwrap_or_default();
    if !category.is_empty() && !valid_ident(&category) {
        return Err(ApiError::InvalidRequest);
    }
    let file_type = match p.filetype.as_deref() {
        None | Some("") => String::new(),
        Some(t) => search::file_type(t).ok_or(ApiError::InvalidRequest)?,
    };
    Ok(CatalogQuery {
        text,
        category,
        file_type,
        page: number_in(p.page.as_deref(), 1, MAX_PAGE)?,
        per_page: number_in(p.per_page.as_deref(), DEFAULT_PER_PAGE, MAX_PER_PAGE)?,
    })
}

/// `GET /v1/store/catalog` — paged, searchable list of apps.
pub async fn catalog_list(
    State(st): State<Shared>,
    headers: HeaderMap,
    params: Result<Query<ListParams>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(p) = params.map_err(|_| ApiError::InvalidRequest)?;
    let query = parse_list_params(&p)?;
    let page = catalog::list(&st.db.lock(), &query)?;
    json_response(&headers, &page, CACHE_REVALIDATE)
}

/// `GET /v1/store/categories` — categories with app counts.
pub async fn categories(
    State(st): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let list = catalog::categories(&st.db.lock())?;
    json_response(
        &headers,
        &serde_json::json!({ "categories": list }),
        CACHE_REVALIDATE,
    )
}

/// `GET /v1/store/apps` — unpaged listing in the shape older devices expect (a bare array).
pub async fn store_apps(
    State(st): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let list = catalog::legacy_list(&st.db.lock())?;
    json_response(&headers, &list, CACHE_REVALIDATE)
}

/// `GET /v1/store/apps/{id}` — details of one app.
pub async fn app_details(
    State(st): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if !valid_ident(&id) {
        return Err(ApiError::NotFound);
    }
    let details = catalog::details(&st.db.lock(), &id)?.ok_or(ApiError::NotFound)?;
    json_response(&headers, &details, CACHE_REVALIDATE)
}

/// Query parameters of the package download.
#[derive(Debug, Default, Deserialize)]
pub struct DownloadParams {
    version: Option<String>,
}

/// `GET /v1/store/apps/{id}/download[?version=]` — the package file.
///
/// Free apps: no account. Premium apps: token (`auth_required`) and entitlement
/// (`not_entitled`). Supports `Range`, `If-Range`, `ETag` and `If-None-Match`.
pub async fn app_download(
    State(st): State<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
    params: Result<Query<DownloadParams>, QueryRejection>,
) -> Result<Response, ApiError> {
    let Query(p) = params.map_err(|_| ApiError::InvalidRequest)?;
    let version = p.version.as_deref();
    if !valid_ident(&id) || version.is_some_and(|v| !valid_ident(v)) {
        return Err(ApiError::NotFound);
    }
    let found = catalog::download_target(&st.db.lock(), &id, version)?;
    let (premium, target) = found.ok_or(ApiError::NotFound)?;
    if premium {
        require_entitlement(&st, &headers, EntitlementKind::App, &id)?;
    }
    tracing::info!(event = "download", kind = "app", app = %id, premium);
    st.files.deliver(&headers, &target).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn params(q: &str, category: &str, filetype: &str, page: &str, per_page: &str) -> ListParams {
        let opt = |s: &str| (!s.is_empty()).then(|| s.to_string());
        ListParams {
            q: opt(q),
            category: opt(category),
            filetype: opt(filetype),
            page: opt(page),
            per_page: opt(per_page),
        }
    }

    #[test]
    fn defaults() {
        let q = parse_list_params(&ListParams::default()).unwrap();
        assert_eq!((q.page, q.per_page), (1, DEFAULT_PER_PAGE));
        assert!(q.text.is_empty() && q.category.is_empty() && q.file_type.is_empty());
    }

    #[test]
    fn accepts_valid_values() {
        let q = parse_list_params(&params("جدول", "office", ".XLSX", "3", "100")).unwrap();
        assert_eq!(q.text, "جدول");
        assert_eq!(q.category, "office");
        assert_eq!(q.file_type, "xlsx");
        assert_eq!((q.page, q.per_page), (3, 100));
    }

    #[test]
    fn rejects_out_of_range_and_malformed_values() {
        let bad = [
            params("", "", "", "0", ""),
            params("", "", "", "-1", ""),
            params("", "", "", "100001", ""),
            params("", "", "", "1e3", ""),
            params("", "", "", "+5", ""),
            params("", "", "", "", "0"),
            params("", "", "", "", "101"),
            params("", "", "", "", "abc"),
            params("", "../x", "", "", ""),
            params("", "", "a/b", "", ""),
            params(&"x".repeat(MAX_QUERY_CHARS + 1), "", "", "", ""),
        ];
        for p in bad {
            assert_eq!(
                parse_list_params(&p),
                Err(ApiError::InvalidRequest),
                "{p:?}"
            );
        }
        let longest = params(&"ج".repeat(MAX_QUERY_CHARS), "", "", "", "");
        assert!(
            parse_list_params(&longest).is_ok(),
            "limit counts characters, not bytes"
        );
    }
}
