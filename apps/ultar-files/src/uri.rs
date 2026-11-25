//! URIs Files opens and hands to other apps: `trash:///`, `files://<area>/<path>` and the
//! `file://` URIs of mounted drives (`volumes.state`). Every URI is hostile input.

use crate::fs::{AreaId, Loc};
use crate::model::Drive;

/// Longest URI accepted (the launcher's limit).
pub const MAX_URI: usize = 2048;

/// The `files://` URI of `loc` (path parts percent-encoded).
pub fn of_loc(loc: &Loc) -> String {
    let mut out = format!("files://{}/", area_text(&loc.area));
    let parts: Vec<String> = loc.parts.iter().map(|p| encode(p)).collect();
    out.push_str(&parts.join("/"));
    out
}

/// The shown name of the item a `files://` URI names (its last part, decoded).
pub fn name_of(uri: &str) -> String {
    let last = uri.trim_end_matches('/').rsplit('/').next().unwrap_or(uri);
    let raw = last.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        let hex = raw
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok());
        match (raw[i], hex.and_then(|h| u8::from_str_radix(h, 16).ok())) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode(part: &str) -> String {
    let mut out = Vec::with_capacity(part.len());
    for b in part.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) || b >= 0x80 {
            out.push(b);
        } else {
            out.extend_from_slice(format!("%{b:02X}").as_bytes());
        }
    }
    String::from_utf8(out).unwrap_or_default()
}

fn area_text(area: &AreaId) -> String {
    match area {
        AreaId::Drive(d) => format!("removable/{}", encode(d)),
        other => other.name(),
    }
}

fn decode(part: &str) -> Option<String> {
    let raw = part.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' {
            let hex = std::str::from_utf8(raw.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(raw[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The place a URI names: the trash, a `files://` place, or a folder of a mounted drive.
pub fn parse(uri: &str, drives: &[Drive]) -> Option<Loc> {
    if uri.len() > MAX_URI || uri.chars().any(char::is_control) {
        return None;
    }
    if uri == "trash:///" || uri == "trash:" {
        return Some(Loc::root(AreaId::Trash));
    }
    if let Some(rest) = uri.strip_prefix("files://") {
        let (area, path) = match rest.strip_prefix("removable/") {
            Some(r) => {
                let j = r.find('/').unwrap_or(r.len());
                (format!("removable/{}", decode(&r[..j])?), &r[j..])
            }
            None => {
                let j = rest.find('/').unwrap_or(rest.len());
                (rest[..j].to_owned(), &rest[j..])
            }
        };
        return walk(Loc::root(AreaId::parse(&area)?), path);
    }
    let drive = drives
        .iter()
        .find(|d| uri == d.uri || uri.strip_prefix(&d.uri).is_some_and(|r| r.starts_with('/')))?;
    walk(Loc::root(drive.area.clone()), &uri[drive.uri.len()..])
}

fn walk(mut loc: Loc, path: &str) -> Option<Loc> {
    for part in path.split('/').filter(|p| !p.is_empty()) {
        loc = loc.child(&decode(part)?).ok()?;
    }
    Some(loc)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_round_trip_through_uris() {
        let loc = Loc::root(AreaId::Documents)
            .child("عقود 2026")
            .unwrap()
            .child("a%b.txt")
            .unwrap();
        let uri = of_loc(&loc);
        assert_eq!(uri, "files://documents/عقود%202026/a%25b.txt");
        assert_eq!(parse(&uri, &[]), Some(loc));
        assert_eq!(parse("trash:///", &[]), Some(Loc::root(AreaId::Trash)));
        let d = Loc::root(AreaId::Drive("STICK".into())).child("x").unwrap();
        assert_eq!(parse(&of_loc(&d), &[]), Some(d));
    }

    #[test]
    fn drive_uris_and_hostile_input() {
        let drive = Drive {
            id: "sdb1".into(),
            name: "STICK".into(),
            uri: "file:///run/ultar/media/u/STICK".into(),
            area: AreaId::Drive("STICK".into()),
        };
        let ds = [drive.clone()];
        assert_eq!(parse(&drive.uri, &ds), Some(Loc::root(drive.area.clone())));
        assert_eq!(
            parse("file:///run/ultar/media/u/STICK/a", &ds)
                .unwrap()
                .parts,
            ["a"]
        );
        for bad in [
            "file:///run/ultar/media/u/STICKY",
            "file:///etc/passwd",
            "files://documents/../etc",
            "files://documents/%2e%2e",
            "files://documents/a%2Fb",
            "files://nowhere/a",
            "files://documents/%zz",
            "files://documents/a\nb",
        ] {
            assert_eq!(parse(bad, &ds), None, "{bad}");
        }
        assert_eq!(parse(&"a".repeat(MAX_URI + 1), &ds), None);
    }
}
