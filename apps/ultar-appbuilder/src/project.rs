//! Checking a project folder: the manifest (APP-RULES §3) and the payload.

use crate::error::Issue;
use std::path::{Path, PathBuf};

/// Manifest file name inside a project.
pub const MANIFEST: &str = "ultar-app.toml";
/// Folder with the built files, laid out as installed.
pub const PAYLOAD: &str = "payload";
/// Largest manifest read.
const MAX_MANIFEST: u64 = 64 * 1024;
/// Sections every manifest must have.
const SECTIONS: [&str; 3] = ["form", "permissions", "limits"];

/// A project that passed every check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checked {
    /// The project folder.
    pub dir: PathBuf,
    /// App id (`<publisher>.<app>`).
    pub id: String,
    /// Version (`x.y.z`).
    pub version: String,
    /// Publisher (the name of the developer's key file on devices).
    pub publisher: String,
}

impl Checked {
    /// Manifest path.
    pub fn manifest(&self) -> PathBuf {
        self.dir.join(MANIFEST)
    }

    /// Payload folder.
    pub fn payload(&self) -> PathBuf {
        self.dir.join(PAYLOAD)
    }
}

fn word(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 64
        && part.starts_with(|c: char| c.is_ascii_lowercase())
        && part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `<publisher>.<app>` with lowercase words, the publisher equal to `publisher`.
pub fn valid_id(id: &str, publisher: &str) -> bool {
    match id.split_once('.') {
        Some((p, app)) => p == publisher && word(p) && word(app),
        None => false,
    }
}

/// `x.y.z` with numbers only.
pub fn valid_version(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
}

fn text<'a>(t: &'a toml::Table, key: &str) -> &'a str {
    t.get(key).and_then(toml::Value::as_str).unwrap_or("")
}

/// A path inside the project for a manifest path value (`/usr/…` → `payload/usr/…`), refusing
/// `..` and empty values.
fn inside(base: &Path, value: &str) -> Option<PathBuf> {
    let rel = value.trim_start_matches('/');
    if rel.is_empty() || rel.split('/').any(|c| c == ".." || c.is_empty()) {
        return None;
    }
    Some(base.join(rel))
}

fn dir_has_entries(dir: &Path) -> bool {
    std::fs::read_dir(dir).is_ok_and(|mut rd| rd.next().is_some())
}

/// Problems of the identity fields (id, version, publisher, kind, texts, arch).
fn header_issues(t: &toml::Table) -> Vec<Issue> {
    let mut issues = Vec::new();
    let (id, version, publisher) = (text(t, "id"), text(t, "version"), text(t, "publisher"));
    if publisher.is_empty() {
        issues.push(Issue::NoPublisher);
    }
    if !valid_id(id, publisher) {
        issues.push(Issue::BadId);
    }
    if !valid_version(version) {
        issues.push(Issue::BadVersion);
    }
    if !matches!(text(t, "kind"), "native" | "data") {
        issues.push(Issue::BadKind);
    }
    if !text(t, "name").starts_with("app.") || !text(t, "description").starts_with("app.") {
        issues.push(Issue::NameNotKey);
    }
    if !matches!(text(t, "arch"), "x86_64" | "aarch64") {
        issues.push(Issue::BadArch);
    }
    issues
}

/// Problems of the files (icon, payload, exec) and the sections.
fn file_issues(t: &toml::Table, dir: &Path) -> Vec<Issue> {
    let mut issues = Vec::new();
    if !inside(dir, text(t, "icon")).is_some_and(|p| p.is_file()) {
        issues.push(Issue::NoIcon);
    }
    let payload = dir.join(PAYLOAD);
    if !dir_has_entries(&payload) {
        issues.push(Issue::NoPayload);
    } else if let Some(exec) = t.get("exec").and_then(toml::Value::as_str) {
        if !inside(&payload, exec).is_some_and(|p| p.is_file()) {
            issues.push(Issue::ExecMissing);
        }
    }
    for section in SECTIONS {
        if !t.get(section).is_some_and(toml::Value::is_table) {
            issues.push(Issue::MissingSection(section));
        }
    }
    let network = t
        .get("permissions")
        .and_then(|p| p.get("network"))
        .and_then(toml::Value::as_str)
        .unwrap_or("none");
    if !matches!(network, "none" | "ask" | "vpn-only" | "allowed") {
        issues.push(Issue::BadNetwork);
    }
    issues
}

/// Checks a project folder; every problem found is listed (not only the first).
pub fn check(dir: &Path) -> Result<Checked, Vec<Issue>> {
    use std::io::Read;
    let mut body = String::new();
    let read = std::fs::File::open(dir.join(MANIFEST))
        .and_then(|f| f.take(MAX_MANIFEST).read_to_string(&mut body));
    if read.is_err() {
        return Err(vec![Issue::NoManifest]);
    }
    let Ok(t) = body.parse::<toml::Table>() else {
        return Err(vec![Issue::ManifestDamaged]);
    };
    let (id, version, publisher) = (text(&t, "id"), text(&t, "version"), text(&t, "publisher"));
    let mut issues = header_issues(&t);
    issues.extend(file_issues(&t, dir));
    if !issues.is_empty() {
        return Err(issues);
    }
    Ok(Checked {
        dir: dir.to_path_buf(),
        id: id.to_string(),
        version: version.to_string(),
        publisher: publisher.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_and_version_rules() {
        assert!(valid_id("acme.notes", "acme"));
        assert!(valid_id("km-code-craft.appbuilder", "km-code-craft"));
        for bad in [
            "notes",
            "other.notes",
            "acme.Notes",
            "acme.",
            ".notes",
            "acme.no tes",
            "acme.a.b",
        ] {
            assert!(!valid_id(bad, "acme"), "{bad}");
        }
        assert!(valid_version("0.8.0"));
        for bad in ["1.2", "1.2.3.4", "1.x.3", "", "1..3", "1.2.3-beta"] {
            assert!(!valid_version(bad), "{bad}");
        }
    }

    #[test]
    fn inside_refuses_escapes() {
        let base = Path::new("/p");
        assert_eq!(
            inside(base, "/usr/bin/x"),
            Some(PathBuf::from("/p/usr/bin/x"))
        );
        assert_eq!(inside(base, "../etc/passwd"), None);
        assert_eq!(inside(base, "a/../../b"), None);
        assert_eq!(inside(base, ""), None);
        assert_eq!(inside(base, "a//b"), None);
    }
}
