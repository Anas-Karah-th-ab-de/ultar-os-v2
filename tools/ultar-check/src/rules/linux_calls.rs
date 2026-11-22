//! Rule `linux-calls`: calls of external programs (plan §3.5, §7 rule 0b).
//!
//! Every `Command::new("program")` with a literal program name is a
//! dependency on a Linux userland tool (`modprobe`, `apt`, `ip`, `sh` ...).
//! Existing calls are recorded in the baseline (the reduction list); the
//! ratchet makes sure no new ones appear. Programs of Ultar itself
//! (`ultar-…`) and developer tooling below `tools/` are not counted.

use super::Rule;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::scan::{ident_at, path_sep_at, punct_at, SourceFile, Token, TokenKind};

/// Areas whose process spawning is not an OS dependency.
const EXEMPT_PREFIXES: [&str; 1] = ["tools/"];

/// Program name prefixes that belong to the system itself.
const OWN_PROGRAM_PREFIXES: [&str; 2] = ["ultar-", "ultar_"];

/// Flags `Command::new("…")` calls of programs that are not part of Ultar.
pub fn check(files: &[SourceFile]) -> Vec<Finding> {
    let mut findings = Vec::new();
    for file in files.iter().filter(|f| !f.is_under(&EXEMPT_PREFIXES)) {
        for (i, token) in file.code.iter().enumerate() {
            if token.in_test {
                continue;
            }
            let Some(program) = spawned_program(&file.code, i) else {
                continue;
            };
            if OWN_PROGRAM_PREFIXES.iter().any(|p| program.starts_with(p)) {
                continue;
            }
            let message = Message::new(Key::LinuxCall, &[program]);
            findings.push(Finding::error(
                Rule::LinuxCalls,
                &file.path,
                token.line,
                message,
            ));
        }
    }
    findings
}

/// Program name if tokens at `i` are `Command::new("…")`.
fn spawned_program(tokens: &[Token], i: usize) -> Option<&str> {
    let shape = ident_at(tokens, i, "Command")
        && path_sep_at(tokens, i + 1)
        && ident_at(tokens, i + 3, "new")
        && punct_at(tokens, i + 4, '(');
    if !shape {
        return None;
    }
    let literal = tokens.get(i + 5).filter(|t| t.kind == TokenKind::Str)?;
    let name = literal.text.rsplit('/').next().unwrap_or(&literal.text);
    (!name.is_empty()).then_some(name)
}
