//! Rule `unsafe`: `unsafe` only in kernel, drivers and HAL, always justified (plan §7 rule 8).

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{ident_at, SourceFile};

/// Areas that may contain `unsafe`.
const ALLOWED_PREFIXES: [&str; 3] = ["kernel/", "drivers/", "system/hal/"];

/// Marker a justification comment must contain.
const MARKER: &str = "SAFETY:";

/// How many lines above the `unsafe` keyword the comment may start.
const WINDOW_LINES: u32 = 3;

const KEYWORD: &str = "unsafe";

/// Flags `unsafe` outside the allowed areas and `unsafe` without a justification.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files {
        let allowed_here = file.is_under(&ALLOWED_PREFIXES);
        for (i, token) in file.code.iter().enumerate() {
            if !ident_at(&file.code, i, KEYWORD) {
                continue;
            }
            let key = if !allowed_here {
                Key::UnsafeArea
            } else if !is_justified(file, token.line) {
                Key::UnsafeNoComment
            } else {
                continue;
            };
            let message = Message::plain(key);
            findings.push(Finding::error(
                Rule::Unsafe,
                &file.path,
                token.line,
                message,
            ));
        }
    }
    findings
}

fn is_justified(file: &SourceFile, line: u32) -> bool {
    let first = line.saturating_sub(WINDOW_LINES);
    file.comments
        .iter()
        .any(|c| c.end_line >= first && c.line <= line && c.text.contains(MARKER))
}
