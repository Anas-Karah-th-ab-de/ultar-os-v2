//! HTTP transport of the cloud client (ureq/rustls): requests, error mapping, verified downloads.
//!
//! Two rules are enforced here for every caller:
//! * the session token is sent only to the configured server, never to another host (old
//!   catalogue entries may point to external locations, and redirects are followed by hand);
//! * a download becomes visible under its final name only after its size and SHA-256 matched
//!   what the server announced (written to `<name>.part`, flushed, then renamed).

use crate::error::CloudError;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Largest JSON answer the client reads.
const JSON_LIMIT: u64 = 4 * 1024 * 1024;
/// Largest download accepted when the server announced no size.
const UNKNOWN_SIZE_CAP: u64 = 8 * 1024 * 1024 * 1024;
const CHUNK: usize = 64 * 1024;

/// What a download must match. Zero / empty mean "not announced".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Expected<'a> {
    /// Size in bytes (0 = unknown).
    pub size: u64,
    /// Lower-case hex SHA-256 (empty = unknown).
    pub sha256: &'a str,
}

/// Connection to one server, with an optional session token.
#[derive(Clone)]
pub struct Transport {
    base_url: String,
    token: Option<String>,
    agent: ureq::Agent,
}

impl Transport {
    /// Creates a transport. A trailing slash of `base_url` is ignored.
    pub fn new(base_url: &str, token: Option<String>) -> Self {
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(Duration::from_secs(15))
            .timeout_read(Duration::from_secs(60))
            .redirects(0)
            .build();
        let token = token.filter(|t| !t.trim().is_empty());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            token,
            agent,
        }
    }

    /// The server address without trailing slash.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// True when a session token is present.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Resolves a location from the server (absolute URL or path) and tells whether it lies on
    /// the configured server.
    pub fn resolve(&self, location: &str) -> (String, bool) {
        if location.starts_with("http://") || location.starts_with("https://") {
            let rest = location.strip_prefix(self.base_url.as_str());
            let ours =
                rest.is_some_and(|r| r.is_empty() || r.starts_with('/') || r.starts_with('?'));
            (location.to_string(), ours)
        } else if location.starts_with('/') {
            (format!("{}{location}", self.base_url), true)
        } else {
            (format!("{}/{location}", self.base_url), true)
        }
    }

    /// Sends a request. The token is attached only when the target is the configured server.
    pub fn send(
        &self,
        method: &str,
        location: &str,
        query: &[(&str, &str)],
        body: Option<&str>,
    ) -> Result<ureq::Response, CloudError> {
        let (url, ours) = self.resolve(location);
        let mut request = self.agent.request(method, &url);
        for (key, value) in query {
            request = request.query(key, value);
        }
        if let (true, Some(token)) = (ours, &self.token) {
            request = request.set("authorization", &format!("Bearer {token}"));
        }
        let result = match body {
            Some(json) => request
                .set("content-type", "application/json")
                .send_string(json),
            None => request.call(),
        };
        result.map_err(convert)
    }

    /// GET that expects a JSON document. `Ok(None)` for `204 No Content`.
    pub fn get_json(
        &self,
        location: &str,
        query: &[(&str, &str)],
    ) -> Result<Option<serde_json::Value>, CloudError> {
        read_json(self.send("GET", location, query, None)?)
    }

    /// POST of a JSON body that expects a JSON document.
    pub fn post_json(
        &self,
        location: &str,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, CloudError> {
        let response = self.send("POST", location, &[], Some(&body.to_string()))?;
        read_json(response)?.ok_or(CloudError::InvalidResponse)
    }

    /// Downloads `location` to `dest`, following at most one redirect, and verifies it.
    pub fn download(
        &self,
        location: &str,
        expected: Expected<'_>,
        dest: &Path,
    ) -> Result<(), CloudError> {
        let mut response = self.send("GET", location, &[], None)?;
        if (300..400).contains(&response.status()) {
            let target = response
                .header("location")
                .ok_or(CloudError::InvalidResponse)?
                .to_string();
            response = self.send("GET", &target, &[], None)?;
        }
        if response.status() != 200 {
            return Err(CloudError::InvalidResponse);
        }
        write_verified(response.into_reader(), expected, dest)
    }
}

fn convert(error: ureq::Error) -> CloudError {
    match error {
        ureq::Error::Status(status, response) => {
            let retry = response
                .header("retry-after")
                .and_then(|v| v.trim().parse::<u64>().ok());
            let mut text = String::new();
            let _ = response.into_reader().take(4096).read_to_string(&mut text);
            let code = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_string));
            CloudError::from_response(status, code.as_deref(), retry)
        }
        ureq::Error::Transport(transport) => CloudError::Transport(transport.to_string()),
    }
}

fn read_json(response: ureq::Response) -> Result<Option<serde_json::Value>, CloudError> {
    match response.status() {
        204 => Ok(None),
        200 => {
            let mut bytes = Vec::new();
            response
                .into_reader()
                .take(JSON_LIMIT + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() as u64 > JSON_LIMIT {
                return Err(CloudError::InvalidResponse);
            }
            serde_json::from_slice(&bytes)
                .map(Some)
                .map_err(|_| CloudError::InvalidResponse)
        }
        _ => Err(CloudError::InvalidResponse),
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_os_string();
    name.push(".part");
    PathBuf::from(name)
}

/// Streams `reader` into `dest` via `<dest>.part`; the file appears under `dest` only when size
/// and digest match `expected`. On any failure the partial file is removed.
pub fn write_verified(
    reader: impl Read,
    expected: Expected<'_>,
    dest: &Path,
) -> Result<(), CloudError> {
    let part = part_path(dest);
    let result = copy_checked(reader, expected, &part).and_then(|()| {
        std::fs::rename(&part, dest)?;
        if let Some(parent) = dest.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::File::open(parent)?.sync_all()?;
        }
        Ok(())
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

fn copy_checked(
    mut reader: impl Read,
    expected: Expected<'_>,
    part: &Path,
) -> Result<(), CloudError> {
    let limit = if expected.size > 0 {
        expected.size
    } else {
        UNKNOWN_SIZE_CAP
    };
    let mut file = std::fs::File::create(part)?;
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            return Err(CloudError::Integrity);
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
    let digest = hex::encode(hasher.finalize());
    let size_ok = expected.size == 0 || total == expected.size;
    let hash_ok = expected.sha256.is_empty() || digest.eq_ignore_ascii_case(expected.sha256);
    if !size_ok || !hash_ok {
        return Err(CloudError::Integrity);
    }
    file.sync_all()?;
    Ok(())
}

/// Writes a small file atomically (temporary file, flush, rename). With `private`, the file is
/// readable by its owner only (tokens, keys).
pub fn write_atomic(path: &Path, content: &[u8], private: bool) -> std::io::Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = part_path(path);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if private {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let written = options.open(&tmp).and_then(|mut file| {
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(content)?;
        file.sync_all()
    });
    match written.and_then(|()| std::fs::rename(&tmp, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

#[cfg(test)]
#[path = "http_tests.rs"]
mod tests;
