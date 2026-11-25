//! Links in the model's answers (`assistant.check_links`): the helper warns about suspicious
//! ones before the person opens them. The check is local and deterministic – no request goes
//! out. Suspicious: a scheme other than http/https (javascript:, data:, file: …), a user-info
//! part (`http://name@host`), a host that is an address as numbers, or a punycode host
//! (`xn--`, the homograph trick).

/// Most links looked at in one answer.
const MAX_LINKS: usize = 8;
/// Longest link kept, in bytes.
const MAX_LINK: usize = 256;
/// Longest scheme looked at, in bytes.
const MAX_SCHEME: usize = 32;

/// The colon after a scheme at the start of `s` (`https:` → 5); `None` when there is none.
fn scheme_colon(s: &str) -> Option<usize> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    for (i, c) in chars.take(MAX_SCHEME) {
        if c == ':' {
            return Some(i);
        }
        if !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
            return None;
        }
    }
    None
}

/// Where a link ends: at whitespace or a character that cannot be part of one in a message.
fn link_end(s: &str) -> usize {
    s.find(|c: char| c.is_whitespace() || "\"'<>{}[]".contains(c))
        .unwrap_or(s.len())
}

/// The links of a text: `scheme://…` and the scheme-only forms `javascript:` / `data:`,
/// in order, without repeats (at most [`MAX_LINKS`]).
pub fn links_in(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (i, _) in text.char_indices() {
        if out.len() >= MAX_LINKS {
            break;
        }
        // A scheme starts at a word boundary, not inside one ("xhttp://" is not a link).
        let before = text[..i].chars().next_back();
        if before.is_some_and(|c| c.is_ascii_alphanumeric() || "+-.".contains(c)) {
            continue;
        }
        let rest = &text[i..];
        let Some(colon) = scheme_colon(rest) else {
            continue;
        };
        let scheme = rest[..colon].to_ascii_lowercase();
        let after = &rest[colon + 1..];
        if !after.starts_with("//") && scheme != "javascript" && scheme != "data" {
            continue;
        }
        let end = link_end(after);
        let url = format!("{}:{}", &rest[..colon], &after[..end]);
        let mut url = url
            .trim_end_matches(['.', ',', ';', ':', '!', '?'])
            .to_owned();
        // A closing parenthesis of the sentence, not one of the link itself.
        while url.ends_with(')') && url.matches(')').count() > url.matches('(').count() {
            url.pop();
        }
        if url.len() <= MAX_LINK && !out.contains(&url) {
            out.push(url);
        }
    }
    out
}

/// Whether a link looks suspicious (the local checks above; `false` for no link).
pub fn suspicious(url: &str) -> bool {
    let Some(colon) = scheme_colon(url) else {
        return false;
    };
    let scheme = url[..colon].to_ascii_lowercase();
    let rest = &url[colon + 1..];
    if scheme != "http" && scheme != "https" {
        return true;
    }
    let authority = rest.strip_prefix("//").unwrap_or(rest);
    if authority.contains('@') {
        return true;
    }
    let host = authority
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    if host.starts_with('[') || host.starts_with("xn--") || host.contains(".xn--") {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 3 && p.chars().all(|c| c.is_ascii_digit()))
}

/// The first suspicious link of a text (`None`: nothing to warn about).
pub fn first_suspicious(text: &str) -> Option<String> {
    links_in(text).into_iter().find(|url| suspicious(url))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_in_the_words_of_a_model() {
        let text = "Open https://example.com/page, or http://ultar.org/docs. \
                    See also https://example.com/page (again).";
        assert_eq!(
            links_in(text),
            vec!["https://example.com/page", "http://ultar.org/docs"]
        );
        assert_eq!(
            links_in("run javascript:alert(1) now"),
            vec!["javascript:alert(1)"]
        );
        assert_eq!(
            links_in("data:text/plain,hello"),
            vec!["data:text/plain,hello"]
        );
        // A scheme inside a word does not start a second link; a plain word with a colon
        // is not one.
        assert_eq!(
            links_in("foxhttp://example.com"),
            vec!["foxhttp://example.com"]
        );
        assert!(links_in("note: nothing here").is_empty());
        assert!(links_in("").is_empty());
        // The parenthesis of the sentence is not part of the link; one of the link is.
        assert_eq!(
            links_in("(see https://example.com/a) and javascript:alert(1)"),
            vec!["https://example.com/a", "javascript:alert(1)"]
        );
    }

    #[test]
    fn ordinary_links_are_not_suspicious() {
        for url in [
            "https://example.com",
            "https://example.com:8443/path?q=1#top",
            "http://ultar.org",
            "https://sub.example.co.uk/a",
            "https://例子.测试",
        ] {
            assert!(!suspicious(url), "{url}");
        }
        assert_eq!(first_suspicious("see https://example.com"), None);
    }

    #[test]
    fn the_tricks_are_suspicious() {
        for url in [
            "javascript:alert(1)",
            "data:text/html,<script>",
            "file:///etc/passwd",
            "http://bank@example.com",
            "https://user:pass@example.com",
            "http://203.0.113.7/login",
            "http://192.168.1.1",
            "https://xn--80ak6aa92e.com",
            "https://shop.xn--p1ai",
            "http://[2001:db8::1]/",
        ] {
            assert!(suspicious(url), "{url}");
        }
        assert_eq!(
            first_suspicious("open https://good.org then http://203.0.113.7/x").as_deref(),
            Some("http://203.0.113.7/x")
        );
        // A host with a number in it is not an address as numbers.
        assert!(!suspicious("https://shop24.example.com"));
        assert!(!suspicious("https://1.2.3.4.5.example.com"));
    }
}
