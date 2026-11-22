//! Rule `edition`: no edition queries outside the profile and init crates (plan §7 rule 3).

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{ident_at, path_sep_at, punct_at, SourceFile, Token};

/// Crates that may look at the edition directly.
const ALLOWED_PREFIXES: [&str; 3] = [
    "system/core/ultar-profile/",
    "system/core/ultar-init/",
    "system/hal/ultar-hal-system/",
];

const EDITION_TYPE: &str = "Edition";
const PROFILE_CRATE: &str = "ultar_profile";

/// Flags `Edition::…` paths and imports of `ultar_profile::Edition`.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|f| !f.is_under(&ALLOWED_PREFIXES)) {
        let mut last_line = 0;
        for line in edition_lines(&file.code) {
            if line != last_line {
                let message = Message::plain(Key::EditionQuery);
                findings.push(Finding::error(Rule::Edition, &file.path, line, message));
                last_line = line;
            }
        }
    }
    findings
}

fn edition_lines(tokens: &[Token]) -> Vec<u32> {
    let mut lines = Vec::new();
    let mut profile_use_until = 0usize;
    for (i, token) in tokens.iter().enumerate() {
        if token.in_test {
            continue;
        }
        if ident_at(tokens, i, "use") {
            profile_use_until = profile_use_end(tokens, i).unwrap_or(profile_use_until);
        }
        if !ident_at(tokens, i, EDITION_TYPE) {
            continue;
        }
        let variant_access = path_sep_at(tokens, i + 1);
        let qualified =
            i >= 3 && path_sep_at(tokens, i - 2) && ident_at(tokens, i - 3, PROFILE_CRATE);
        if variant_access || qualified || i < profile_use_until {
            lines.push(token.line);
        }
    }
    lines
}

/// End index of a `use` statement starting at `start`, if it mentions the profile crate.
fn profile_use_end(tokens: &[Token], start: usize) -> Option<usize> {
    let end = (start..tokens.len()).find(|&i| punct_at(tokens, i, ';'))?;
    (start..end)
        .any(|i| ident_at(tokens, i, PROFILE_CRATE))
        .then_some(end)
}
