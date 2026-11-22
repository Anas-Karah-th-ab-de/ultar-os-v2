//! The individual checks and their registry.

pub mod budget;
pub mod edition;
pub mod layers;
pub mod linux_calls;
pub mod logical_layout;
pub mod paths;
pub mod size;
pub mod text;
pub mod unsafe_code;
pub mod unwrap;

use crate::error::CheckError;
use crate::finding::{Finding, Message};
use crate::scan::SourceFile;
use std::path::Path;

/// One check of the quality gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rule {
    /// Area membership and dependency directions between areas.
    Layers,
    /// Hardware paths only inside HAL, drivers and kernel.
    Paths,
    /// No edition queries outside the profile and init crates.
    Edition,
    /// No hardcoded user-visible text.
    Text,
    /// File, function and struct size limits.
    Size,
    /// `unsafe` only where allowed and always justified.
    Unsafe,
    /// No panicking shortcuts outside tests.
    Unwrap,
    /// Layout uses start/end instead of left/right.
    LogicalLayout,
    /// Calls of external Linux programs (reduction list).
    LinuxCalls,
    /// Size budgets of built binaries.
    Budget,
}

impl Rule {
    /// Every rule in the order `all` runs them.
    pub const ALL: [Rule; 10] = [
        Rule::Layers,
        Rule::Paths,
        Rule::Edition,
        Rule::Text,
        Rule::Size,
        Rule::Unsafe,
        Rule::Unwrap,
        Rule::LogicalLayout,
        Rule::LinuxCalls,
        Rule::Budget,
    ];

    /// Name used on the command line, in output and in the baseline.
    pub fn name(self) -> &'static str {
        match self {
            Rule::Layers => "layers",
            Rule::Paths => "paths",
            Rule::Edition => "edition",
            Rule::Text => "text",
            Rule::Size => "size",
            Rule::Unsafe => "unsafe",
            Rule::Unwrap => "unwrap",
            Rule::LogicalLayout => "logical-layout",
            Rule::LinuxCalls => "linux-calls",
            Rule::Budget => "budget",
        }
    }

    /// Looks a rule up by its name.
    pub fn from_name(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|rule| rule.name() == name)
    }

    /// Whether the rule needs the lexed Rust sources.
    pub fn needs_sources(self) -> bool {
        !matches!(self, Rule::Layers | Rule::Budget)
    }

    /// Runs the rule.
    pub fn run(self, context: &Context<'_>) -> Result<Outcome, CheckError> {
        let files = context.files;
        let findings = match self {
            Rule::Layers => layers::check(context.root)?,
            Rule::Budget => return budget::check(context.root, context.target_dir),
            Rule::Paths => paths::check(files),
            Rule::Edition => edition::check(files),
            Rule::Text => text::check(files),
            Rule::Size => size::check(files),
            Rule::Unsafe => unsafe_code::check(files),
            Rule::Unwrap => unwrap::check(files),
            Rule::LogicalLayout => logical_layout::check(files),
            Rule::LinuxCalls => linux_calls::check(files),
        };
        Ok(Outcome {
            findings,
            notes: Vec::new(),
        })
    }
}

/// Everything a rule may look at.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// Repository root.
    pub root: &'a Path,
    /// Lexed Rust sources (empty when no selected rule needs them).
    pub files: &'a [SourceFile],
    /// Cargo target directory for the budget rule.
    pub target_dir: &'a Path,
}

/// Result of running one or more rules.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Violations and warnings.
    pub findings: Vec<Finding>,
    /// Informational notes (for example skipped checks).
    pub notes: Vec<Message>,
}

/// Runs the given rules and merges their results in rule order.
pub fn run_rules(rules: &[Rule], context: &Context<'_>) -> Result<Outcome, CheckError> {
    let mut merged = Outcome::default();
    for rule in rules {
        let mut outcome = rule.run(context)?;
        outcome
            .findings
            .sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
        merged.findings.append(&mut outcome.findings);
        merged.notes.append(&mut outcome.notes);
    }
    Ok(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for rule in Rule::ALL {
            assert_eq!(Rule::from_name(rule.name()), Some(rule));
        }
        assert_eq!(Rule::from_name("all"), None);
        assert_eq!(Rule::from_name(""), None);
    }

    #[test]
    fn only_layers_and_budget_work_without_sources() {
        let without: Vec<Rule> = Rule::ALL
            .into_iter()
            .filter(|r| !r.needs_sources())
            .collect();
        assert_eq!(without, vec![Rule::Layers, Rule::Budget]);
    }
}
