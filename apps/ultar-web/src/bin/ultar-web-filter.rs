//! Übersetzt Adblock-Listen (EasyList/EasyPrivacy, Adblock-Plus-Syntax) in WebKit-Content-Blocker-Regeln (JSON).
//! WebKit kompiliert diese Regeln einmalig zu einem Automaten – geblockte Anfragen werden gar nicht erst
//! gestellt (schneller, weniger Daten), ganz ohne Code im Seitenprozess.
//!
//! Aufruf: ultar-web-filter <ausgabe.json> <liste1.txt> [liste2.txt …]   -> schreibt auch <ausgabe>.count

use std::fmt::Write as _;

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

const SEP: &str = "[^a-z0-9_.%-]";

/// Adblock-Muster -> regulärer Ausdruck im von WebKit unterstützten Umfang
fn to_regex(p: &str) -> Option<String> {
    let mut s = p;
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
    if s.is_empty() || s == "*" || s.len() < 3 {
        return None;
    }
    for c in s.chars() {
        match c {
            '*' => out.push_str(".*"),
            '^' => out.push_str(SEP),
            // '|' mitten im Muster ist ein gewöhnliches Zeichen (WebKit würde es als Alternative lesen)
            '.' | '+' | '?' | '(' | ')' | '{' | '}' | '[' | ']' | '\\' | '$' | '|' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_ascii() => out.push(c.to_ascii_lowercase()),
            _ => return None, // WebKit akzeptiert nur ASCII in url-filter
        }
    }
    if end_anchor {
        out.push('$');
    }
    // WebKit kennt keine Alternativen (|): ein Trennzeichen am Ende bleibt eine Zeichenklasse.
    // Adressen haben nach dem Host immer mindestens "/", daher trifft ||host^ trotzdem.
    Some(out)
}

struct Rule {
    filter: String,
    types: Vec<&'static str>,
    load: Option<&'static str>,
    if_dom: Vec<String>,
    unless_dom: Vec<String>,
}

fn net_rule(line: &str) -> Option<Rule> {
    let (pat, opts) = match line.rsplit_once('$') {
        Some((p, o)) if !o.contains('/') => (p, o),
        _ => (line, ""),
    };
    if pat.starts_with('/') && pat.ends_with('/') && pat.len() > 2 {
        return None; // eigene reguläre Ausdrücke der Liste: oft außerhalb des WebKit-Umfangs
    }
    let mut r = Rule { filter: to_regex(pat)?, types: Vec::new(), load: None, if_dom: Vec::new(), unless_dom: Vec::new() };
    for o in opts.split(',').filter(|o| !o.is_empty()) {
        match o {
            "third-party" | "3p" => r.load = Some("third-party"),
            "~third-party" | "1p" | "first-party" => r.load = Some("first-party"),
            "script" => r.types.push("script"),
            "image" => r.types.push("image"),
            "stylesheet" | "css" => r.types.push("style-sheet"),
            "font" => r.types.push("font"),
            "media" => r.types.push("media"),
            "xmlhttprequest" | "xhr" | "ping" | "websocket" => r.types.push("raw"),
            "subdocument" | "frame" | "document" | "doc" => r.types.push("document"),
            "match-case" | "important" | "other" => {}
            o if o.starts_with('~') => {} // Ausschluss von Typen: Regel gilt dann breiter – vertretbar
            o if o.starts_with("domain=") || o.starts_with("from=") => {
                for d in o.split_once('=').map(|x| x.1).unwrap_or("").split('|') {
                    if let Some(n) = d.strip_prefix('~') {
                        r.unless_dom.push(format!("*{}", n.to_ascii_lowercase()));
                    } else if !d.is_empty() {
                        r.if_dom.push(format!("*{}", d.to_ascii_lowercase()));
                    }
                }
            }
            _ => return None, // redirect, csp, removeparam, popup … nicht übersetzbar
        }
    }
    if !r.if_dom.is_empty() && !r.unless_dom.is_empty() {
        r.unless_dom.clear(); // WebKit erlaubt nicht beides zugleich
    }
    if r.if_dom.iter().chain(&r.unless_dom).any(|d| !d.is_ascii()) {
        return None;
    }
    r.types.sort();
    r.types.dedup();
    Some(r)
}

fn emit(out: &mut String, r: &Rule, action: &str, n: &mut usize) {
    if *n > 0 {
        out.push_str(",\n");
    }
    *n += 1;
    let _ = write!(out, "{{\"trigger\":{{\"url-filter\":\"{}\"", esc_json(&r.filter));
    if !r.types.is_empty() {
        let t: Vec<String> = r.types.iter().map(|t| format!("\"{t}\"")).collect();
        let _ = write!(out, ",\"resource-type\":[{}]", t.join(","));
    }
    if let Some(l) = r.load {
        let _ = write!(out, ",\"load-type\":[\"{l}\"]");
    }
    for (k, v) in [("if-domain", &r.if_dom), ("unless-domain", &r.unless_dom)] {
        if !v.is_empty() {
            let d: Vec<String> = v.iter().map(|d| format!("\"{}\"", esc_json(d))).collect();
            let _ = write!(out, ",\"{k}\":[{}]", d.join(","));
        }
    }
    let _ = write!(out, "}},\"action\":{{\"type\":\"{action}\"}}}}");
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 2 {
        eprintln!("Aufruf: ultar-web-filter <ausgabe.json> <liste.txt> …");
        std::process::exit(2);
    }
    let mut block = Vec::new();
    let mut allow = Vec::new();
    let mut generic_css: Vec<String> = Vec::new();
    let mut dom_css: Vec<(Vec<String>, String)> = Vec::new();
    let mut skipped = 0usize;
    for f in &a[1..] {
        let s = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("{f}: {e}"));
        for line in s.lines().map(str::trim) {
            if line.is_empty() || line.starts_with('!') || line.starts_with('[') {
                continue;
            }
            if ["#@#", "#?#", "#$#", "#%#", "##+js", "##^"].iter().any(|m| line.contains(m)) {
                skipped += 1;
                continue;
            }
            if let Some((doms, sel)) = line.split_once("##") {
                if sel.is_empty() || [":has(", ":-abp", ":style(", ":xpath(", ":upward(", ":remove("].iter().any(|m| sel.contains(m)) || !sel.is_ascii() || !doms.is_ascii() {
                    skipped += 1;
                    continue;
                }
                if doms.is_empty() {
                    generic_css.push(sel.to_string());
                } else {
                    let d: Vec<String> = doms.split(',').filter(|d| !d.starts_with('~') && !d.is_empty()).map(|d| format!("*{}", d.to_ascii_lowercase())).collect();
                    if !d.is_empty() {
                        dom_css.push((d, sel.to_string()));
                    }
                }
                continue;
            }
            if let Some(ex) = line.strip_prefix("@@") {
                match net_rule(ex) {
                    Some(r) => allow.push(r),
                    None => skipped += 1,
                }
                continue;
            }
            match net_rule(line) {
                Some(r) => block.push(r),
                None => skipped += 1,
            }
        }
    }
    let mut out = String::from("[\n");
    let mut n = 0usize;
    generic_css.sort();
    generic_css.dedup();
    for chunk in generic_css.chunks(500) {
        if n > 0 {
            out.push_str(",\n");
        }
        n += 1;
        let _ = write!(out, "{{\"trigger\":{{\"url-filter\":\".*\"}},\"action\":{{\"type\":\"css-display-none\",\"selector\":\"{}\"}}}}", esc_json(&chunk.join(", ")));
    }
    for (d, sel) in &dom_css {
        if n > 0 {
            out.push_str(",\n");
        }
        n += 1;
        let ds: Vec<String> = d.iter().map(|x| format!("\"{}\"", esc_json(x))).collect();
        let _ = write!(out, "{{\"trigger\":{{\"url-filter\":\".*\",\"if-domain\":[{}]}},\"action\":{{\"type\":\"css-display-none\",\"selector\":\"{}\"}}}}", ds.join(","), esc_json(sel));
    }
    for r in &block {
        emit(&mut out, r, "block", &mut n);
    }
    // Ausnahmen zuletzt: heben vorherige Regeln auf
    for r in &allow {
        emit(&mut out, r, "ignore-previous-rules", &mut n);
    }
    out.push_str("\n]\n");
    std::fs::write(&a[0], out).expect("schreiben");
    std::fs::write(format!("{}.count", a[0].trim_end_matches(".json")), n.to_string()).ok();
    eprintln!("ultar-web-filter: {n} Regeln ({} blockieren, {} Ausnahmen, {} CSS), {skipped} nicht übersetzbar", block.len(), allow.len(), generic_css.len() + dom_css.len());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn muster() {
        assert_eq!(to_regex("||ads.example.com^").unwrap(), "^[a-z][a-z0-9+.-]*://([^/]*\\.)?ads\\.example\\.com[^a-z0-9_.%-]");
        assert!(net_rule("||x.com^$third-party,script").unwrap().types == vec!["script"]);
        assert!(net_rule("||x.com^$redirect=noop").is_none());
    }
}
