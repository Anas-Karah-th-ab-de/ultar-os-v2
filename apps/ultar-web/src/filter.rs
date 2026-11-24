//! Tracking protection: ad-block lists (EasyList/EasyPrivacy, Adblock Plus syntax) compiled
//! to WebKit content-blocker rules (JSON). WebKit turns them into an automaton once; blocked
//! requests are never made and no code runs in the page process. Rules the engine cannot
//! express (scriptlets, redirects, `:has()` …) are counted and skipped, never guessed.

use std::fmt::Write as _;

/// Most rules written (WebKit compiles larger lists only slowly).
pub const MAX_RULES: usize = 150_000;
/// Longest list line considered (bytes).
pub const MAX_LINE: usize = 4096;
/// Selectors per generic hiding rule.
const CSS_CHUNK: usize = 500;
/// Separator class of the `^` placeholder.
const SEP: &str = "[^a-z0-9_.%-]";

/// The result of compiling lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Compiled {
    /// The content-blocker JSON.
    pub json: String,
    /// Rules in it.
    pub rules: usize,
    /// List lines that could not be translated.
    pub skipped: usize,
}

fn esc_json(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(o, "\\u{:04x}", c as u32);
            }
            c => o.push(c),
        }
    }
    o
}

/// An Adblock pattern as a regular expression in the subset WebKit supports.
pub fn to_regex(pattern: &str) -> Option<String> {
    let mut s = pattern;
    let mut out = String::new();
    if let Some(r) = s.strip_prefix("||") {
        out.push_str("^[a-z][a-z0-9+.-]*://([^/]*\\.)?");
        s = r;
    } else if let Some(r) = s.strip_prefix('|') {
        out.push('^');
        s = r;
    }
    let end_anchor = s.ends_with('|');
    let s = s.strip_suffix('|').unwrap_or(s);
    if s.len() < 3 || s == "*" {
        return None;
    }
    for c in s.chars() {
        match c {
            '*' => out.push_str(".*"),
            '^' => out.push_str(SEP),
            '.' | '+' | '?' | '(' | ')' | '{' | '}' | '[' | ']' | '\\' | '$' | '|' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii() => out.push(c.to_ascii_lowercase()),
            _ => return None,
        }
    }
    if end_anchor {
        out.push('$');
    }
    Some(out)
}

/// One network rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// URL regular expression.
    pub filter: String,
    /// Resource types (empty = all).
    pub types: Vec<&'static str>,
    /// First- or third-party only.
    pub load: Option<&'static str>,
    /// Only on these domains.
    pub if_domain: Vec<String>,
    /// Not on these domains.
    pub unless_domain: Vec<String>,
}

fn resource_type(option: &str) -> Option<Option<&'static str>> {
    Some(Some(match option {
        "script" => "script",
        "image" => "image",
        "stylesheet" | "css" => "style-sheet",
        "font" => "font",
        "media" => "media",
        "xmlhttprequest" | "xhr" | "ping" | "websocket" => "raw",
        "subdocument" | "frame" | "document" | "doc" => "document",
        "match-case" | "important" | "other" => return Some(None),
        _ => return None,
    }))
}

fn apply_option(r: &mut Rule, option: &str) -> Option<()> {
    match option {
        "third-party" | "3p" => r.load = Some("third-party"),
        "~third-party" | "1p" | "first-party" => r.load = Some("first-party"),
        o if o.starts_with('~') => {}
        o if o.starts_with("domain=") || o.starts_with("from=") => {
            for d in o.split_once('=').map_or("", |x| x.1).split('|') {
                if let Some(n) = d.strip_prefix('~') {
                    r.unless_domain.push(format!("*{}", n.to_ascii_lowercase()));
                } else if !d.is_empty() {
                    r.if_domain.push(format!("*{}", d.to_ascii_lowercase()));
                }
            }
        }
        o => {
            if let Some(t) = resource_type(o)? {
                r.types.push(t);
            }
        }
    }
    Some(())
}

/// A network rule from one list line (without `@@`).
pub fn net_rule(line: &str) -> Option<Rule> {
    let (pattern, options) = match line.rsplit_once('$') {
        Some((p, o)) if !o.contains('/') => (p, o),
        _ => (line, ""),
    };
    if pattern.starts_with('/') && pattern.ends_with('/') && pattern.len() > 2 {
        return None;
    }
    let mut r = Rule {
        filter: to_regex(pattern)?,
        types: Vec::new(),
        load: None,
        if_domain: Vec::new(),
        unless_domain: Vec::new(),
    };
    for option in options.split(',').filter(|o| !o.is_empty()) {
        apply_option(&mut r, option)?;
    }
    if !r.if_domain.is_empty() {
        r.unless_domain.clear();
    }
    if r.if_domain
        .iter()
        .chain(&r.unless_domain)
        .any(|d| !d.is_ascii())
    {
        return None;
    }
    r.types.sort_unstable();
    r.types.dedup();
    Some(r)
}

fn emit(out: &mut Vec<String>, r: &Rule, action: &str) {
    let mut s = format!("{{\"trigger\":{{\"url-filter\":\"{}\"", esc_json(&r.filter));
    if !r.types.is_empty() {
        let t: Vec<String> = r.types.iter().map(|t| format!("\"{t}\"")).collect();
        let _ = write!(s, ",\"resource-type\":[{}]", t.join(","));
    }
    if let Some(l) = r.load {
        let _ = write!(s, ",\"load-type\":[\"{l}\"]");
    }
    for (k, v) in [
        ("if-domain", &r.if_domain),
        ("unless-domain", &r.unless_domain),
    ] {
        if !v.is_empty() {
            let d: Vec<String> = v.iter().map(|d| format!("\"{}\"", esc_json(d))).collect();
            let _ = write!(s, ",\"{k}\":[{}]", d.join(","));
        }
    }
    let _ = write!(s, "}},\"action\":{{\"type\":\"{action}\"}}}}");
    out.push(s);
}

#[derive(Default)]
struct Parsed {
    block: Vec<Rule>,
    allow: Vec<Rule>,
    generic_css: Vec<String>,
    domain_css: Vec<(Vec<String>, String)>,
    skipped: usize,
}

const UNSUPPORTED: [&str; 6] = ["#@#", "#?#", "#$#", "#%#", "##+js", "##^"];
const UNSUPPORTED_CSS: [&str; 6] = [
    ":has(", ":-abp", ":style(", ":xpath(", ":upward(", ":remove(",
];

fn hiding_rule(p: &mut Parsed, domains: &str, selector: &str) {
    let bad = selector.is_empty()
        || UNSUPPORTED_CSS.iter().any(|m| selector.contains(m))
        || !selector.is_ascii()
        || !domains.is_ascii();
    if bad {
        p.skipped += 1;
    } else if domains.is_empty() {
        p.generic_css.push(selector.to_string());
    } else {
        let d: Vec<String> = domains
            .split(',')
            .filter(|d| !d.starts_with('~') && !d.is_empty())
            .map(|d| format!("*{}", d.to_ascii_lowercase()))
            .collect();
        if !d.is_empty() {
            p.domain_css.push((d, selector.to_string()));
        }
    }
}

fn parse_line(p: &mut Parsed, line: &str) {
    if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
        return;
    }
    if line.len() > MAX_LINE || UNSUPPORTED.iter().any(|m| line.contains(m)) {
        p.skipped += 1;
        return;
    }
    if let Some((domains, selector)) = line.split_once("##") {
        hiding_rule(p, domains, selector);
        return;
    }
    let (list, text) = match line.strip_prefix("@@") {
        Some(ex) => (&mut p.allow, ex),
        None => (&mut p.block, line),
    };
    match net_rule(text) {
        Some(r) => list.push(r),
        None => p.skipped += 1,
    }
}

/// Compiles the text of one or more lists.
pub fn compile(lists: &[&str]) -> Compiled {
    let mut p = Parsed::default();
    for list in lists {
        for line in list.lines().map(str::trim) {
            parse_line(&mut p, line);
        }
    }
    let mut rules = Vec::new();
    p.generic_css.sort();
    p.generic_css.dedup();
    for chunk in p.generic_css.chunks(CSS_CHUNK) {
        rules.push(format!(
            "{{\"trigger\":{{\"url-filter\":\".*\"}},\"action\":{{\"type\":\"css-display-none\",\"selector\":\"{}\"}}}}",
            esc_json(&chunk.join(", "))
        ));
    }
    for (d, sel) in &p.domain_css {
        let ds: Vec<String> = d.iter().map(|x| format!("\"{}\"", esc_json(x))).collect();
        rules.push(format!(
            "{{\"trigger\":{{\"url-filter\":\".*\",\"if-domain\":[{}]}},\"action\":{{\"type\":\"css-display-none\",\"selector\":\"{}\"}}}}",
            ds.join(","),
            esc_json(sel)
        ));
    }
    for r in &p.block {
        emit(&mut rules, r, "block");
    }
    // Exceptions last: they lift the rules before them; they are never cut by the limit.
    let mut allow = Vec::new();
    for r in &p.allow {
        emit(&mut allow, r, "ignore-previous-rules");
    }
    let room = MAX_RULES.saturating_sub(allow.len());
    let skipped = p.skipped + rules.len().saturating_sub(room);
    rules.truncate(room);
    rules.extend(allow);
    Compiled {
        json: format!("[\n{}\n]\n", rules.join(",\n")),
        rules: rules.len(),
        skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_and_options() {
        assert_eq!(
            to_regex("||ads.example.com^").unwrap(),
            "^[a-z][a-z0-9+.-]*://([^/]*\\.)?ads\\.example\\.com[^a-z0-9_.%-]"
        );
        assert_eq!(to_regex("|https://x.y/a|").unwrap(), "^https://x\\.y/a$");
        assert!(to_regex("ab").is_none() && to_regex("*").is_none() && to_regex("äbc").is_none());
        let r = net_rule("||x.com^$third-party,script,css,domain=a.com|~b.com").unwrap();
        assert_eq!(r.types, vec!["script", "style-sheet"]);
        assert_eq!(r.load, Some("third-party"));
        assert_eq!(r.if_domain, vec!["*a.com"]);
        assert!(r.unless_domain.is_empty());
        assert!(net_rule("||x.com^$redirect=noop").is_none());
        assert!(net_rule("/ads[0-9]/").is_none());
        assert_eq!(
            net_rule("/banner/*$~image,1p").unwrap().load,
            Some("first-party")
        );
    }

    #[test]
    fn lists_compile_to_rules() {
        let list = "! comment\n[Adblock]\n||ads.example^\n@@||ads.example/ok^\n##.ad\nexample.com##.box\nx.com#@#.y\n##.a:has(b)\nexample.org,~foo.org##.z\n";
        let c = compile(&[list]);
        assert_eq!(c.rules, 5);
        assert_eq!(c.skipped, 2);
        assert!(c.json.contains("\"type\":\"block\""));
        assert!(c
            .json
            .trim_end()
            .ends_with("\"ignore-previous-rules\"}}\n]"));
        assert!(c.json.contains("\"if-domain\":[\"*example.org\"]"));
        let long = format!("||{}^\n", "a".repeat(MAX_LINE));
        assert_eq!(compile(&[&long]).skipped, 1);
        assert_eq!(esc_json("a\"\\\u{1}"), "a\\\"\\\\\\u0001");
    }

    #[test]
    fn the_rule_limit_keeps_the_exceptions() {
        let mut list = String::new();
        for i in 0..MAX_RULES + 10 {
            let _ = writeln!(list, "||host{i}.example^");
        }
        list.push_str("@@||good.example^\n");
        let c = compile(&[&list]);
        assert_eq!(c.rules, MAX_RULES);
        assert_eq!(c.skipped, 11);
        assert!(c.json.contains("good\\\\.example"));
    }
}
