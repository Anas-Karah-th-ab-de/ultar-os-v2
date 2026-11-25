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
    /// A sound or a video; its one link is the file, its text the name shown.
    Media(MediaKind),
}

/// What a media block plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKind {
    /// `<audio>`.
    Audio,
    /// `<video>`.
    Video,
}

/// A `<video>` or `<audio>` being read.
struct PendingMedia {
    kind: MediaKind,
    src: Option<String>,
    label: String,
}

impl Block {
    /// A media block for the file at `src` shown as `label`.
    pub fn media(kind: MediaKind, src: &str, label: &str) -> Block {
        let text = if label.trim().is_empty() {
            file_name(src)
        } else {
            label.trim().to_owned()
        };
        Block {
            kind: Kind::Media(kind),
            links: vec![Link {
                start: 0,
                end: text.len(),
                href: src.to_owned(),
            }],
            text,
        }
    }

    /// The file of a media block.
    pub fn media_src(&self) -> Option<&str> {
        matches!(self.kind, Kind::Media(_)).then(|| self.links.first().map(|l| l.href.as_str()))?
    }
}

/// The last part of an address, decoded loosely (the name of a media file).
fn file_name(src: &str) -> String {
    let path = src.split(['?', '#']).next().unwrap_or(src);
    let last = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or(path);
    if last.is_empty() {
        src.to_owned()
    } else {
        last.replace("%20", " ")
    }
}

/// Media types the player plays best (a `<source>` of one of them is taken first).
const PLAYABLE: [&str; 7] = ["mp4", "mpeg", "aac", "wav", "ogg", "flac", "m4a"];

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
    media: Option<PendingMedia>,
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
        // the fallback text inside <video>/<audio> ("your browser cannot …") is not shown
        if self.media.is_some() || self.text.len() >= MAX_BLOCK_TEXT {
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
        media: None,
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
    end_media(&mut b);
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
        ("video" | "audio" | "source", _) => media_element(b, name, closing, attrs),
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

/// `<video>`, `<audio>` and their `<source>`s.
fn media_element(b: &mut Builder, name: &str, closing: bool, attrs: &str) {
    match (name, closing) {
        ("source", false) => {
            let src = attr(attrs, "src").and_then(|s| join(&b.base, &s));
            let kind = attr(attrs, "type").unwrap_or_default().to_ascii_lowercase();
            let Some((m, src)) = b.media.as_mut().zip(src) else {
                return;
            };
            let playable = PLAYABLE.iter().any(|p| kind.contains(p)) || is_playable(&src);
            if m.src.is_none() || (playable && !m.src.as_deref().is_some_and(is_playable)) {
                m.src = Some(src);
            }
        }
        ("source", true) => {}
        (_, true) => end_media(b),
        (_, false) => {
            end_media(b);
            b.flush();
            let kind = if name == "video" {
                MediaKind::Video
            } else {
                MediaKind::Audio
            };
            let label = attr(attrs, "title")
                .or_else(|| attr(attrs, "aria-label"))
                .unwrap_or_default();
            let src = attr(attrs, "src").and_then(|s| join(&b.base, &s));
            b.media = Some(PendingMedia { kind, src, label });
        }
    }
}

/// The `<video>`/`<audio>` being read becomes a block (when it named a file).
fn end_media(b: &mut Builder) {
    if let Some(m) = b.media.take() {
        if let Some(src) = m.src {
            if b.doc.blocks.len() < MAX_BLOCKS {
                b.doc.blocks.push(Block::media(m.kind, &src, &m.label));
            }
        }
    }
}

fn is_playable(src: &str) -> bool {
    let lower = src.to_ascii_lowercase();
    PLAYABLE.iter().any(|p| lower.contains(p))
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
#[path = "html_tests.rs"]
mod tests;
