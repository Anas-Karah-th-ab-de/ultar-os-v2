//! Rule `paths`: hardware paths only inside HAL, drivers and kernel (plan §7 rule 2).

use super::Rule;
use crate::finding::{excerpt, Finding, Message};
use crate::msg::Key;
use crate::scan::{SourceFile, TokenKind};

/// Top-level pseudo file systems that give direct hardware or kernel access.
const HARDWARE_ROOTS: [&str; 3] = ["sys", "dev", "proc"];

/// Areas that may name hardware paths.
const EXEMPT_PREFIXES: [&str; 3] = ["system/hal/", "drivers/", "kernel/"];

/// Flags string literals that contain a hardware path outside the exempt areas.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let needles: Vec<String> = HARDWARE_ROOTS.iter().map(|r| format!("/{r}/")).collect();
    let mut findings = Vec::new();
    for file in files.iter().filter(|f| !f.is_under(&EXEMPT_PREFIXES)) {
        for token in &file.code {
            let hit = token.kind == TokenKind::Str
                && !token.in_test
                && needles.iter().any(|n| token.text.contains(n.as_str()));
            if hit {
                let message = Message::new(Key::HardwarePath, &[excerpt(&token.text)]);
                findings.push(Finding::error(Rule::Paths, &file.path, token.line, message));
            }
        }
    }
    findings
}
