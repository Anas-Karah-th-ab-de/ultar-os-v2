//! The network source: the update endpoints of `ultraos-server` (API v1) over HTTP(S).
//!
//! Only two requests exist: `GET /v1/update/latest` and a ranged `GET` of the release file.
//! Nothing the server says is trusted – it only tells where bytes are. The session token (for
//! gated channels) is sent to the configured server only, never to a redirect target elsewhere.
//!
//! **TLS is pure Rust:** rustls with the Graviola crypto provider (`rustls-graviola`; written by
//! the rustls author, built from formally verified assembly, compiled by `rustc` alone – no C
//! compiler, so the updater builds statically for every target). Certificates are always
//! validated, against the Mozilla roots (`webpki-roots`) or the roots a caller gives. Graviola
//! needs certain CPU features ([`tls_available`]); on a CPU without them an `https` source
//! refuses to start ([`UpdateError::TlsUnavailable`]) instead of falling back to anything –
//! offline bundles and plain `http` servers in a local network still work.

use crate::error::{NetworkError, UpdateError};
use crate::source::{Query, Remote, UpdateSource};
use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{Certificate, RootCerts, TlsConfig, TlsProvider};

/// Largest `update/latest` answer read.
const MAX_ANSWER: u64 = 64 * 1024;
/// Longest location accepted from the server.
const MAX_LOCATION: usize = 2048;

/// True when this CPU can run the TLS crypto provider (Graviola). The list mirrors Graviola's
/// own start-up check exactly, which would otherwise stop the process.
///
/// * `x86_64`: `aes`, `pclmulqdq`, `bmi1`, `adx`, `avx`, `avx2` (about every x86_64 CPU since
///   2014);
/// * `aarch64`: `neon`, `aes`, `pmull`, `sha2` (the ARMv8 crypto extensions; not on a
///   Raspberry Pi 4 or earlier);
/// * other architectures: never.
pub fn tls_available() -> bool {
    #[cfg(target_arch = "x86_64")]
    {
        std::arch::is_x86_feature_detected!("aes")
            && std::arch::is_x86_feature_detected!("pclmulqdq")
            && std::arch::is_x86_feature_detected!("bmi1")
            && std::arch::is_x86_feature_detected!("adx")
            && std::arch::is_x86_feature_detected!("avx")
            && std::arch::is_x86_feature_detected!("avx2")
    }
    #[cfg(target_arch = "aarch64")]
    {
        std::arch::is_aarch64_feature_detected!("neon")
            && std::arch::is_aarch64_feature_detected!("aes")
            && std::arch::is_aarch64_feature_detected!("pmull")
            && std::arch::is_aarch64_feature_detected!("sha2")
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
    {
        false
    }
}

/// Which certificate authorities an `https` source trusts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Roots {
    /// The Mozilla root programme, compiled in (`webpki-roots`).
    #[default]
    Mozilla,
    /// Exactly these root certificates (DER) – for an own update server with an own CA.
    Certificates(Vec<Vec<u8>>),
}

/// Time limits of every request. None of them can be switched off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// Name lookup and TCP connection, each.
    pub connect: Duration,
    /// Sending the request; waiting for the response head.
    pub response: Duration,
    /// Each single read of the body (a stalled stream); see also [`MinRate`].
    pub per_read: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(15),
            response: Duration::from_secs(60),
            per_read: Duration::from_secs(60),
        }
    }
}

/// The update server as a bundle source.
pub struct HttpSource {
    base: String,
    token: Option<String>,
    agent: ureq::Agent,
}

fn status_error(status: u16) -> UpdateError {
    match status {
        401 => UpdateError::AuthRequired,
        403 => UpdateError::NotEntitled,
        404 => UpdateError::UnknownChannel,
        429 => UpdateError::RateLimited,
        other => UpdateError::Network(NetworkError::Status(other)),
    }
}

const PROTOCOL: UpdateError = UpdateError::Network(NetworkError::Protocol);
const TRANSPORT: UpdateError = UpdateError::Network(NetworkError::Transport);

fn tls_config(roots: &Roots) -> TlsConfig {
    let root_certs = match roots {
        Roots::Mozilla => RootCerts::WebPki,
        Roots::Certificates(ders) => {
            let certs = ders.iter().map(|der| Certificate::from_der(der).to_owned());
            RootCerts::Specific(Arc::new(certs.collect()))
        }
    };
    TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .unversioned_rustls_crypto_provider(Arc::new(rustls_graviola::default_provider()))
        .root_certs(root_certs)
        .disable_verification(false)
        .build()
}

fn agent(https: bool, roots: &Roots, timeouts: &Timeouts) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .tls_config(tls_config(roots))
        // The source follows at most one redirect, by hand (token scope, https check).
        .max_redirects(0)
        // Statuses are mapped to typed errors here, not inside the client.
        .http_status_as_error(false)
        // A second guard below the location check of `resolve`.
        .https_only(https)
        // No proxy from the environment: a root daemon does not take routing from variables.
        .proxy(None)
        .timeout_resolve(Some(timeouts.connect))
        .timeout_connect(Some(timeouts.connect))
        .timeout_send_request(Some(timeouts.response))
        .timeout_recv_response(Some(timeouts.response))
        .timeout_per_read(Some(timeouts.per_read))
        .build();
    ureq::Agent::new_with_config(config)
}

impl HttpSource {
    /// A source for the server at `base_url` (`http://…` or `https://…`; a trailing slash is
    /// ignored), trusting the Mozilla roots. `token`: session token for gated channels,
    /// `None` when nobody is signed in.
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self, UpdateError> {
        Self::with(base_url, token, &Roots::Mozilla, &Timeouts::default())
    }

    /// A source with explicit trust roots and time limits.
    /// [`UpdateError::TlsUnavailable`] for an `https` server on a CPU without the features
    /// the TLS provider needs ([`tls_available`]).
    pub fn with(
        base_url: &str,
        token: Option<String>,
        roots: &Roots,
        timeouts: &Timeouts,
    ) -> Result<Self, UpdateError> {
        let base = base_url.trim_end_matches('/').to_string();
        let https = base.starts_with("https://");
        let scheme_ok = https || base.starts_with("http://");
        if !scheme_ok || base.len() > MAX_LOCATION || base.contains(char::is_whitespace) {
            return Err(UpdateError::InvalidArgument);
        }
        if https && !tls_available() {
            return Err(UpdateError::TlsUnavailable);
        }
        let token = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_graphic()));
        let agent = agent(https, roots, timeouts);
        Ok(Self { base, token, agent })
    }

    /// Absolute URL of a location the server gave, and whether it lies on this server.
    fn resolve(&self, location: &str) -> Result<(String, bool), UpdateError> {
        let bad = location.is_empty()
            || location.len() > MAX_LOCATION
            || location.bytes().any(|b| b.is_ascii_control() || b == b' ');
        if bad {
            return Err(PROTOCOL);
        }
        // A source configured with https never leaves https: not for the file location and
        // not for a redirect (no clear-text transfer, no requests into the local network over
        // plain http on the server's say-so).
        if self.base.starts_with("https://") && location.starts_with("http://") {
            return Err(PROTOCOL);
        }
        if location.starts_with("http://") || location.starts_with("https://") {
            let rest = location.strip_prefix(self.base.as_str());
            let ours =
                rest.is_some_and(|r| r.is_empty() || r.starts_with('/') || r.starts_with('?'));
            Ok((location.to_string(), ours))
        } else if location.starts_with('/') {
            Ok((format!("{}{location}", self.base), true))
        } else {
            Err(PROTOCOL)
        }
    }

    fn get(
        &self,
        location: &str,
        query: &[(&str, &str)],
        range: Option<&str>,
    ) -> Result<ureq::http::Response<ureq::Body>, UpdateError> {
        let (url, ours) = self.resolve(location)?;
        let mut request = self.agent.get(&url);
        for (key, value) in query {
            request = request.query(*key, *value);
        }
        if let Some(range) = range {
            request = request.header("range", range);
        }
        if let (true, Some(token)) = (ours, &self.token) {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        let response = request.call().map_err(|error| match error {
            ureq::Error::StatusCode(status) => status_error(status),
            ureq::Error::BadUri(_) | ureq::Error::Http(_) | ureq::Error::RequireHttpsOnly(_) => {
                PROTOCOL
            }
            _ => TRANSPORT,
        })?;
        match response.status().as_u16() {
            status @ 400..=599 => Err(status_error(status)),
            _ => Ok(response),
        }
    }
}

fn header<'a>(response: &'a ureq::http::Response<ureq::Body>, name: &str) -> Option<&'a str> {
    response.headers().get(name)?.to_str().ok()
}

/// A transfer slower than this on average (bytes per second) is given up …
pub const MIN_RATE: u64 = 256;
/// … once it has run for this long. (The lowest rate limit a policy can set is 1 KiB/s.)
pub const MIN_RATE_GRACE: Duration = Duration::from_secs(60);

/// Ends a stream that only drips. The read time-out alone is per read: a server that sends
/// one byte a minute would hold the updater's single operation slot for ever. The transfer
/// fails with `TimedOut` instead; what was received is kept and resumed by the next attempt.
pub struct MinRate<R, E: FnMut() -> Duration> {
    inner: R,
    elapsed: E,
    bytes: u64,
}

impl<R: Read, E: FnMut() -> Duration> MinRate<R, E> {
    /// Guards `inner`; `elapsed` returns the time since the transfer started.
    pub fn new(inner: R, elapsed: E) -> Self {
        Self {
            inner,
            elapsed,
            bytes: 0,
        }
    }
}

impl<R: Read, E: FnMut() -> Duration> Read for MinRate<R, E> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.bytes += n as u64;
        let elapsed = (self.elapsed)();
        let too_slow = self.bytes < MIN_RATE.saturating_mul(elapsed.as_secs());
        if n > 0 && elapsed > MIN_RATE_GRACE && too_slow {
            return Err(std::io::ErrorKind::TimedOut.into());
        }
        Ok(n)
    }
}

/// The start offset a `206` answer announces (`Content-Range: bytes <start>-<end>/<total>`).
fn range_start(header: Option<&str>) -> Option<u64> {
    let rest = header?.trim().strip_prefix("bytes ")?;
    rest.split_once('-')?.0.parse().ok()
}

impl UpdateSource for HttpSource {
    fn latest(&mut self, query: &Query<'_>) -> Result<Option<Remote>, UpdateError> {
        let pairs = [
            ("channel", query.channel),
            ("arch", query.arch),
            ("edition", query.edition),
        ];
        let response = self.get("/v1/update/latest", &pairs, None)?;
        match response.status().as_u16() {
            204 => return Ok(None),
            200 => {}
            other => return Err(UpdateError::Network(NetworkError::Status(other))),
        }
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_ANSWER + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| TRANSPORT)?;
        if bytes.len() as u64 > MAX_ANSWER {
            return Err(PROTOCOL);
        }
        let answer: serde_json::Value = serde_json::from_slice(&bytes).map_err(|_| PROTOCOL)?;
        let locator = answer.get("url").and_then(|u| u.as_str()).ok_or(PROTOCOL)?;
        self.resolve(locator)?;
        Ok(Some(Remote {
            locator: locator.to_string(),
            size_hint: answer.get("size").and_then(|s| s.as_u64()).unwrap_or(0),
        }))
    }

    fn open(
        &mut self,
        remote: &Remote,
        offset: u64,
        limit: Option<u64>,
    ) -> Result<(u64, Box<dyn Read + Send>), UpdateError> {
        let range = match limit {
            Some(limit) if limit > 0 => Some(format!("bytes={offset}-{}", offset + (limit - 1))),
            _ if offset > 0 => Some(format!("bytes={offset}-")),
            _ => None,
        };
        let mut response = self.get(&remote.locator, &[], range.as_deref())?;
        if response.status().is_redirection() {
            // One hop, by hand: `get` attaches the token only for this server.
            let target = header(&response, "location").ok_or(PROTOCOL)?.to_string();
            response = self.get(&target, &[], range.as_deref())?;
        }
        let start = match response.status().as_u16() {
            200 => 0,
            206 => range_start(header(&response, "content-range")).ok_or(PROTOCOL)?,
            other => return Err(UpdateError::Network(NetworkError::Status(other))),
        };
        if start != 0 && start != offset {
            return Err(PROTOCOL);
        }
        let started = std::time::Instant::now();
        let body = response.into_body().into_reader();
        let reader = MinRate::new(body, move || started.elapsed());
        match limit {
            Some(limit) => Ok((start, Box::new(reader.take(limit)))),
            None => Ok((start, Box::new(reader))),
        }
    }

    fn is_network(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_rules() {
        assert!(HttpSource::new("https://ultar.code-craft.tech/", None).is_ok());
        assert!(HttpSource::new("http://127.0.0.1:8080", None).is_ok());
        for bad in [
            "",
            "ftp://x",
            "ultar.code-craft.tech",
            "https://a b",
            "file:///etc/passwd",
        ] {
            assert!(HttpSource::new(bad, None).is_err(), "{bad}");
        }
    }

    #[test]
    fn locations_and_token_scope() {
        let s = HttpSource::new("https://u.example", Some(" tok \n".into())).unwrap();
        assert_eq!(s.token.as_deref(), Some("tok"));
        assert_eq!(
            s.resolve("/v1/update/file/7").unwrap(),
            ("https://u.example/v1/update/file/7".to_string(), true)
        );
        assert!(s.resolve("https://u.example/x").unwrap().1);
        assert!(!s.resolve("https://u.example.evil.org/x").unwrap().1);
        assert!(!s.resolve("https://cdn.example/x").unwrap().1);
        assert!(
            s.resolve("http://u.example/x").is_err(),
            "https never falls back to http"
        );
        assert!(s.resolve("http://127.0.0.1:8080/x").is_err());
        let plain = HttpSource::new("http://lan.example", None).unwrap();
        assert!(plain.resolve("http://lan.example/x").unwrap().1);
        assert!(
            !plain.resolve("https://cdn.example/x").unwrap().1,
            "upgrade is allowed"
        );
        for bad in [
            "",
            "relative/path",
            "/a b",
            "/a\r\nx-injected: 1",
            "file:///etc/passwd",
        ] {
            assert!(s.resolve(bad).is_err(), "{bad:?}");
        }
        assert!(s
            .resolve(&format!("/{}", "a".repeat(MAX_LOCATION)))
            .is_err());
        // A token with characters that could break out of the header is dropped.
        let s = HttpSource::new("https://u.example", Some("a\r\nb".into())).unwrap();
        assert!(s.token.is_none());
    }

    #[test]
    fn dripping_stream_is_given_up_after_the_grace_period() {
        use std::cell::Cell;
        let now = Cell::new(Duration::ZERO);
        let data = vec![7u8; 100_000];
        let mut guarded = MinRate::new(data.as_slice(), || now.get());
        let mut byte = [0u8; 1];
        // One byte per second: tolerated during the grace period ...
        for second in 1..=60 {
            now.set(Duration::from_secs(second));
            assert_eq!(guarded.read(&mut byte).unwrap(), 1, "second {second}");
        }
        // ... then it is over.
        now.set(Duration::from_secs(61));
        let error = guarded.read(&mut byte).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        // A stream at a healthy rate is never touched, however long it runs.
        let mut healthy = MinRate::new(data.as_slice(), || now.get());
        let mut chunk = [0u8; 1000];
        for second in 1..=100 {
            now.set(Duration::from_secs(second));
            assert_eq!(healthy.read(&mut chunk).unwrap(), 1000);
        }
        // The end of a stream is not an error.
        now.set(Duration::from_secs(10_000));
        let mut empty = MinRate::new(&b""[..], || now.get());
        assert_eq!(empty.read(&mut chunk).unwrap(), 0);
    }

    #[test]
    fn content_range_parsing_and_status_mapping() {
        assert_eq!(range_start(Some("bytes 100-199/1000")), Some(100));
        assert_eq!(range_start(Some("bytes */1000")), None);
        assert_eq!(range_start(Some("items 1-2/3")), None);
        assert_eq!(range_start(None), None);
        assert!(matches!(status_error(401), UpdateError::AuthRequired));
        assert!(matches!(status_error(403), UpdateError::NotEntitled));
        assert!(matches!(status_error(404), UpdateError::UnknownChannel));
        assert!(matches!(status_error(429), UpdateError::RateLimited));
        assert!(matches!(
            status_error(500),
            UpdateError::Network(NetworkError::Status(500))
        ));
    }
}
