//! A forgiving HTML reader: the text of a page as blocks (headings, paragraphs, list items,
//! quotes, preformatted text) with links, the title and the base address. Scripts, styles
//! and everything invisible are left out; no script ever runs. Input is hostile: sizes are
//! bounded, unknown tags are text containers, broken markup ends a block, nothing panics.

/// Most blocks of one page.
pub const MAX_BLOCKS: usize = 5000;
/// Most bytes of text of one block.
pub const MAX_BLOCK_TEXT: usize = 16 * 1024;

/// What a block is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Heading of level 1 to 3 (4–6 count as 3).
    Heading(u8),
    /// Running text.
    Para,
    /// A list item.
    Item,
    /// A quote.
    Quote,
    /// Preformatted text (line breaks kept).
    Pre,
}

/// A link inside a block: bytes `start..end` of its text lead to `href` (absolute).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// First byte of the link text.
    pub start: usize,
    /// End of the link text (exclusive).
    pub end: usize,
    /// Where it leads (absolute http or https).
    pub href: String,
}

/// One block of text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// What it is.
    pub kind: Kind,
    /// Its text (white space collapsed, except in [`Kind::Pre`]).
    pub text: String,
    /// Its links.
    pub links: Vec<Link>,
}

/// A read page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    /// The `<title>`.
    pub title: String,
    /// The text, in order.
    pub blocks: Vec<Block>,
}

/// Elements whose content is never shown.
const HIDDEN: [&str; 8] = [
    "script", "style", "noscript", "template", "svg", "iframe", "object", "select",
];
/// Elements that end a block.
const BLOCKS: [&str; 30] = [
    "p",
    "div",
    "section",
    "article",
    "main",
    "header",
    "footer",
    "nav",
    "aside",
    "ul",
    "ol",
    "li",
    "dl",
    "dt",
    "dd",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "pre",
    "blockquote",
    "table",
    "tr",
    "form",
    "figure",
    "figcaption",
    "hr",
    "address",
];

struct Builder {
    doc: Document,
    text: String,
    links: Vec<Link>,
    kind: Kind,
    link: Option<(usize, String)>,
    pre: usize,
    base: String,
}

impl Builder {
    fn flush(&mut self) {
        self.close_link();
        let text = if self.pre > 0 {
            self.text.trim_end().to_owned()
        } else {
            self.text.trim().to_owned()
        };
        let cut = self.text.len() - self.text.trim_start().len();
        if !text.is_empty() && self.doc.blocks.len() < MAX_BLOCKS {
            let shift = if self.pre > 0 { 0 } else { cut };
            let links = std::mem::take(&mut self.links)
                .into_iter()
                .filter_map(|l| {
                    let start = l.start.saturating_sub(shift).min(text.len());
                    let end = l.end.saturating_sub(shift).min(text.len());
                    (end > start).then_some(Link {
                        start,
                        end,
                        href: l.href,
                    })
                })
                .collect();
            self.doc.blocks.push(Block {
                kind: self.kind,
                text,
                links,
            });
        }
        self.text.clear();
        self.links.clear();
        self.kind = if self.pre > 0 { Kind::Pre } else { Kind::Para };
    }

    fn push_text(&mut self, raw: &str) {
        if self.text.len() >= MAX_BLOCK_TEXT {
            return;
        }
        if self.pre > 0 {
            self.text.push_str(raw);
            return;
        }
        for c in raw.chars() {
            if c.is_whitespace() {
                if !self.text.is_empty() && !self.text.ends_with(' ') {
                    self.text.push(' ');
                }
            } else {
                self.text.push(c);
            }
        }
    }

    fn close_link(&mut self) {
        if let Some((start, href)) = self.link.take() {
            let end = self.text.trim_end().len();
            if end > start {
                self.links.push(Link { start, end, href });
            }
        }
    }
}

/// Reads `html` fetched from `url`.
pub fn read(html: &str, url: &str) -> Document {
    let mut b = Builder {
        doc: Document::default(),
        text: String::new(),
        links: Vec::new(),
        kind: Kind::Para,
        link: None,
        pre: 0,
        base: url.to_owned(),
    };
    let mut hidden: Option<String> = None;
    let mut in_title = false;
    let mut title = String::new();
    let mut rest = html;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            text(&mut b, rest, hidden.is_some(), in_title, &mut title);
            break;
        };
        text(&mut b, &rest[..lt], hidden.is_some(), in_title, &mut title);
        rest = &rest[lt..];
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |i| &after[i + 3..]);
            continue;
        }
        let Some(gt) = tag_end(rest) else {
            break;
        };
        let tag = &rest[1..gt];
        rest = &rest[gt + 1..];
        let (name, closing, attrs) = parse_tag(tag);
        if let Some(h) = &hidden {
            if closing && name == *h {
                hidden = None;
            }
            continue;
        }
        if name == "title" {
            in_title = !closing;
            continue;
        }
        if !closing && HIDDEN.contains(&name.as_str()) {
            if !tag.trim_end().ends_with('/') {
                hidden = Some(name);
            }
            continue;
        }
        element(&mut b, &name, closing, attrs);
    }
    b.flush();
    b.doc.title = title.split_whitespace().collect::<Vec<_>>().join(" ");
    b.doc
}

fn text(b: &mut Builder, raw: &str, hidden: bool, in_title: bool, title: &mut String) {
    if raw.is_empty() {
        return;
    }
    let decoded = entities(raw);
    if in_title {
        if title.len() < 512 {
            title.push_str(&decoded);
        }
    } else if !hidden {
        b.push_text(&decoded);
    }
}

fn element(b: &mut Builder, name: &str, closing: bool, attrs: &str) {
    match (name, closing) {
        ("a", false) => {
            b.close_link();
            if let Some(href) = attr(attrs, "href").and_then(|h| join(&b.base, &h)) {
                b.link = Some((b.text.len(), href));
            }
        }
        ("a", true) => b.close_link(),
        ("base", false) => {
            if let Some(h) = attr(attrs, "href").and_then(|h| join(&b.base, &h)) {
                b.base = h;
            }
        }
        ("br", _) => {
            if b.pre > 0 {
                b.text.push('\n');
            } else {
                b.flush();
            }
        }
        ("img", false) => {
            if let Some(alt) = attr(attrs, "alt").filter(|a| !a.trim().is_empty()) {
                b.push_text(&format!(" [{}] ", alt.trim()));
            }
        }
        ("td" | "th", false) => b.push_text(" "),
        _ if BLOCKS.contains(&name) => {
            b.flush();
            if closing {
                if name == "pre" {
                    b.pre = b.pre.saturating_sub(1);
                    b.kind = Kind::Para;
                }
                return;
            }
            b.kind = match name {
                "h1" => Kind::Heading(1),
                "h2" => Kind::Heading(2),
                "h3" | "h4" | "h5" | "h6" => Kind::Heading(3),
                "li" | "dt" => Kind::Item,
                "blockquote" => Kind::Quote,
                "pre" => {
                    b.pre += 1;
                    Kind::Pre
                }
                _ => b.kind,
            };
        }
        _ => {}
    }
}

/// End of a tag starting at `s[0] == '<'`: the `>` outside quotes.
fn tag_end(s: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// Name (lower case), closing?, attributes of a tag's inside.
fn parse_tag(tag: &str) -> (String, bool, &str) {
    let t = tag.trim_start();
    let (closing, t) = match t.strip_prefix('/') {
        Some(rest) => (true, rest),
        None => (false, t),
    };
    let end = t
        .find(|c: char| c.is_whitespace() || c == '/' || c == '>')
        .unwrap_or(t.len());
    (t[..end].to_ascii_lowercase(), closing, &t[end..])
}

/// The value of attribute `name` (entities decoded).
fn attr(attrs: &str, name: &str) -> Option<String> {
    let lower = attrs.to_ascii_lowercase();
    let mut from = 0;
    while let Some(i) = lower[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let before_ok = at == 0 || lower.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = attrs[from..].trim_start();
        if !before_ok || !rest.starts_with('=') {
            continue;
        }
        let v = rest[1..].trim_start();
        let value = match v.chars().next() {
            Some(q @ ('"' | '\'')) => v[1..].split(q).next().unwrap_or(""),
            _ => v
                .split(|c: char| c.is_whitespace() || c == '>')
                .next()
                .unwrap_or(""),
        };
        return Some(entities(value));
    }
    None
}

/// Decodes the common named entities and numeric references.
pub fn entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let end = rest[..rest.len().min(12)].find(';');
        let decoded = end.and_then(|e| entity(&rest[1..e]).map(|c| (c, e + 1)));
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn entity(name: &str) -> Option<char> {
    if let Some(num) = name.strip_prefix('#') {
        let n = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok()?,
            None => num.parse().ok()?,
        };
        return char::from_u32(n).filter(|c| !c.is_control() || c.is_whitespace());
    }
    Some(match name {
        "amp" => '&',
        "lt" => '<',
        "gt" => '>',
        "quot" => '"',
        "apos" => '\'',
        "nbsp" => ' ',
        "copy" => '©',
        "reg" => '®',
        "laquo" => '«',
        "raquo" => '»',
        "hellip" => '…',
        "mdash" => '—',
        "ndash" => '–',
        "middot" => '·',
        "rsquo" => '’',
        "lsquo" => '‘',
        "rdquo" => '”',
        "ldquo" => '“',
        "zwnj" => '\u{200C}',
        "zwj" => '\u{200D}',
        "rlm" => '\u{200F}',
        "lrm" => '\u{200E}',
        _ => return None,
    })
}

/// `href` made absolute against `base`; only http(s) links are kept.
pub fn join(base: &str, href: &str) -> Option<String> {
    let href = href.trim();
    if href.is_empty() || href.starts_with('#') {
        return None;
    }
    let lower = href.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        return Some(href.to_owned());
    }
    if lower.contains(':') && !lower.starts_with('/') && !lower.starts_with('?') {
        let colon = lower.find(':').unwrap_or(0);
        let slash = lower.find('/').unwrap_or(usize::MAX);
        if colon < slash {
            return None; // mailto:, javascript:, data: …
        }
    }
    let (scheme, rest) = base.split_once("://")?;
    let host_end = rest.find('/').unwrap_or(rest.len());
    let host = &rest[..host_end];
    let path = &rest[host_end..];
    if let Some(net) = href.strip_prefix("//") {
        return Some(format!("{scheme}://{net}"));
    }
    if href.starts_with('/') {
        return Some(format!("{scheme}://{host}{href}"));
    }
    let path = path.split(['?', '#']).next().unwrap_or("");
    if href.starts_with('?') {
        return Some(format!("{scheme}://{host}{path}{href}"));
    }
    let dir = &path[..path.rfind('/').map_or(0, |i| i + 1)];
    let mut parts: Vec<&str> = Vec::new();
    let joined = format!("{dir}{href}");
    let (joined_path, query) = match joined.find(['?', '#']) {
        Some(i) => (&joined[..i], &joined[i..]),
        None => (joined.as_str(), ""),
    };
    for seg in joined_path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    let trailing = if joined_path.ends_with('/') && !parts.is_empty() {
        "/"
    } else {
        ""
    };
    Some(format!(
        "{scheme}://{host}/{}{trailing}{query}",
        parts.join("/")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_links_title_and_hidden_parts() {
        let html = r#"<html><head><title> سوريا &amp; العالم </title><style>p{}</style>
            <script>alert("x<p>")</script></head><body>
            <h1>عنوان</h1><p>نص <a href="/wiki/دمشق">دمشق</a> هنا.</p>
            <ul><li>one</li><li><a href='https://example.org/x'>two</a></li></ul>
            <pre>a  b
c</pre><!-- <p>no</p> --><img src=x alt="صورة"></body></html>"#;
        let d = read(html, "https://ar.wikipedia.org/wiki/سوريا");
        assert_eq!(d.title, "سوريا & العالم");
        let texts: Vec<&str> = d.blocks.iter().map(|b| b.text.as_str()).collect();
        assert_eq!(
            texts,
            ["عنوان", "نص دمشق هنا.", "one", "two", "a  b\nc", "[صورة]"]
        );
        assert_eq!(d.blocks[0].kind, Kind::Heading(1));
        assert_eq!(d.blocks[2].kind, Kind::Item);
        assert_eq!(d.blocks[4].kind, Kind::Pre);
        let link = &d.blocks[1].links[0];
        assert_eq!(&d.blocks[1].text[link.start..link.end], "دمشق");
        assert_eq!(link.href, "https://ar.wikipedia.org/wiki/دمشق");
        assert_eq!(d.blocks[3].links[0].href, "https://example.org/x");
    }

    #[test]
    fn broken_markup_does_not_panic() {
        for html in [
            "<",
            "<a href='x",
            "<p>a<b",
            "&#xFFFFFFF;",
            "<!--",
            "<a href=>x</a>",
            "&",
        ] {
            let _ = read(html, "http://h/");
        }
        assert_eq!(entities("&lt;&#1587;&#x0645;&bogus;"), "<سم&bogus;");
    }

    #[test]
    fn relative_links() {
        let b = "https://h.org/a/b/c.html?q=1";
        assert_eq!(
            join(b, "d.html").as_deref(),
            Some("https://h.org/a/b/d.html")
        );
        assert_eq!(join(b, "../d").as_deref(), Some("https://h.org/a/d"));
        assert_eq!(join(b, "/x?y").as_deref(), Some("https://h.org/x?y"));
        assert_eq!(join(b, "//cdn.org/z").as_deref(), Some("https://cdn.org/z"));
        assert_eq!(
            join(b, "?p=2").as_deref(),
            Some("https://h.org/a/b/c.html?p=2")
        );
        assert_eq!(join(b, "javascript:alert(1)"), None);
        assert_eq!(join(b, "mailto:a@b"), None);
        assert_eq!(join(b, "#top"), None);
    }
}
