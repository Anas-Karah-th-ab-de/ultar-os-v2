//! Files that repeat the identity outside Rust code: `os-release`, crate manifests and the
//! brand mark of the icon set.

use super::SOURCE_CRATE;
use crate::error::CheckError;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::repo;
use crate::rules::Rule;
use crate::scan::{SourceFile, Token, TokenKind};
use std::path::Path;

/// The brand part of the system's `os-release` in the repository.
pub const OS_RELEASE_PATH: &str = "rootfs/etc/os-release";

/// The icon table of the toolkit.
const ICON_TABLE: &str = "platform/ultar-ui/src/icon/mod.rs";

fn error(path: &str, line: u32, key: Key, args: &[&str]) -> Finding {
    Finding::error(Rule::Brand, path, line, Message::new(key, args))
}

/// `rootfs/etc/os-release` must be exactly the text of the source.
fn check_os_release(root: &Path, findings: &mut Vec<Finding>) -> Result<(), CheckError> {
    let expected = ultar_brand::release::OS_RELEASE;
    let Some(text) = repo::read_optional(&root.join(OS_RELEASE_PATH))? else {
        findings.push(error(OS_RELEASE_PATH, 1, Key::BrandOsReleaseMissing, &[]));
        return Ok(());
    };
    if text == expected {
        return Ok(());
    }
    let mut found = text.lines();
    let mut number = 0u32;
    for wanted in expected.lines() {
        number += 1;
        if found.next() != Some(wanted) {
            findings.push(error(
                OS_RELEASE_PATH,
                number,
                Key::BrandOsRelease,
                &[wanted],
            ));
            return Ok(());
        }
    }
    // same lines, but more text follows (or the final line break is missing)
    findings.push(error(
        OS_RELEASE_PATH,
        number + 1,
        Key::BrandOsRelease,
        &[""],
    ));
    Ok(())
}

/// An `authors` entry of a manifest must be the one of the source.
fn check_manifests(root: &Path, findings: &mut Vec<Finding>) -> Result<(), CheckError> {
    let expected = format!("[\"{}\"]", ultar_brand::MANIFEST_AUTHORS);
    for path in repo::walk(root)?.manifests {
        let text = repo::read_text(root, &path)?;
        for (index, line) in text.lines().enumerate() {
            let Some(value) = line
                .strip_prefix("authors")
                .map(str::trim_start)
                .and_then(|rest| rest.strip_prefix('='))
            else {
                continue;
            };
            if value.trim() != expected {
                let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                findings.push(error(&path, number, Key::BrandAuthors, &[&expected]));
            }
        }
    }
    Ok(())
}

/// A mail address at the company's own host that is not the public contact address, in a
/// line of a script, a configuration file or a document.
fn stale_address(line: &str) -> Option<&str> {
    let host = ultar_brand::WEBSITE_HOST;
    let mut from = 0;
    while let Some(found) = line.get(from..).and_then(|rest| rest.find(host)) {
        let at = from + found;
        from = at + host.len();
        let Some(head) = line.get(..at).and_then(|head| head.strip_suffix('@')) else {
            continue;
        };
        let local_len = head
            .bytes()
            .rev()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'+' | b'-'))
            .count();
        let address = line.get(head.len() - local_len..from)?;
        if local_len > 0 && address != ultar_brand::CONTACT_EMAIL {
            return Some(address);
        }
    }
    None
}

/// Scripts, configuration and documents may name only the public contact address.
fn check_addresses(root: &Path, findings: &mut Vec<Finding>) -> Result<(), CheckError> {
    let tree = repo::walk(root)?;
    for path in tree.texts.iter().chain(&tree.manifests) {
        let text = repo::read_text(root, path)?;
        for (index, line) in text.lines().enumerate() {
            if let Some(address) = stale_address(line) {
                let number = u32::try_from(index + 1).unwrap_or(u32::MAX);
                let args = [address, ultar_brand::CONTACT_EMAIL];
                findings.push(error(path, number, Key::BrandAddress, &args));
            }
        }
    }
    Ok(())
}

/// The tokens of the `Mark, "mark", …;` entry of the icon table, after the name.
fn mark_entry(code: &[Token]) -> Option<&[Token]> {
    let name = ultar_brand::mark::NAME;
    let start = code.windows(3).position(|w| {
        w[0].kind == TokenKind::Ident
            && w[1].kind == TokenKind::Punct
            && w[1].text == ","
            && w[2].kind == TokenKind::Str
            && w[2].text == name
    })? + 3;
    let rest = code.get(start..)?;
    let end = rest
        .iter()
        .position(|t| t.kind == TokenKind::Punct && t.text == ";")?;
    rest.get(..end)
}

/// The brand mark of the icon set must be the path of the source, never mirrored.
fn check_mark(files: &[SourceFile], findings: &mut Vec<Finding>) {
    let Some(file) = files.iter().find(|f| f.path == ICON_TABLE) else {
        return;
    };
    let Some(entry) = mark_entry(&file.code) else {
        return;
    };
    let line = entry.first().map_or(1, |t| t.line);
    let mirrored = entry
        .iter()
        .any(|t| t.kind == TokenKind::Ident && t.text == "true");
    // the first literal is the meaning, the following ones are the path data
    let path: String = entry
        .iter()
        .filter(|t| t.kind == TokenKind::Str)
        .skip(1)
        .map(|t| t.text.as_str())
        .collect();
    if path != ultar_brand::mark::PATH || mirrored != ultar_brand::mark::MIRRORED {
        let args = [ultar_brand::mark::PATH];
        findings.push(error(ICON_TABLE, line, Key::BrandMark, &args));
    }
}

/// Checks the files; `os-release` only in a repository that has the source crate.
pub fn check(root: &Path, files: &[SourceFile]) -> Result<Vec<Finding>, CheckError> {
    let mut findings = Vec::new();
    if root.join(SOURCE_CRATE).is_dir() {
        check_os_release(root, &mut findings)?;
    }
    check_manifests(root, &mut findings)?;
    check_addresses(root, &mut findings)?;
    check_mark(files, &mut findings);
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_other_addresses_at_the_own_host_are_stale() {
        assert_eq!(stale_address("Maintainer: X <info@code-craft.tech>"), None);
        assert_eq!(
            stale_address("Maintainer: X <ak@code-craft.tech>"),
            Some("ak@code-craft.tech")
        );
        assert_eq!(
            stale_address("info@code-craft.tech, a.b+c@code-craft.tech"),
            Some("a.b+c@code-craft.tech")
        );
        assert_eq!(
            stale_address("https://code-craft.tech and @code-craft.tech"),
            None
        );
        assert_eq!(stale_address("x@store.code-craft.tech"), None);
        assert_eq!(stale_address("x@example.org"), None);
        assert_eq!(stale_address(""), None);
    }
}
