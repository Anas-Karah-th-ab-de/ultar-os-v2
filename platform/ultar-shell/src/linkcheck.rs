//! Link-Prüfung für den Helfer: „Ist dieser Link sicher?"
//!
//! Zwei Stufen:
//! 1. `analyze` – reine Betrachtung der Adresse (kein Netz): Verschlüsselung, IP statt Name,
//!    nachgeahmte Markennamen, Kurzlink-Dienste, verdächtige Endungen, Zugangsdaten in der Adresse …
//! 2. `probe` – der Link wird in einer **abgeschotteten Box** verfolgt: eigener Prozess ohne Rechte
//!    (Benutzer nobody, kein Schreiben, wenig Speicher, wenige Sekunden), der nur die Kopfzeilen holt,
//!    Weiterleitungen folgt und nichts ausführt oder speichert. So zeigt sich, wohin ein Kurzlink wirklich führt.
//!
//! Das Ergebnis ist eine Einschätzung, keine Garantie.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Verdict {
    /// keine Auffälligkeiten
    Ok,
    /// Vorsicht
    Warn,
    /// wahrscheinlich gefährlich
    Bad,
}

#[derive(Clone, Debug)]
pub struct Report {
    pub url: String,
    pub host: String,
    pub verdict: Verdict,
    /// Gründe (englische Schlüssel für die Übersetzung) mit optionalem Zusatz
    pub reasons: Vec<(&'static str, String)>,
}

/// Zerlegte Adresse
#[derive(Clone, Debug, PartialEq)]
pub struct Parts {
    pub scheme: String,
    pub userinfo: bool,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
}

pub fn parse(url: &str) -> Option<Parts> {
    let u = url.trim();
    let (scheme, rest) = match u.find("://") {
        Some(i) => (u[..i].to_ascii_lowercase(), &u[i + 3..]),
        None => ("http".to_string(), u),
    };
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (auth, path) = (&rest[..end], &rest[end..]);
    let (userinfo, hostport) = match auth.rfind('@') {
        Some(i) => (true, &auth[i + 1..]),
        None => (false, auth),
    };
    let (host, port) = if let Some(h) = hostport.strip_prefix('[') {
        let e = h.find(']')?;
        (
            h[..e].to_string(),
            h[e + 1..].strip_prefix(':').and_then(|p| p.parse().ok()),
        )
    } else {
        match hostport.rfind(':') {
            Some(i)
                if hostport[i + 1..].chars().all(|c| c.is_ascii_digit())
                    && i + 1 < hostport.len() =>
            {
                (hostport[..i].to_string(), hostport[i + 1..].parse().ok())
            }
            _ => (hostport.to_string(), None),
        }
    };
    let host = host.trim_end_matches('.').to_lowercase();
    if host.is_empty() || !host.contains('.') && host != "localhost" && !host.contains(':') {
        return None;
    }
    Some(Parts {
        scheme,
        userinfo,
        host,
        port,
        path: path.to_string(),
    })
}

/// Adressen in einem Text finden (http/https/www. oder name.tld/…).
pub fn find_urls(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '<' | '>' | '"' | '\'' | '(' | ')' | '[' | ']' | '«' | '»'
            )
    }) {
        let w = raw.trim_end_matches(['.', ',', ';', ':', '!', '?', '…']);
        if w.len() < 5 {
            continue;
        }
        let lw = w.to_lowercase();
        let looks =
            lw.starts_with("http://") || lw.starts_with("https://") || lw.starts_with("www.") || {
                // nackte Domain mit Pfad, z. B. bit.ly/abc
                let host = lw.split('/').next().unwrap_or("");
                lw.contains('/')
                    && host.contains('.')
                    && host.rsplit('.').next().is_some_and(|t| {
                        (2..=10).contains(&t.len()) && t.chars().all(|c| c.is_ascii_alphabetic())
                    })
                    && !host.contains('@')
            };
        if looks && parse(w).is_some() && !out.iter().any(|o: &String| o == w) {
            out.push(w.to_string());
        }
    }
    out
}

const SHORTENERS: [&str; 22] = [
    "bit.ly",
    "t.co",
    "tinyurl.com",
    "goo.gl",
    "ow.ly",
    "is.gd",
    "buff.ly",
    "cutt.ly",
    "rb.gy",
    "rebrand.ly",
    "shorturl.at",
    "t.ly",
    "tiny.cc",
    "lnkd.in",
    "s.id",
    "v.gd",
    "soo.gd",
    "bl.ink",
    "shorte.st",
    "adf.ly",
    "qr.ae",
    "u.to",
];
/// Endungen, die auffällig oft für Betrug benutzt werden (billig/kostenlos)
const RISKY_TLDS: [&str; 22] = [
    "zip", "mov", "tk", "ml", "ga", "cf", "gq", "top", "xyz", "click", "link", "work", "country",
    "kim", "men", "loan", "date", "racing", "win", "download", "review", "cam",
];
/// Marken, die gern nachgeahmt werden: (Wort im Namen, echte Domain-Enden)
const BRANDS: [(&str, &[&str]); 16] = [
    ("paypal", &["paypal.com", "paypal.me"]),
    ("whatsapp", &["whatsapp.com", "whatsapp.net", "wa.me"]),
    ("telegram", &["telegram.org", "t.me", "telegram.me"]),
    (
        "google",
        &[
            "google.com",
            "google.de",
            "googleapis.com",
            "goo.gl",
            "google.ae",
            "google.com.sa",
        ],
    ),
    ("apple", &["apple.com", "icloud.com"]),
    ("icloud", &["icloud.com", "apple.com"]),
    (
        "microsoft",
        &[
            "microsoft.com",
            "live.com",
            "office.com",
            "microsoftonline.com",
        ],
    ),
    (
        "amazon",
        &[
            "amazon.com",
            "amazon.de",
            "amazon.ae",
            "amazon.sa",
            "amzn.to",
            "amazonaws.com",
        ],
    ),
    ("facebook", &["facebook.com", "fb.com", "fb.me", "meta.com"]),
    ("instagram", &["instagram.com"]),
    ("netflix", &["netflix.com"]),
    ("dhl", &["dhl.com", "dhl.de"]),
    ("binance", &["binance.com"]),
    ("syriatel", &["syriatel.sy", "syriatel.com.sy"]),
    ("mtn", &["mtn.com.sy", "mtn.com", "mtnsyr.com"]),
    (
        "github",
        &["github.com", "github.io", "githubusercontent.com"],
    ),
];
const BAIT: [&str; 14] = [
    "login", "signin", "verify", "verif", "secure", "account", "update", "confirm", "password",
    "wallet", "bonus", "gift", "free", "prize",
];
const RISKY_EXT: [&str; 12] = [
    ".apk", ".exe", ".scr", ".bat", ".cmd", ".msi", ".js", ".vbs", ".jar", ".dmg", ".ps1", ".lnk",
];

/// Registrierbare Domain grob bestimmen (letzte zwei Teile, bei co.uk/com.sy u. Ä. drei).
pub fn registrable(host: &str) -> String {
    let p: Vec<&str> = host.split('.').collect();
    if p.len() <= 2 {
        return host.to_string();
    }
    let second = p[p.len() - 2];
    let n = if p[p.len() - 1].len() == 2
        && matches!(second, "co" | "com" | "net" | "org" | "gov" | "edu" | "ac")
    {
        3
    } else {
        2
    };
    p[p.len().saturating_sub(n)..].join(".")
}

fn is_ip(host: &str) -> bool {
    host.contains(':')
        || (host.split('.').count() == 4 && host.split('.').all(|p| p.parse::<u8>().is_ok()))
        || (host.chars().all(|c| c.is_ascii_digit()) && !host.is_empty())
}

/// Adresse ohne Netz beurteilen.
pub fn analyze(url: &str) -> Report {
    let mut r = Report {
        url: url.to_string(),
        host: String::new(),
        verdict: Verdict::Ok,
        reasons: Vec::new(),
    };
    let Some(p) = parse(url) else {
        r.verdict = Verdict::Warn;
        r.reasons.push(("Not a valid web address", String::new()));
        return r;
    };
    r.host = p.host.clone();
    let mut score = 0;
    let add = |score: &mut i32, pts: i32, why: &'static str, extra: String, r: &mut Report| {
        *score += pts;
        r.reasons.push((why, extra));
    };
    let reg = registrable(&p.host);
    match p.scheme.as_str() {
        "https" => {}
        "http" => add(&mut score, 2, "Not encrypted (http)", String::new(), &mut r),
        other => add(
            &mut score,
            5,
            "Unusual link type",
            other.to_string(),
            &mut r,
        ),
    }
    if p.userinfo {
        add(
            &mut score,
            7,
            "Hides the real address behind an @",
            String::new(),
            &mut r,
        );
    }
    if is_ip(&p.host) {
        add(
            &mut score,
            5,
            "Numeric address instead of a name",
            p.host.clone(),
            &mut r,
        );
    }
    if p.host.split('.').any(|l| l.starts_with("xn--")) || !p.host.is_ascii() {
        add(
            &mut score,
            5,
            "Look-alike letters in the name",
            String::new(),
            &mut r,
        );
    }
    if let Some(port) = p.port {
        if !matches!(port, 80 | 443) {
            add(&mut score, 2, "Unusual port", port.to_string(), &mut r);
        }
    }
    let tld = p.host.rsplit('.').next().unwrap_or("");
    if RISKY_TLDS.contains(&tld) {
        add(
            &mut score,
            3,
            "Ending often used for scams",
            format!(".{tld}"),
            &mut r,
        );
    }
    if SHORTENERS.contains(&reg.as_str()) || SHORTENERS.contains(&p.host.as_str()) {
        add(
            &mut score,
            2,
            "Short link – the real target is hidden",
            reg.clone(),
            &mut r,
        );
    }
    if p.host.split('.').count() >= 5 {
        add(
            &mut score,
            2,
            "Very long chain of sub-names",
            String::new(),
            &mut r,
        );
    }
    let squashed: String = p
        .host
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => 'l',
            '3' => 'e',
            '5' => 's',
            _ => c,
        })
        .filter(|c| *c != '-')
        .collect();
    for (brand, real) in BRANDS {
        let genuine = real
            .iter()
            .any(|d| reg == *d || p.host == *d || p.host.ends_with(&format!(".{d}")));
        if !genuine && (p.host.contains(brand) || squashed.contains(brand)) {
            add(
                &mut score,
                7,
                "Imitates a known brand",
                format!("{brand} ≠ {reg}"),
                &mut r,
            );
            break;
        }
    }
    let lp = p.path.to_lowercase();
    if let Some(ext) = RISKY_EXT
        .iter()
        .find(|e| lp.split(['?', '#']).next().unwrap_or("").ends_with(**e))
    {
        add(
            &mut score,
            4,
            "Downloads a program file",
            ext.to_string(),
            &mut r,
        );
    }
    let bait = BAIT
        .iter()
        .filter(|b| p.host.contains(**b) || lp.contains(**b))
        .count();
    if bait >= 1 && score >= 2 {
        add(
            &mut score,
            2,
            "Bait words in the address",
            String::new(),
            &mut r,
        );
    }
    if p.host.matches('-').count() >= 3 {
        add(
            &mut score,
            1,
            "Many hyphens in the name",
            String::new(),
            &mut r,
        );
    }
    if url.len() > 200 || lp.matches('%').count() > 12 {
        add(
            &mut score,
            1,
            "Very long or disguised address",
            String::new(),
            &mut r,
        );
    }
    r.verdict = if score >= 7 {
        Verdict::Bad
    } else if score >= 2 {
        Verdict::Warn
    } else {
        Verdict::Ok
    };
    r
}

/// Ergebnis der Verfolgung in der Box
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trace {
    /// besuchte Stationen (Adresse, HTTP-Status oder 0)
    pub hops: Vec<(String, u16)>,
    /// Inhaltstyp der letzten Station, falls bekannt
    pub ctype: String,
    pub error: String,
}

/// Läuft im abgeschotteten Kindprozess (`ultar-shell --linkprobe <url>`): folgt Weiterleitungen über
/// unverschlüsseltes HTTP (nur Kopfzeilen), gibt je Station eine Zeile aus. Bei https endet die Spur
/// an dieser Station – der Name der Zielseite ist damit bekannt, ohne sie zu laden.
pub fn probe_child(url: &str) -> i32 {
    sandbox();
    let mut cur = url.to_string();
    for _ in 0..6 {
        let Some(p) = parse(&cur) else {
            println!("ERR invalid");
            return 1;
        };
        if p.scheme != "http" {
            println!("HOP 0 {cur}");
            return 0;
        }
        let port = p.port.unwrap_or(80);
        let addr = match (p.host.as_str(), port).to_socket_addrs() {
            Ok(mut a) => a.next(),
            Err(e) => {
                eprintln!("dns: {e}");
                None
            }
        };
        let Some(addr) = addr else {
            println!("HOP 0 {cur}");
            println!("ERR dns");
            return 1;
        };
        // nie ins eigene Netz oder aufs Gerät selbst greifen
        let private = match addr.ip() {
            std::net::IpAddr::V4(v) => {
                v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified()
            }
            std::net::IpAddr::V6(v) => v.is_loopback() || v.is_unspecified(),
        };
        if private {
            println!("HOP 0 {cur}");
            println!("ERR private");
            return 1;
        }
        let Ok(mut s) = TcpStream::connect_timeout(&addr, Duration::from_secs(4)) else {
            println!("HOP 0 {cur}");
            println!("ERR connect");
            return 1;
        };
        let _ = s.set_read_timeout(Some(Duration::from_secs(4)));
        let _ = s.set_write_timeout(Some(Duration::from_secs(4)));
        let path = if p.path.is_empty() {
            "/".to_string()
        } else {
            p.path.split('#').next().unwrap_or("/").to_string()
        };
        let req = format!("HEAD {path} HTTP/1.1\r\nHost: {}\r\nUser-Agent: Mozilla/5.0 (Linux; Android 11) Mobile\r\nAccept: */*\r\nConnection: close\r\n\r\n", p.host);
        if s.write_all(req.as_bytes()).is_err() {
            println!("ERR send");
            return 1;
        }
        let mut buf = vec![0u8; 8192];
        let mut n = 0;
        while n < buf.len() {
            match s.read(&mut buf[n..]) {
                Ok(0) | Err(_) => break,
                Ok(k) => {
                    n += k;
                    if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                        break;
                    }
                }
            }
        }
        let head = String::from_utf8_lossy(&buf[..n]).to_string();
        let status: u16 = head
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        println!("HOP {status} {cur}");
        let hdr = |name: &str| {
            head.lines().find_map(|l| {
                l.split_once(':')
                    .filter(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.trim().to_string())
            })
        };
        if (300..400).contains(&status) {
            if let Some(loc) = hdr("location") {
                cur = if loc.contains("://") {
                    loc
                } else if loc.starts_with('/') {
                    format!("http://{}{loc}", p.host)
                } else {
                    format!("http://{}/{loc}", p.host)
                };
                continue;
            }
        }
        if let Some(ct) = hdr("content-type") {
            println!("TYPE {ct}");
        }
        return 0;
    }
    println!("ERR loop");
    1
}

/// Die „Box": Rechte abgeben und Grenzen setzen, bevor irgendetwas aus dem Netz gelesen wird.
fn sandbox() {
    use nix::libc;
    // SAFETY: einfache Systemaufrufe im frisch gestarteten Kindprozess, keine geteilten Daten.
    unsafe {
        let lim = |res, v: u64| {
            let r = libc::rlimit {
                rlim_cur: v,
                rlim_max: v,
            };
            libc::setrlimit(res, &r);
        };
        lim(libc::RLIMIT_CPU, 4);
        lim(libc::RLIMIT_FSIZE, 0); // keine Datei schreiben
        lim(libc::RLIMIT_NOFILE, 24);
        lim(libc::RLIMIT_NPROC, 0); // keine weiteren Prozesse
        lim(libc::RLIMIT_AS, 256 << 20);
        libc::alarm(14); // harte Zeitgrenze
                         // Samsung-/Android-Kerne erlauben Netzwerk-Sockets nur der Gruppe 3003 (inet) – das ist das einzige Recht, das bleibt
        let inet: [libc::gid_t; 1] = [3003];
        libc::setgroups(1, inet.as_ptr());
        libc::setgid(65534);
        libc::setuid(65534);
        libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0);
    }
}

/// Link in der Box verfolgen (blockiert bis ~15 s; im Hintergrund aufrufen).
pub fn probe(url: &str) -> Trace {
    let mut t = Trace::default();
    let exe = std::env::current_exe().unwrap_or_else(|_| "/usr/bin/ultar-shell".into());
    let out = std::process::Command::new(exe)
        .arg("--linkprobe")
        .arg(url)
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) => parse_trace(&String::from_utf8_lossy(&o.stdout), &mut t),
        Err(e) => t.error = e.to_string(),
    }
    t
}

fn parse_trace(s: &str, t: &mut Trace) {
    for l in s.lines() {
        if let Some(r) = l.strip_prefix("HOP ") {
            if let Some((st, u)) = r.split_once(' ') {
                t.hops.push((u.to_string(), st.parse().unwrap_or(0)));
            }
        } else if let Some(r) = l.strip_prefix("TYPE ") {
            t.ctype = r.to_string();
        } else if let Some(r) = l.strip_prefix("ERR ") {
            t.error = r.to_string();
        }
    }
}

/// Gesamturteil: Adresse selbst + das Ziel, das die Box gefunden hat.
pub fn judge(url: &str, trace: Option<&Trace>) -> Report {
    let mut r = analyze(url);
    let Some(t) = trace else { return r };
    if let Some((last, _)) = t.hops.last() {
        if last != url && t.hops.len() > 1 {
            let f = analyze(last);
            r.reasons.push(("Leads to", f.host.clone()));
            for x in f.reasons {
                if !r.reasons.iter().any(|y| y.0 == x.0) {
                    r.reasons.push(x);
                }
            }
            // das Ziel entscheidet; ein Kurzlink auf eine unauffällige https-Seite ist nur noch ein Hinweis
            r.verdict = match (r.verdict, f.verdict) {
                (_, Verdict::Bad) | (Verdict::Bad, _) => Verdict::Bad,
                (_, Verdict::Warn) => Verdict::Warn,
                (v, Verdict::Ok) => {
                    let only_short = r.reasons.iter().all(|x| {
                        matches!(
                            x.0,
                            "Short link – the real target is hidden"
                                | "Not encrypted (http)"
                                | "Leads to"
                        )
                    });
                    if only_short {
                        Verdict::Ok
                    } else {
                        v
                    }
                }
            };
        }
    }
    let ct = t.ctype.to_lowercase();
    if ct.contains("application/vnd.android.package-archive")
        || ct.contains("application/x-msdownload")
        || ct.contains("application/octet-stream")
    {
        r.reasons
            .push(("Downloads a program file", t.ctype.clone()));
        r.verdict = Verdict::Bad;
    }
    match t.error.as_str() {
        "dns" => r
            .reasons
            .push(("The address does not exist", String::new())),
        "private" => {
            r.reasons
                .push(("Points into the local network", String::new()));
            if r.verdict == Verdict::Ok {
                r.verdict = Verdict::Warn;
            }
        }
        _ => {}
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zerlegen() {
        let p = parse("https://user:pw@Sub.Example.com:8443/a/b?x=1#f").unwrap();
        assert_eq!(
            (
                p.scheme.as_str(),
                p.userinfo,
                p.host.as_str(),
                p.port,
                p.path.as_str()
            ),
            ("https", true, "sub.example.com", Some(8443), "/a/b?x=1#f")
        );
        assert_eq!(parse("bit.ly/abc").unwrap().scheme, "http");
        assert!(parse("hallo").is_none());
        assert_eq!(registrable("a.b.example.co.uk"), "example.co.uk");
        assert_eq!(registrable("mail.google.com"), "google.com");
    }

    #[test]
    fn findet_adressen_im_text() {
        let t = "Schau mal: https://example.com/x, oder www.test.de. Und bit.ly/3abc! Kein Link: 3.5/10 und max@mail.de";
        assert_eq!(
            find_urls(t),
            vec!["https://example.com/x", "www.test.de", "bit.ly/3abc"]
        );
    }

    #[test]
    fn unauffaellige_adresse() {
        let r = analyze("https://www.wikipedia.org/wiki/Damascus");
        assert_eq!(r.verdict, Verdict::Ok, "{:?}", r.reasons);
        assert_eq!(
            analyze("https://accounts.google.com/signin").verdict,
            Verdict::Ok
        );
    }

    #[test]
    fn erkennt_betrugsmuster() {
        for u in [
            "http://paypal.com.secure-login.xyz/verify",
            "https://paypa1-account.com/login",
            "http://192.168.4.20/update.apk",
            "https://google.com@evil.example.net/",
            "http://xn--pple-43d.com/",
            "https://whatsapp-gift.top/free",
        ] {
            assert_eq!(
                analyze(u).verdict,
                Verdict::Bad,
                "{u}: {:?}",
                analyze(u).reasons
            );
        }
        assert_eq!(analyze("http://bit.ly/abc").verdict, Verdict::Warn);
        assert_eq!(analyze("http://example.com/").verdict, Verdict::Warn);
    }

    #[test]
    fn urteil_mit_spur() {
        // Kurzlink führt auf unauffällige https-Seite → in Ordnung
        let mut t = Trace::default();
        parse_trace(
            "HOP 301 http://bit.ly/abc\nHOP 0 https://www.wikipedia.org/\n",
            &mut t,
        );
        assert_eq!(t.hops.len(), 2);
        let r = judge("http://bit.ly/abc", Some(&t));
        assert_eq!(r.verdict, Verdict::Ok, "{:?}", r.reasons);
        assert!(r
            .reasons
            .iter()
            .any(|x| x.0 == "Leads to" && x.1 == "www.wikipedia.org"));
        // Kurzlink führt auf Nachahmung → gefährlich
        let mut t = Trace::default();
        parse_trace(
            "HOP 302 http://bit.ly/zzz\nHOP 0 https://paypal.com.verify-account.top/login\n",
            &mut t,
        );
        assert_eq!(judge("http://bit.ly/zzz", Some(&t)).verdict, Verdict::Bad);
        // Programmdatei
        let mut t = Trace::default();
        parse_trace(
            "HOP 200 http://example.org/app\nTYPE application/vnd.android.package-archive\n",
            &mut t,
        );
        assert_eq!(
            judge("http://example.org/app", Some(&t)).verdict,
            Verdict::Bad
        );
    }
}
