//! The network source: the update endpoints of `ultraos-server` (API v1) over HTTP(S).
//!
//! Only two requests exist: `GET /v1/update/latest` and a ranged `GET` of the release file.
//! Nothing the server says is trusted – it only tells where bytes are. The session token (for
//! gated channels) is sent to the configured server only, never to a redirect target elsewhere.

use crate::error::{NetworkError, UpdateError};
use crate::source::{Query, Remote, UpdateSource};
use std::io::Read;
use std::time::Duration;

/// Largest `update/latest` answer read.
const MAX_ANSWER: u64 = 64 * 1024;
/// Longest location accepted from the server.
const MAX_LOCATION: usize = 2048;

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

impl HttpSource {
    /// A source for the server at `base_url` (`http://…` or `https://…`; a trailing slash is
    /// ignored). `token`: session token for gated channels, `None` when nobody is signed in.
    pub fn new(base_url: &str, token: Option<String>) -> Result<Self, UpdateError> {
        let base = base_url.trim_end_matches('/').to_string();
        let scheme_ok = base.starts_with("https://") || base.starts_with("http://");
        if !scheme_ok || base.len() > MAX_LOCATION || base.contains(char::is_whitespace) {
            return Err(UpdateError::InvalidArgument);
        }
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .redirects(0)
            .build();
        let token = token
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && t.bytes().all(|b| b.is_ascii_graphic()));
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
    ) -> Result<ureq::Response, UpdateError> {
        let (url, ours) = self.resolve(location)?;
        let mut request = self.agent.get(&url);
        for (key, value) in query {
            request = request.query(key, value);
        }
        if let Some(range) = range {
            request = request.set("range", range);
        }
        if let (true, Some(token)) = (ours, &self.token) {
            request = request.set("authorization", &format!("Bearer {token}"));
        }
        match request.call() {
            Ok(response) => Ok(response),
            Err(ureq::Error::Status(status, _)) => Err(status_error(status)),
            Err(ureq::Error::Transport(_)) => Err(UpdateError::Network(NetworkError::Transport)),
        }
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
        match response.status() {
            204 => return Ok(None),
            200 => {}
            other => return Err(UpdateError::Network(NetworkError::Status(other))),
        }
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(MAX_ANSWER + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| UpdateError::Network(NetworkError::Transport))?;
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
        if (300..400).contains(&response.status()) {
            // One hop, by hand: `get` attaches the token only for this server.
            let target = response.header("location").ok_or(PROTOCOL)?.to_string();
            response = self.get(&target, &[], range.as_deref())?;
        }
        let start = match response.status() {
            200 => 0,
            206 => range_start(response.header("content-range")).ok_or(PROTOCOL)?,
            other => return Err(UpdateError::Network(NetworkError::Status(other))),
        };
        if start != 0 && start != offset {
            return Err(PROTOCOL);
        }
        let reader = response.into_reader();
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
            !s.resolve("http://u.example/x").unwrap().1,
            "other scheme is another server"
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
