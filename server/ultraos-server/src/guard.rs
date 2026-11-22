//! Request guard in front of every route: URI size limit, per-client rate limiting, access log.
//!
//! The client address is used only as the key of an in-memory rate-limit bucket (see
//! [`crate::ratelimit`]); it is never logged and never written to disk. The access log holds
//! method, path without query string, limit class and status.

use crate::error::ApiError;
use crate::ratelimit::{Decision, LimitClass};
use crate::AppState;
use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

/// Longest accepted request target (path and query) in bytes.
pub const MAX_URI_BYTES: usize = 2048;
/// Largest accepted request body in bytes (all bodies are small JSON documents).
pub const MAX_BODY_BYTES: usize = 16 * 1024;

/// A reverse proxy address or network (`10.0.0.5`, `172.18.0.0/16`, `::1`, `fd00::/8`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProxyNet {
    addr: IpAddr,
    prefix: u8,
}

fn unmapped(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        v4 => v4,
    }
}

impl ProxyNet {
    /// A single address.
    pub fn host(ip: IpAddr) -> Self {
        let ip = unmapped(ip);
        Self {
            addr: ip,
            prefix: if ip.is_ipv4() { 32 } else { 128 },
        }
    }

    /// Parses an address or `address/prefix`. `None` for anything malformed.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let Some((addr, prefix)) = text.split_once('/') else {
            return text.parse::<IpAddr>().ok().map(Self::host);
        };
        let addr = unmapped(addr.parse::<IpAddr>().ok()?);
        let prefix = prefix.parse::<u8>().ok()?;
        let max = if addr.is_ipv4() { 32 } else { 128 };
        (prefix <= max).then_some(Self { addr, prefix })
    }

    /// True when `ip` lies in this network.
    pub fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, unmapped(ip)) {
            (IpAddr::V4(net), IpAddr::V4(ip)) => {
                let shift = 32 - u32::from(self.prefix);
                let mask = u32::MAX.checked_shl(shift).unwrap_or(0);
                (u32::from(net) & mask) == (u32::from(ip) & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(ip)) => {
                let shift = 128 - u32::from(self.prefix);
                let mask = u128::MAX.checked_shl(shift).unwrap_or(0);
                (u128::from(net) & mask) == (u128::from(ip) & mask)
            }
            _ => false,
        }
    }
}

/// The limit class a path is charged to; `None` for paths that are never limited.
pub fn class_for(path: &str) -> Option<LimitClass> {
    if path == "/v1/health" {
        None
    } else if path == "/v1/account/register" || path == "/v1/account/login" {
        Some(LimitClass::Auth)
    } else if path.starts_with("/v1/update/file/") || path.ends_with("/download") {
        Some(LimitClass::Download)
    } else {
        Some(LimitClass::General)
    }
}

/// Determines the client address.
///
/// When the TCP peer is one of the configured trusted proxies, the right-most address of
/// `X-Forwarded-For` is used (the entry appended by that proxy; entries further left are
/// client-controlled and ignored). Otherwise the peer address itself counts, so a client cannot
/// pick its own rate-limit bucket by sending the header.
pub fn client_ip(
    peer: Option<IpAddr>,
    headers: &HeaderMap,
    trusted_proxies: &[ProxyNet],
) -> IpAddr {
    let Some(peer) = peer else {
        return IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    };
    if !trusted_proxies.iter().any(|net| net.contains(peer)) {
        return peer;
    }
    headers
        .get_all("x-forwarded-for")
        .iter()
        .next_back()
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.rsplit(',').next())
        .and_then(|last| last.trim().parse::<IpAddr>().ok())
        .unwrap_or(peer)
}

/// The middleware. Applied to the whole router.
pub async fn guard(
    State(st): State<Arc<AppState>>,
    peer: Option<ConnectInfo<SocketAddr>>,
    request: Request,
    next: Next,
) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_string();
    let uri_len = request
        .uri()
        .path_and_query()
        .map_or(path.len(), |pq| pq.as_str().len());
    if uri_len > MAX_URI_BYTES {
        return ApiError::PayloadTooLarge.into_response();
    }
    let class = class_for(&path);
    if let Some(class) = class {
        let ip = client_ip(
            peer.map(|c| c.0.ip()),
            request.headers(),
            &st.trusted_proxies,
        );
        if let Decision::Limited { retry_after_secs } = st.limiter.check(ip, class) {
            tracing::warn!(event = "rate_limited", class = ?class);
            return ApiError::RateLimited { retry_after_secs }.into_response();
        }
    }
    let response = next.run(request).await;
    // Paths hold app ids and release ids only; the query string (search terms) is left out.
    let logged_path: String = path.chars().take(160).collect();
    tracing::info!(
        event = "request",
        method = %method,
        path = %logged_path,
        class = ?class,
        status = response.status().as_u16()
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    fn net(text: &str) -> ProxyNet {
        ProxyNet::parse(text).unwrap()
    }

    #[test]
    fn proxy_networks() {
        assert!(net("127.0.0.1").contains(ip("127.0.0.1")));
        assert!(!net("127.0.0.1").contains(ip("127.0.0.2")));
        assert!(net("172.18.0.0/16").contains(ip("172.18.200.7")));
        assert!(!net("172.18.0.0/16").contains(ip("172.19.0.1")));
        assert!(net("0.0.0.0/0").contains(ip("8.8.8.8")));
        assert!(!net("0.0.0.0/0").contains(ip("2001:db8::1")));
        assert!(net("fd00::/8").contains(ip("fd12::1")));
        assert!(!net("fd00::/8").contains(ip("fe80::1")));
        assert!(net("::1").contains(ip("::1")));
        assert!(
            net("10.0.0.0/8").contains(ip("::ffff:10.1.2.3")),
            "IPv4-mapped peers"
        );
        assert_eq!(ProxyNet::host(ip("::ffff:10.1.2.3")), net("10.1.2.3"));
        for bad in [
            "",
            "10.0.0.0/33",
            "::/129",
            "10.0.0.0/x",
            "localhost",
            "10.0.0/8",
        ] {
            assert_eq!(ProxyNet::parse(bad), None, "{bad}");
        }
    }

    fn xff(values: &[&str]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for v in values {
            h.append("x-forwarded-for", HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn classes() {
        assert_eq!(class_for("/v1/health"), None);
        assert_eq!(class_for("/v1/account/login"), Some(LimitClass::Auth));
        assert_eq!(class_for("/v1/account/register"), Some(LimitClass::Auth));
        assert_eq!(
            class_for("/v1/account/entitlements"),
            Some(LimitClass::General)
        );
        assert_eq!(
            class_for("/v1/store/apps/x/download"),
            Some(LimitClass::Download)
        );
        assert_eq!(class_for("/v1/update/file/3"), Some(LimitClass::Download));
        assert_eq!(class_for("/v1/store/catalog"), Some(LimitClass::General));
        assert_eq!(class_for("/anything/else"), Some(LimitClass::General));
    }

    #[test]
    fn forwarded_header_is_ignored_from_untrusted_peers() {
        let trusted = [net("127.0.0.1")];
        let h = xff(&["9.9.9.9"]);
        assert_eq!(
            client_ip(Some(ip("203.0.113.5")), &h, &trusted),
            ip("203.0.113.5")
        );
        assert_eq!(client_ip(Some(ip("127.0.0.1")), &h, &[]), ip("127.0.0.1"));
    }

    #[test]
    fn trusted_proxy_supplies_the_right_most_address() {
        let trusted = [net("127.0.0.1")];
        let peer = Some(ip("127.0.0.1"));
        assert_eq!(client_ip(peer, &xff(&["9.9.9.9"]), &trusted), ip("9.9.9.9"));
        assert_eq!(
            client_ip(peer, &xff(&["1.1.1.1, 9.9.9.9"]), &trusted),
            ip("9.9.9.9")
        );
        assert_eq!(
            client_ip(peer, &xff(&["1.1.1.1", "2.2.2.2, 2001:db8::1"]), &trusted),
            ip("2001:db8::1")
        );
        assert_eq!(
            client_ip(peer, &xff(&["garbage"]), &trusted),
            ip("127.0.0.1")
        );
        assert_eq!(
            client_ip(peer, &HeaderMap::new(), &trusted),
            ip("127.0.0.1")
        );
    }

    #[test]
    fn unknown_peer_shares_one_bucket() {
        assert_eq!(
            client_ip(None, &xff(&["9.9.9.9"]), &[net("127.0.0.1")]),
            ip("0.0.0.0")
        );
    }
}
