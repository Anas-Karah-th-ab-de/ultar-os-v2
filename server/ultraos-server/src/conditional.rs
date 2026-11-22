//! HTTP validators and byte ranges: `ETag` / `If-None-Match`, `Range` / `If-Range`, and JSON
//! responses that carry an entity tag.

use crate::error::ApiError;
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use sha2::{Digest, Sha256};

/// Cache policy for public, revalidated content.
pub const CACHE_REVALIDATE: &str = "no-cache";
/// Cache policy for content that depends on the caller's entitlement.
pub const CACHE_PRIVATE: &str = "private, no-store";

/// Which bytes of a file a request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteRange {
    /// The whole file (no usable `Range` header).
    Full,
    /// The inclusive byte range `start..=end`.
    Partial {
        /// First byte.
        start: u64,
        /// Last byte (inclusive).
        end: u64,
    },
    /// A syntactically valid range that lies outside the file.
    Unsatisfiable,
}

fn strip_weak(tag: &str) -> &str {
    tag.trim().strip_prefix("W/").unwrap_or(tag.trim())
}

/// True when `If-None-Match` names `etag` (weak comparison) or is `*`.
pub fn none_match_hits(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get_all(header::IF_NONE_MATCH)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .any(|value| {
            value
                .split(',')
                .map(strip_weak)
                .any(|candidate| candidate == "*" || candidate == etag)
        })
}

/// Parses a `Range` header value for a file of `len` bytes.
///
/// Only a single `bytes` range is honoured; anything else (other units, several ranges, syntax
/// errors) is ignored as the RFC permits, which yields the full file.
pub fn parse_range(value: &str, len: u64) -> ByteRange {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return ByteRange::Full;
    };
    let Some((first, last)) = spec.trim().split_once('-') else {
        return ByteRange::Full;
    };
    let (first, last) = (first.trim(), last.trim());
    if spec.contains(',') || !is_digits_or_empty(first) || !is_digits_or_empty(last) {
        return ByteRange::Full;
    }
    match (first.parse::<u64>().ok(), last.parse::<u64>().ok()) {
        (Some(start), end) if !first.is_empty() => {
            let end = end.map_or(len.saturating_sub(1), |e| e.min(len.saturating_sub(1)));
            if !last.is_empty() && last.parse::<u64>().is_err() {
                return ByteRange::Full;
            }
            if start >= len || start > end {
                return if start >= len {
                    ByteRange::Unsatisfiable
                } else {
                    ByteRange::Full
                };
            }
            ByteRange::Partial { start, end }
        }
        (None, Some(suffix)) if first.is_empty() => {
            if suffix == 0 || len == 0 {
                return ByteRange::Unsatisfiable;
            }
            ByteRange::Partial {
                start: len.saturating_sub(suffix),
                end: len - 1,
            }
        }
        _ => ByteRange::Full,
    }
}

fn is_digits_or_empty(text: &str) -> bool {
    text.len() <= 19 && text.bytes().all(|b| b.is_ascii_digit())
}

/// Resolves the range a request wants, honouring `If-Range` (a mismatch means "send everything").
pub fn requested_range(headers: &HeaderMap, etag: &str, len: u64) -> ByteRange {
    let Some(range) = headers.get(header::RANGE).and_then(|v| v.to_str().ok()) else {
        return ByteRange::Full;
    };
    if let Some(cond) = headers.get(header::IF_RANGE) {
        if cond.to_str().ok().map(str::trim) != Some(etag) {
            return ByteRange::Full;
        }
    }
    parse_range(range, len)
}

/// Strong entity tag for a byte string: quoted first 128 bits of its SHA-256.
pub fn etag_for(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("\"{}\"", hex::encode(&digest[..16]))
}

fn header_value(text: &str) -> Result<HeaderValue, ApiError> {
    HeaderValue::from_str(text).map_err(|_| ApiError::Internal)
}

/// Serialises `value` as JSON with an `ETag`; answers `304 Not Modified` when the client's
/// `If-None-Match` already names it.
pub fn json_response<T: Serialize>(
    headers: &HeaderMap,
    value: &T,
    cache: &'static str,
) -> Result<Response, ApiError> {
    let body = serde_json::to_vec(value).map_err(|_| ApiError::Internal)?;
    let etag = etag_for(&body);
    let tag = header_value(&etag)?;
    let common = [
        (header::ETAG, tag),
        (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        (header::VARY, HeaderValue::from_static("authorization")),
    ];
    if none_match_hits(headers, &etag) {
        return Ok((StatusCode::NOT_MODIFIED, common).into_response());
    }
    let json = [(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    )];
    Ok((StatusCode::OK, common, json, body).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (k, v) in pairs {
            h.append(k.clone(), HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn range_forms() {
        assert_eq!(
            parse_range("bytes=0-9", 100),
            ByteRange::Partial { start: 0, end: 9 }
        );
        assert_eq!(
            parse_range("bytes=90-", 100),
            ByteRange::Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=-10", 100),
            ByteRange::Partial { start: 90, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=-500", 100),
            ByteRange::Partial { start: 0, end: 99 }
        );
        assert_eq!(
            parse_range("bytes=50-5000", 100),
            ByteRange::Partial { start: 50, end: 99 }
        );
        assert_eq!(
            parse_range(" bytes=5-5 ", 100),
            ByteRange::Partial { start: 5, end: 5 }
        );
    }

    #[test]
    fn range_outside_file_is_unsatisfiable() {
        assert_eq!(parse_range("bytes=100-", 100), ByteRange::Unsatisfiable);
        assert_eq!(parse_range("bytes=100-200", 100), ByteRange::Unsatisfiable);
        assert_eq!(parse_range("bytes=-0", 100), ByteRange::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-", 0), ByteRange::Unsatisfiable);
        assert_eq!(parse_range("bytes=-5", 0), ByteRange::Unsatisfiable);
    }

    #[test]
    fn malformed_or_unsupported_ranges_are_ignored() {
        for v in [
            "items=0-5",
            "bytes=5-2",
            "bytes=a-b",
            "bytes=0-5,10-15",
            "bytes=-",
            "bytes=",
            "bytes=+1-5",
            "bytes=1-5x",
            "bytes=99999999999999999999-",
            "bytes=0--5",
        ] {
            assert_eq!(parse_range(v, 100), ByteRange::Full, "{v}");
        }
    }

    #[test]
    fn if_none_match_lists_weak_tags_and_star() {
        let tag = "\"abc\"";
        assert!(none_match_hits(
            &headers(&[(header::IF_NONE_MATCH, "\"abc\"")]),
            tag
        ));
        assert!(none_match_hits(
            &headers(&[(header::IF_NONE_MATCH, "\"x\", W/\"abc\"")]),
            tag
        ));
        assert!(none_match_hits(
            &headers(&[(header::IF_NONE_MATCH, "*")]),
            tag
        ));
        assert!(!none_match_hits(
            &headers(&[(header::IF_NONE_MATCH, "\"abcd\"")]),
            tag
        ));
        assert!(!none_match_hits(&HeaderMap::new(), tag));
    }

    #[test]
    fn if_range_mismatch_sends_everything() {
        let tag = "\"abc\"";
        let h = headers(&[(header::RANGE, "bytes=0-1"), (header::IF_RANGE, "\"abc\"")]);
        assert_eq!(
            requested_range(&h, tag, 10),
            ByteRange::Partial { start: 0, end: 1 }
        );
        let h = headers(&[(header::RANGE, "bytes=0-1"), (header::IF_RANGE, "\"old\"")]);
        assert_eq!(requested_range(&h, tag, 10), ByteRange::Full);
        assert_eq!(requested_range(&HeaderMap::new(), tag, 10), ByteRange::Full);
    }

    #[test]
    fn json_response_sets_etag_and_answers_304() {
        let value = serde_json::json!({"a": 1});
        let first = json_response(&HeaderMap::new(), &value, CACHE_REVALIDATE).unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let etag = first
            .headers()
            .get(header::ETAG)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string();
        assert_eq!(etag, etag_for(br#"{"a":1}"#));
        let h = headers(&[(header::IF_NONE_MATCH, &etag)]);
        let second = json_response(&h, &value, CACHE_REVALIDATE).unwrap();
        assert_eq!(second.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            second
                .headers()
                .get(header::ETAG)
                .unwrap()
                .to_str()
                .unwrap(),
            etag
        );
        let other = json_response(&h, &serde_json::json!({"a": 2}), CACHE_REVALIDATE).unwrap();
        assert_eq!(other.status(), StatusCode::OK);
    }
}
