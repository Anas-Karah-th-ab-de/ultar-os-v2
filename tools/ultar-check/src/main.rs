//! Command line entry point of `ultar-check` (developer tooling, area `tools/`).
//!
//! All behaviour lives in the library; see [`ultar_check::run`].

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::process::ExitCode;

/// Environment variable that selects the output language (`ar`, `en`, `de`).
const LANG_VARIABLE: &str = "ULTAR_LANG";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let env_lang = std::env::var(LANG_VARIABLE).ok();
    let result = ultar_check::run(&args, env_lang.as_deref());
    // A closed pipe must not turn a finished check into a panic.
    let _ = std::io::stdout().write_all(result.stdout.as_bytes());
    let _ = std::io::stderr().write_all(result.stderr.as_bytes());
    ExitCode::from(result.exit_code)
}
