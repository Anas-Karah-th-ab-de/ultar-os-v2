//! Fetching a page in pure Rust: HTTP and HTTPS with `ureq` (redirects followed, size and
//! time bounded), TLS by rustls with the Graviola provider where the CPU and the kernel offer
//! its instructions, else the RustCrypto provider; Mozilla's roots compiled in. Text is
//! decoded with the charset the server or the page names (UTF-8, Windows-1256, …).

use crate::privacy::guard::{self, Blocked};
use crate::privacy::persona::Persona;
use std::io::Read;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

/// Largest page read.
pub const MAX_BYTES: u64 = 4 * 1024 * 1024;
/// Largest sound or video file fetched for playing.
pub const MAX_MEDIA: u64 = 300 * 1024 * 1024;
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
    /// The address is refused: this device, the local network or not http(s).
    Blocked(Blocked),
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

/// How the client reaches the internet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Through Tor: the proxy address of the browser's Tor client
    /// (`socks5h://secret@127.0.0.1:port`, names resolved by Tor).
    Tor(String),
    /// Straight (tests on this machine only; pages never go this way).
    Direct,
}

/// Most redirects followed (each one is checked like the first address).
const MAX_REDIRECTS: usize = 10;

/// The HTTP client of the reader: every request carries only the persona's headers, every
/// address (and every redirect) passes the guard, everything goes through Tor.
#[derive(Clone)]
pub struct Client {
    agent: ureq::Agent,
    persona: Arc<Persona>,
    /// Tests on this machine reach their local server (never in the browser).
    #[cfg(test)]
    pub(crate) allow_local: bool,
}

impl Client {
    /// A client for `persona` along `route`.
    pub fn new(persona: Persona, route: &Route) -> Client {
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
        let proxy = match route {
            Route::Tor(url) => ureq::Proxy::new(url).ok(),
            Route::Direct => None,
        };
        let config = ureq::Agent::config_builder()
            .tls_config(tls)
            .user_agent(persona.user_agent)
            // redirects are followed here, each one checked by the guard
            .max_redirects(0)
            .save_redirect_history(false)
            .http_status_as_error(false)
            .proxy(proxy)
            .timeout_resolve(Some(TIMEOUT))
            .timeout_connect(Some(TIMEOUT))
            .timeout_send_request(Some(TIMEOUT))
            .timeout_recv_response(Some(TIMEOUT))
            .timeout_per_read(Some(TIMEOUT))
            .build();
        Client {
            agent: ureq::Agent::new_with_config(config),
            persona: Arc::new(persona),
            #[cfg(test)]
            allow_local: false,
        }
    }

    /// The identity pages see.
    pub fn persona(&self) -> &Persona {
        &self.persona
    }

    fn check(&self, url: &str) -> Result<(), FetchError> {
        #[cfg(test)]
        if self.allow_local {
            return Ok(());
        }
        guard::check(url).map_err(FetchError::Blocked)
    }

    /// Asks for `url`, following redirects; gives the answer and the final address.
    fn open(&self, url: &str) -> Result<(ureq::http::Response<ureq::Body>, String), FetchError> {
        let mut url = url.to_owned();
        for _ in 0..=MAX_REDIRECTS {
            self.check(&url)?;
            let mut request = self.agent.get(&url);
            for (name, value) in self.persona.headers() {
                request = request.header(name, value);
            }
            let response = request
                .call()
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            if !response.status().is_redirection() {
                return Ok((response, url));
            }
            let to = header(&response, "location")
                .ok_or(FetchError::Status(response.status().as_u16()))?;
            url = super::html::join(&url, &to).ok_or(FetchError::Blocked(Blocked::Scheme))?;
        }
        Err(FetchError::Unreachable("too many redirects".into()))
    }

    /// Fetches `url` (http or https).
    pub fn get(&self, url: &str) -> Result<Fetched, FetchError> {
        // the address after redirects (links of the page are relative to it)
        let (response, final_url) = self.open(url)?;
        let status = response.status().as_u16();
        let content_type = header(&response, "content-type")
            .unwrap_or_else(|| "text/html".into())
            .to_ascii_lowercase();
        if !(200..300).contains(&status) {
            return Err(FetchError::Status(status));
        }
        let mime = mime_of(&content_type);
        if is_media(&mime) {
            // played by the media player, which fetches it itself (Client::get_media)
            return Ok(Fetched {
                url: final_url,
                mime,
                text: String::new(),
            });
        }
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

/// A header of `response` as text.
fn header(response: &ureq::http::Response<ureq::Body>, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
}

/// The media type of a `Content-Type` value (lower case, without parameters).
fn mime_of(content_type: &str) -> String {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_owned()
}

/// `true` for a sound or video type.
pub fn is_media(mime: &str) -> bool {
    mime.starts_with("audio/") || mime.starts_with("video/")
}

impl Client {
    /// Fetches a sound or video file for playing (up to [`MAX_MEDIA`]): fills `progress` with
    /// per mille done (when the size is known) and stops early when `stop` is set. Gives the
    /// bytes and the media type.
    pub fn get_media(
        &self,
        url: &str,
        progress: &AtomicU32,
        stop: &AtomicBool,
    ) -> Result<(Vec<u8>, String), FetchError> {
        let (response, _) = self.open(url)?;
        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            return Err(FetchError::Status(status));
        }
        let mime = mime_of(
            &header(&response, "content-type")
                .unwrap_or_default()
                .to_ascii_lowercase(),
        );
        let total = header(&response, "content-length").and_then(|v| v.parse::<u64>().ok());
        if total.is_some_and(|t| t > MAX_MEDIA) {
            return Err(FetchError::TooLarge);
        }
        let mut reader = response.into_body().into_reader();
        let mut bytes = Vec::with_capacity(total.unwrap_or(0).min(MAX_MEDIA) as usize);
        let mut chunk = vec![0u8; 64 * 1024];
        loop {
            if stop.load(Ordering::Relaxed) {
                return Err(FetchError::Unreachable("stopped".into()));
            }
            let n = reader
                .read(&mut chunk)
                .map_err(|e| FetchError::Unreachable(e.to_string()))?;
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&chunk[..n]);
            if bytes.len() as u64 > MAX_MEDIA {
                return Err(FetchError::TooLarge);
            }
            if let Some(t) = total.filter(|t| *t > 0) {
                progress.store(
                    (bytes.len() as u64 * 1000 / t).min(1000) as u32,
                    Ordering::Relaxed,
                );
            }
        }
        Ok((bytes, mime))
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
