//! Fetching a page in pure Rust: HTTP and HTTPS with `ureq` (redirects followed, size and
//! time bounded), TLS by rustls with the Graviola provider where the CPU and the kernel offer
//! its instructions, else the RustCrypto provider; Mozilla's roots compiled in. Text is
//! decoded with the charset the server or the page names (UTF-8, Windows-1256, …).

use std::io::Read;
use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Largest page read.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Time for each step (name lookup, connect, answer, every read).
const TIMEOUT: Duration = Duration::from_secs(20);

/// What came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fetched {
    /// The address after redirects.
    pub url: String,
    /// The media type, lower case, without parameters (`text/html`).
    pub mime: String,
    /// The body as text.
    pub text: String,
}

/// Why a page did not come.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    /// Name lookup, connection or TLS failed.
    Unreachable(String),
    /// The server answered with this status.
    Status(u16),
    /// Not text (a picture, a program …).
    NotText(String),
    /// Larger than [`MAX_BYTES`].
    TooLarge,
}

fn graviola_usable() -> bool {
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

/// The HTTP client of the reader.
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
}

impl Client {
    /// A client that names itself `user_agent`.
    pub fn new(user_agent: &str) -> Client {
        let provider = if graviola_usable() {
            rustls_graviola::default_provider()
        } else {
            rustls_rustcrypto::provider()
        };
        let tls = TlsConfig::builder()
            .provider(TlsProvider::Rustls)
            .unversioned_rustls_crypto_provider(Arc::new(provider))
            .root_certs(RootCerts::WebPki)
            .disable_verification(false)
            .build();
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(user_agent)
            .max_redirects(10)
            .http_status_as_error(false)
            .proxy(None)
            .timeout_resolve(Some(TIMEOUT))
            .timeout_connect(Some(TIMEOUT))
            .timeout_send_request(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .timeout_per_read(Some(TIMEOUT))
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
        }
    }

    /// Fetches `url` (http or https).
    pub fn get(&self, url: &str) -> Result<Fetched, FetchError> {
        let response = self
            .agent
            .get(url)
            .header("Accept", "text/html,text/plain;q=0.9,*/*;q=0.1")
            .header("Accept-Language", "ar,en;q=0.8,de;q=0.6")
            .call()
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
        let status = response.status().as_u16();
        // the address after redirects (links of the page are relative to it)
        let final_url = {
            use ureq::ResponseExt;
            response.get_uri().to_string()
        };
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_ascii_lowercase();
        if !(200..300).contains(&status) {
            return Err(FetchError::Status(status));
        }
        let mime = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_owned();
        let texty = mime.is_empty()
            || mime.starts_with("text/")
            || mime.contains("xml")
            || mime.contains("json");
        if !texty {
            return Err(FetchError::NotText(mime));
        }
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| FetchError::Unreachable(e.to_string()))?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(FetchError::TooLarge);
        }
        let charset = charset_of(&content_type).or_else(|| meta_charset(&bytes));
        Ok(Fetched {
            url: final_url,
            mime,
            text: decode(&bytes, charset.as_deref()),
        })
    }
}

fn charset_of(content_type: &str) -> Option<String> {
    content_type
        .split(';')
        .filter_map(|p| p.trim().strip_prefix("charset="))
        .map(|c| c.trim_matches('"').to_owned())
        .next()
}

/// The charset a page names in its first 2 KiB (`<meta charset=…>` or `content="…charset=…"`).
fn meta_charset(bytes: &[u8]) -> Option<String> {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).to_ascii_lowercase();
    let at = head.find("charset=")?;
    let rest = head[at + 8..].trim_start_matches(['"', '\'']);
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or(rest.len());
    (end > 0).then(|| rest[..end].to_owned())
}

/// `bytes` as text in `charset` (UTF-8 when unknown).
pub fn decode(bytes: &[u8], charset: Option<&str>) -> String {
    let encoding = charset
        .and_then(|c| encoding_rs::Encoding::for_label(c.as_bytes()))
        .unwrap_or(encoding_rs::UTF_8);
    encoding.decode(bytes).0.into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn charsets() {
        assert_eq!(
            charset_of("text/html; charset=windows-1256").as_deref(),
            Some("windows-1256")
        );
        assert_eq!(
            meta_charset(b"<meta charset=\"UTF-8\">").as_deref(),
            Some("utf-8")
        );
        // "سلام" in Windows-1256
        assert_eq!(
            decode(&[0xD3, 0xE1, 0xC7, 0xE3], Some("windows-1256")),
            "سلام"
        );
        assert_eq!(decode("نص".as_bytes(), None), "نص");
    }
}
