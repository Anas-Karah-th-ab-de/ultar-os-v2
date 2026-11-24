//! What the address field makes of the typed text: a web address or a search.
//!
//! Typed text is hostile input: it is limited in length, control characters are refused and
//! only `http`, `https` and `about:blank` are opened. Schemes that run code or reach local
//! files (`javascript:`, `data:`, `file:` …) are refused instead of being searched for, so a
//! pasted `javascript:` link never runs.

use std::fmt;

/// Longest address the browser opens (bytes).
pub const MAX_URL: usize = 8192;

/// The search engines the user can choose (Settings); all without an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SearchEngine {
    /// DuckDuckGo (default: no tracking).
    #[default]
    DuckDuckGo,
    /// Startpage.
    Startpage,
    /// Brave Search.
    Brave,
}

impl SearchEngine {
    /// Every engine in the order of the settings list.
    pub const ALL: [SearchEngine; 3] = [
        SearchEngine::DuckDuckGo,
        SearchEngine::Startpage,
        SearchEngine::Brave,
    ];

    /// Stable identifier (settings file).
    pub fn id(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "duckduckgo",
            SearchEngine::Startpage => "startpage",
            SearchEngine::Brave => "brave",
        }
    }

    /// The engine with identifier `id`.
    pub fn from_id(id: &str) -> Option<SearchEngine> {
        SearchEngine::ALL.into_iter().find(|e| e.id() == id)
    }

    /// Name shown in the settings (a proper name, the same in every language).
    pub fn name(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Startpage => "Startpage",
            SearchEngine::Brave => "Brave Search",
        }
    }

    /// Address of the results for `terms`.
    pub fn query_url(self, terms: &str) -> String {
        let base = match self {
            SearchEngine::DuckDuckGo => "https://duckduckgo.com/?q=",
            SearchEngine::Startpage => "https://www.startpage.com/do/search?q=",
            SearchEngine::Brave => "https://search.brave.com/search?q=",
        };
        format!("{base}{}", encode_query(terms))
    }
}

/// Why typed text is not opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrlError {
    /// Nothing typed.
    Empty,
    /// Longer than [`MAX_URL`].
    TooLong,
    /// Contains control characters.
    ControlCharacter,
    /// A scheme the browser does not open (`javascript:`, `file:` …).
    RefusedScheme,
    /// `http(s)://` without a valid host.
    BadHost,
}

impl fmt::Display for UrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            UrlError::Empty => "empty input",
            UrlError::TooLong => "address too long",
            UrlError::ControlCharacter => "control character in the address",
            UrlError::RefusedScheme => "scheme not allowed",
            UrlError::BadHost => "invalid host",
        };
        f.write_str(text)
    }
}

impl std::error::Error for UrlError {}

impl UrlError {
    /// Catalog key of the message shown under the address field.
    pub fn key(self) -> &'static str {
        match self {
            UrlError::Empty => "web.error.empty",
            UrlError::TooLong => "web.error.too-long",
            UrlError::ControlCharacter | UrlError::BadHost => "web.error.invalid",
            UrlError::RefusedScheme => "web.error.scheme",
        }
    }
}

/// What to open for typed text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The address to load.
    pub url: String,
    /// `true` when the text was searched for.
    pub searched: bool,
}

/// Turns typed text into an address or a search with `engine`.
pub fn resolve(input: &str, engine: SearchEngine) -> Result<Resolved, UrlError> {
    let text = input.trim();
    if text.is_empty() {
        return Err(UrlError::Empty);
    }
    if text.len() > MAX_URL {
        return Err(UrlError::TooLong);
    }
    if text.chars().any(char::is_control) {
        return Err(UrlError::ControlCharacter);
    }
    if text.eq_ignore_ascii_case("about:blank") {
        return Ok(page("about:blank".into()));
    }
    if let Some((scheme, rest)) = split_scheme(text) {
        return match scheme.as_str() {
            "http" | "https" => absolute(&scheme, rest).map(page),
            _ => Err(UrlError::RefusedScheme),
        };
    }
    if !text.contains(char::is_whitespace) {
        if let Some(url) = bare_address(text) {
            return Ok(page(url));
        }
    }
    Ok(Resolved {
        url: engine.query_url(text),
        searched: true,
    })
}

fn page(url: String) -> Resolved {
    Resolved {
        url,
        searched: false,
    }
}

/// `scheme:` at the start (letters, digits, `+-.`, starting with a letter), lower-cased.
/// `host:port` is not a scheme (a digit follows the colon).
fn split_scheme(text: &str) -> Option<(String, &str)> {
    let colon = text.find(':')?;
    let (scheme, rest) = (&text[..colon], &text[colon + 1..]);
    let mut chars = scheme.chars();
    let first_ok = chars.next().is_some_and(|c| c.is_ascii_alphabetic());
    let valid = first_ok && chars.all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !valid || rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((scheme.to_ascii_lowercase(), rest))
}

/// `http(s)://host…` with a checked, lower-cased host.
fn absolute(scheme: &str, rest: &str) -> Result<String, UrlError> {
    let rest = rest.strip_prefix("//").ok_or(UrlError::BadHost)?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let host = host_part(authority).ok_or(UrlError::BadHost)?;
    let port = &authority[authority.len() - port_len(authority)..];
    let tail = if tail.is_empty() { "/" } else { tail };
    Ok(format!(
        "{scheme}://{}{port}{tail}",
        host.to_ascii_lowercase()
    ))
}

/// Length of a `:port` suffix of an authority (0 without one).
fn port_len(authority: &str) -> usize {
    match authority.rfind(':') {
        Some(i)
            if !authority[i + 1..].is_empty()
                && authority[i + 1..].chars().all(|c| c.is_ascii_digit())
                && !authority.ends_with(']') =>
        {
            authority.len() - i
        }
        _ => 0,
    }
}

/// The host of an authority (no user info, valid characters, not empty).
fn host_part(authority: &str) -> Option<&str> {
    if authority.contains('@') {
        return None;
    }
    let host = &authority[..authority.len() - port_len(authority)];
    let port = &authority[host.len()..];
    if port.len() > 6 || port.get(1..).is_some_and(|p| p.parse::<u16>().is_err()) {
        return None;
    }
    let ok = !host.is_empty()
        && host.len() <= 253
        && host
            .chars()
            .all(|c| c.is_alphanumeric() || "-.[]:".contains(c));
    ok.then_some(host)
}

/// Typed text without a scheme that looks like an address: `localhost`, an IPv4 address or
/// a name with a dot and a top-level part of letters (≥ 2).
fn bare_address(text: &str) -> Option<String> {
    let end = text.find(['/', '?', '#']).unwrap_or(text.len());
    let authority = &text[..end];
    let host = host_part(authority)?;
    let plain = host == "localhost" || is_ipv4(host);
    let tld = host.rsplit('.').next().unwrap_or("");
    let named = host.contains('.')
        && !host.starts_with('.')
        && tld.chars().count() >= 2
        && tld.chars().all(char::is_alphabetic);
    if !(plain || named) {
        return None;
    }
    let scheme = if plain { "http" } else { "https" };
    absolute(scheme, &format!("//{text}")).ok()
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.parse::<u8>().is_ok())
}

/// Percent-encodes search terms (`+` for spaces, unreserved characters kept).
pub fn encode_query(terms: &str) -> String {
    let mut out = String::with_capacity(terms.len() * 3);
    for byte in terms.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Percent-encodes one segment of a file path for a `file://` address.
pub fn encode_path_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// The host of an `http(s)` address (lower case), `None` for anything else.
pub fn host_of(url: &str) -> Option<String> {
    let (scheme, rest) = split_scheme(url)?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let rest = rest.strip_prefix("//")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    host_part(&rest[..end]).map(str::to_ascii_lowercase)
}

/// `true` for an `https` address.
pub fn is_secure(url: &str) -> bool {
    split_scheme(url).is_some_and(|(s, _)| s == "https")
}

/// The text a tab shows for an address without a title: the host, else the address.
pub fn short_name(url: &str) -> String {
    host_of(url).unwrap_or_else(|| url.chars().take(64).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open(text: &str) -> String {
        resolve(text, SearchEngine::DuckDuckGo).unwrap().url
    }

    #[test]
    fn addresses_and_searches() {
        assert_eq!(open("code-craft.tech"), "https://code-craft.tech/");
        assert_eq!(
            open("  HTTPS://Code-Craft.TECH/a?b#c "),
            "https://code-craft.tech/a?b#c"
        );
        assert_eq!(open("localhost:8080/x"), "http://localhost:8080/x");
        assert_eq!(open("127.0.0.1"), "http://127.0.0.1/");
        assert_eq!(open("http://[::1]:80/"), "http://[::1]:80/");
        assert_eq!(open("about:BLANK"), "about:blank");
        let s = resolve("كيف حالك", SearchEngine::DuckDuckGo).unwrap();
        assert!(s.searched);
        assert_eq!(
            s.url,
            "https://duckduckgo.com/?q=%D9%83%D9%8A%D9%81+%D8%AD%D8%A7%D9%84%D9%83"
        );
        assert!(resolve("rust lang", SearchEngine::Brave)
            .unwrap()
            .url
            .starts_with("https://search.brave.com/"));
        assert!(resolve("word", SearchEngine::Startpage).unwrap().searched);
        assert!(resolve("a.b1", SearchEngine::DuckDuckGo).unwrap().searched);
        assert!(
            resolve("1.2.3.999", SearchEngine::DuckDuckGo)
                .unwrap()
                .searched
        );
    }

    #[test]
    fn hostile_input_is_refused() {
        let e = |t: &str| resolve(t, SearchEngine::DuckDuckGo).unwrap_err();
        assert_eq!(e("   "), UrlError::Empty);
        assert_eq!(e(&"a".repeat(MAX_URL + 1)), UrlError::TooLong);
        assert_eq!(e("a\u{7}b"), UrlError::ControlCharacter);
        assert_eq!(e("javascript:alert(1)"), UrlError::RefusedScheme);
        assert_eq!(e("JavaScript:alert(1)"), UrlError::RefusedScheme);
        assert_eq!(e("file:///etc/passwd"), UrlError::RefusedScheme);
        assert_eq!(e("data:text/html,x"), UrlError::RefusedScheme);
        assert_eq!(e("https://user@evil.example/"), UrlError::BadHost);
        assert_eq!(e("https://"), UrlError::BadHost);
        assert_eq!(e("http:x"), UrlError::BadHost);
        assert_eq!(e("https://a b/"), UrlError::BadHost);
        assert_eq!(e("https://host:99999/"), UrlError::BadHost);
        for err in [UrlError::Empty, UrlError::TooLong, UrlError::RefusedScheme] {
            assert!(err.key().starts_with("web.error."));
            assert!(!err.to_string().is_empty());
        }
    }

    #[test]
    fn hosts_and_names() {
        assert_eq!(
            host_of("https://Code-Craft.tech/x").as_deref(),
            Some("code-craft.tech")
        );
        assert_eq!(host_of("http://localhost:8080"), Some("localhost".into()));
        assert_eq!(host_of("about:blank"), None);
        assert_eq!(host_of("ftp://x.y/"), None);
        assert!(is_secure("https://a.b/"));
        assert!(!is_secure("http://a.b/"));
        assert_eq!(short_name("https://a.example/p"), "a.example");
        assert_eq!(short_name("about:blank"), "about:blank");
        assert_eq!(SearchEngine::from_id("brave"), Some(SearchEngine::Brave));
        assert_eq!(SearchEngine::from_id("x"), None);
        assert!(SearchEngine::ALL.iter().all(|e| !e.name().is_empty()));
        assert_eq!(encode_query("a b&c"), "a+b%26c");
    }

    #[test]
    fn fuzzed_input_never_panics_and_never_opens_other_schemes() {
        let mut seed = 0x2545_f491_u64;
        let alphabet: Vec<char> = "ab:/.%@?#[]-_ 1J\u{0627}\u{1}xvascript".chars().collect();
        for _ in 0..20_000 {
            let len = (seed % 24) as usize;
            let mut text = String::new();
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                text.push(alphabet[(seed % alphabet.len() as u64) as usize]);
            }
            if let Ok(r) = resolve(&text, SearchEngine::DuckDuckGo) {
                assert!(
                    r.url.starts_with("https://")
                        || r.url.starts_with("http://")
                        || r.url == "about:blank",
                    "{text:?} -> {}",
                    r.url
                );
            }
        }
    }
}
