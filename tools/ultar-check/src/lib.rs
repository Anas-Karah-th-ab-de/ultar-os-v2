//! `ultar-check` – the repository quality gate of Ultar OS.
//!
//! **Area:** developer tooling (`tools/`), not part of any system image.
//!
//! The plan (§3.0 and §7) splits the repository into seven areas with fixed
//! dependency directions and a set of clean-code rules that CI enforces.
//! This crate implements those checks without calling `cargo` or any other
//! external program:
//!
//! | rule | checks |
//! |---|---|
//! | `layers` | every crate in exactly one area; path dependencies only in allowed directions |
//! | `paths` | hardware paths only in HAL, drivers and kernel |
//! | `edition` | no edition queries outside the profile and init crates |
//! | `text` | no hardcoded Arabic or German text in string literals |
//! | `size` | file ≤ 600 lines (warning above 400), function ≤ 60 lines, struct ≤ 15 fields |
//! | `unsafe` | `unsafe` only in kernel, drivers, HAL and with a `// SAFETY:` comment |
//! | `unwrap` | no `.unwrap()`, `.expect(`, `panic!`, `todo!`, `unimplemented!` outside tests |
//! | `logical-layout` | no `left`/`right` identifiers in platform and apps |
//! | `linux-calls` | calls of external programs (reduction list) |
//! | `budget` | size budgets of built binaries |
//! | `brand` | product and company identity only from `system/core/ultar-brand` (docs/BRAND.md) |
//!
//! Legacy violations are listed with a count in `tools/check-baseline.toml`;
//! a rule + path group passes while its count does not grow (ratchet).
//!
//! Library entry points: [`execute`] runs rules and returns a [`Report`],
//! [`baseline_toml`] prints the current state as a baseline and [`run`] is
//! the complete command line behaviour.

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

pub mod baseline;
pub mod cli;
pub mod error;
pub mod finding;
pub mod layers_config;
pub mod manifest;
pub mod msg;
pub mod repo;
pub mod report;
pub mod rules;
pub mod scan;

use baseline::Baseline;
use cli::{Command, Format, Options};
use error::CheckError;
use msg::Key;
use report::Report;
use rules::{Context, Outcome, Rule};

/// Exit code: all selected checks passed.
pub const EXIT_OK: u8 = 0;
/// Exit code: at least one violation.
pub const EXIT_VIOLATIONS: u8 = 1;
/// Exit code: usage, configuration or I/O error.
pub const EXIT_ERROR: u8 = 2;

/// What the binary prints and returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunResult {
    /// Text for standard output.
    pub stdout: String,
    /// Text for standard error.
    pub stderr: String,
    /// Process exit code.
    pub exit_code: u8,
}

fn run_selected(options: &Options, rules: &[Rule]) -> Result<Outcome, CheckError> {
    let files = if rules.iter().any(|rule| rule.needs_sources()) {
        repo::load_sources(&options.root)?
    } else {
        Vec::new()
    };
    let target_dir = options
        .target_dir
        .clone()
        .unwrap_or_else(|| options.root.join("target"));
    let context = Context {
        root: &options.root,
        files: &files,
        target_dir: &target_dir,
    };
    rules::run_rules(rules, &context)
}

fn load_baseline(options: &Options) -> Result<Baseline, CheckError> {
    if !options.use_baseline {
        return Ok(Baseline::default());
    }
    let path = options
        .baseline
        .clone()
        .unwrap_or_else(|| options.root.join(baseline::DEFAULT_PATH));
    match repo::read_optional(&path)? {
        Some(text) => Baseline::parse(&path.display().to_string(), &text),
        None => Ok(Baseline::default()),
    }
}

fn selected_rules(options: &Options) -> Vec<Rule> {
    match &options.command {
        Command::Check(rules) => rules.clone(),
        Command::Baseline | Command::Help => Rule::ALL.to_vec(),
    }
}

/// Runs the rules selected in `options` and applies the baseline.
pub fn execute(options: &Options) -> Result<Report, CheckError> {
    let rules = selected_rules(options);
    let baseline = load_baseline(options)?;
    let outcome = run_selected(options, &rules)?;
    let gated = baseline.apply(&rules, outcome.findings);
    let mut notes = outcome.notes;
    notes.extend(gated.notes);
    Ok(Report {
        violations: gated.violations,
        warnings: gated.warnings,
        notes,
        baselined: gated.baselined,
    })
}

/// Runs every rule without a baseline and returns a baseline for the current state.
pub fn baseline_toml(options: &Options) -> Result<String, CheckError> {
    let outcome = run_selected(options, &Rule::ALL)?;
    Ok(Baseline::from_findings(&outcome.findings).to_toml())
}

/// Complete command line behaviour: parses `args` (without the program name) and runs.
///
/// `env_lang` is the value of the `ULTAR_LANG` environment variable, if set.
pub fn run(args: &[String], env_lang: Option<&str>) -> RunResult {
    let fallback_lang = env_lang
        .and_then(msg::Lang::from_code)
        .unwrap_or(msg::Lang::Ar);
    let failure = |error: CheckError, lang| RunResult {
        stdout: String::new(),
        stderr: format!("ultar-check: {}\n", error.message().render(lang)),
        exit_code: EXIT_ERROR,
    };
    let options = match cli::parse(args, env_lang) {
        Ok(options) => options,
        Err(usage) => return failure(usage.into(), fallback_lang),
    };
    let lang = options.lang;
    let success = |stdout: String, exit_code| RunResult {
        stdout,
        stderr: String::new(),
        exit_code,
    };
    match options.command {
        Command::Help => success(format!("{}\n", msg::render(lang, Key::Help, &[])), EXIT_OK),
        Command::Baseline => match baseline_toml(&options) {
            Ok(text) => success(text, EXIT_OK),
            Err(error) => failure(error, lang),
        },
        Command::Check(_) => match execute(&options) {
            Ok(report) => {
                let text = match options.format {
                    Format::Text => report.to_text(lang),
                    Format::Json => report.to_json(lang),
                };
                let code = if report.passed() {
                    EXIT_OK
                } else {
                    EXIT_VIOLATIONS
                };
                success(text, code)
            }
            Err(error) => failure(error, lang),
        },
    }
}
