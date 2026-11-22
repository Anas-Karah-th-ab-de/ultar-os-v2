//! Baseline of tolerated legacy violations with a ratchet ("counts may only go down").

use crate::error::{CheckError, ConfigIssue};
use crate::finding::{Finding, Message, Severity};
use crate::msg::Key;
use crate::rules::Rule;
use serde::Deserialize;
use std::collections::BTreeMap;

/// Default repository-relative location of the baseline file.
pub const DEFAULT_PATH: &str = "tools/check-baseline.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBaseline {
    #[serde(default)]
    entry: Vec<RawEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    rule: String,
    path: String,
    count: u32,
}

/// Tolerated violation counts per rule and path.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Baseline {
    entries: BTreeMap<(Rule, String), u32>,
}

/// Findings after the baseline was applied.
#[derive(Debug, Default)]
pub struct Gated {
    /// Violations that fail the check.
    pub violations: Vec<Finding>,
    /// Warnings (never fail the check, never baselined).
    pub warnings: Vec<Finding>,
    /// Ratchet hints: exceeded, lowerable or removable entries.
    pub notes: Vec<Message>,
    /// Number of violations tolerated by the baseline.
    pub baselined: usize,
}

impl Baseline {
    /// Parses a baseline file; `path` is used in error messages.
    pub fn parse(path: &str, text: &str) -> Result<Baseline, CheckError> {
        let raw: RawBaseline = toml::from_str(text).map_err(|source| CheckError::Toml {
            path: path.to_string(),
            source,
        })?;
        let fail = |issue| CheckError::Config {
            path: path.to_string(),
            issue,
        };
        let mut entries = BTreeMap::new();
        for entry in raw.entry {
            let rule = Rule::from_name(&entry.rule)
                .ok_or_else(|| fail(ConfigIssue::UnknownRule(entry.rule.clone())))?;
            let previous = entries.insert((rule, entry.path.clone()), entry.count);
            if previous.is_some() {
                return Err(fail(ConfigIssue::DuplicateEntry {
                    rule: entry.rule,
                    path: entry.path,
                }));
            }
        }
        Ok(Baseline { entries })
    }

    /// Builds the baseline that exactly tolerates the given findings.
    pub fn from_findings(findings: &[Finding]) -> Baseline {
        Baseline {
            entries: count_errors(findings),
        }
    }

    /// Tolerated count for a rule and path (0 if not listed).
    pub fn allowed(&self, rule: Rule, path: &str) -> u32 {
        self.entries
            .get(&(rule, path.to_string()))
            .copied()
            .unwrap_or(0)
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the baseline has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Applies the ratchet to the findings of the rules that ran.
    ///
    /// A rule + path group passes while its count is not above the baseline;
    /// otherwise every finding of the group is reported. Entries of rules
    /// that did not run are ignored.
    pub fn apply(&self, ran: &[Rule], findings: Vec<Finding>) -> Gated {
        let counts = count_errors(&findings);
        let mut gated = Gated::default();
        for finding in findings {
            let key = (finding.rule, finding.path.clone());
            let count = counts.get(&key).copied().unwrap_or(0);
            if finding.severity == Severity::Warning {
                gated.warnings.push(finding);
            } else if count <= self.allowed(finding.rule, &finding.path) {
                gated.baselined += 1;
            } else {
                gated.violations.push(finding);
            }
        }
        for ((rule, path), allowed) in &self.entries {
            if !ran.contains(rule) {
                continue;
            }
            let count = counts.get(&(*rule, path.clone())).copied().unwrap_or(0);
            let numbers = |a: u32, b: u32| {
                [
                    rule.name().to_string(),
                    path.clone(),
                    a.to_string(),
                    b.to_string(),
                ]
            };
            if count > *allowed {
                let args = numbers(count, *allowed);
                gated.notes.push(Message::new(Key::BaselineExceeded, &args));
            } else if count == 0 {
                let args = [rule.name(), path.as_str()];
                gated
                    .notes
                    .push(Message::new(Key::BaselineCanRemove, &args));
            } else if count < *allowed {
                let args = numbers(*allowed, count);
                gated.notes.push(Message::new(Key::BaselineCanLower, &args));
            }
        }
        gated
    }

    /// Serializes the baseline as TOML (sorted by rule, then path).
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        for ((rule, path), count) in &self.entries {
            let quoted = toml::Value::String(path.clone()).to_string();
            out.push_str("[[entry]]\n");
            out.push_str(&format!("rule = \"{}\"\n", rule.name()));
            out.push_str(&format!("path = {quoted}\n"));
            out.push_str(&format!("count = {count}\n\n"));
        }
        out
    }
}

fn count_errors(findings: &[Finding]) -> BTreeMap<(Rule, String), u32> {
    let mut counts = BTreeMap::new();
    for finding in findings.iter().filter(|f| f.severity == Severity::Error) {
        *counts
            .entry((finding.rule, finding.path.clone()))
            .or_insert(0) += 1;
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(rule: Rule, path: &str, line: u32) -> Finding {
        Finding::error(rule, path, line, Message::new(Key::PanicCall, &["x"]))
    }

    fn baseline(entries: &[(Rule, &str, u32)]) -> Baseline {
        let mut text = String::new();
        for (rule, path, count) in entries {
            text.push_str(&format!(
                "[[entry]]\nrule = \"{}\"\npath = \"{path}\"\ncount = {count}\n",
                rule.name()
            ));
        }
        Baseline::parse("b.toml", &text).unwrap()
    }

    #[test]
    fn equal_count_passes_silently() {
        let b = baseline(&[(Rule::Unwrap, "a.rs", 2)]);
        let findings = vec![
            finding(Rule::Unwrap, "a.rs", 1),
            finding(Rule::Unwrap, "a.rs", 2),
        ];
        let gated = b.apply(&[Rule::Unwrap], findings);
        assert!(gated.violations.is_empty());
        assert_eq!(gated.baselined, 2);
        assert!(gated.notes.is_empty());
    }

    #[test]
    fn grown_count_reports_the_whole_group() {
        let b = baseline(&[(Rule::Unwrap, "a.rs", 1)]);
        let findings = vec![
            finding(Rule::Unwrap, "a.rs", 1),
            finding(Rule::Unwrap, "a.rs", 2),
        ];
        let gated = b.apply(&[Rule::Unwrap], findings);
        assert_eq!(gated.violations.len(), 2);
        assert_eq!(gated.baselined, 0);
        assert_eq!(gated.notes.len(), 1);
        assert_eq!(gated.notes[0].key, Key::BaselineExceeded);
        assert_eq!(gated.notes[0].args, ["unwrap", "a.rs", "2", "1"]);
    }

    #[test]
    fn shrunk_count_suggests_lowering_and_removal() {
        let b = baseline(&[(Rule::Unwrap, "a.rs", 5), (Rule::Unwrap, "gone.rs", 3)]);
        let gated = b.apply(&[Rule::Unwrap], vec![finding(Rule::Unwrap, "a.rs", 1)]);
        assert!(gated.violations.is_empty());
        assert_eq!(gated.notes.len(), 2);
        assert_eq!(gated.notes[0].key, Key::BaselineCanLower);
        assert_eq!(gated.notes[0].args, ["unwrap", "a.rs", "5", "1"]);
        assert_eq!(gated.notes[1].key, Key::BaselineCanRemove);
    }

    #[test]
    fn groups_are_independent_per_rule_and_path() {
        let b = baseline(&[(Rule::Unwrap, "a.rs", 1)]);
        let findings = vec![
            finding(Rule::Unwrap, "a.rs", 1),
            finding(Rule::Unwrap, "b.rs", 1),
            finding(Rule::Paths, "a.rs", 1),
        ];
        let gated = b.apply(&[Rule::Unwrap, Rule::Paths], findings);
        assert_eq!(gated.baselined, 1);
        assert_eq!(gated.violations.len(), 2);
    }

    #[test]
    fn entries_of_rules_that_did_not_run_are_ignored() {
        let b = baseline(&[(Rule::Size, "a.rs", 3)]);
        let gated = b.apply(&[Rule::Unwrap], Vec::new());
        assert!(gated.notes.is_empty());
    }

    #[test]
    fn warnings_are_neither_counted_nor_baselined() {
        let b = baseline(&[(Rule::Size, "a.rs", 1)]);
        let warning = Finding::warning(
            Rule::Size,
            "a.rs",
            1,
            Message::new(Key::FileLong, &["1", "2"]),
        );
        let gated = b.apply(&[Rule::Size], vec![warning]);
        assert_eq!(gated.warnings.len(), 1);
        assert_eq!(gated.baselined, 0);
        assert_eq!(gated.notes[0].key, Key::BaselineCanRemove);
    }

    #[test]
    fn toml_round_trip() {
        let findings = vec![
            finding(Rule::Unwrap, "a \"q\".rs", 1),
            finding(Rule::Unwrap, "a \"q\".rs", 2),
            finding(Rule::LinuxCalls, "b.rs", 1),
        ];
        let generated = Baseline::from_findings(&findings);
        assert_eq!(generated.len(), 2);
        let parsed = Baseline::parse("b.toml", &generated.to_toml()).unwrap();
        assert_eq!(parsed, generated);
        assert_eq!(parsed.allowed(Rule::Unwrap, "a \"q\".rs"), 2);
        assert_eq!(parsed.allowed(Rule::Unwrap, "other.rs"), 0);
        assert!(Baseline::default().is_empty());
        assert_eq!(Baseline::default().to_toml(), "");
    }

    #[test]
    fn bad_baselines_are_rejected() {
        let unknown = "[[entry]]\nrule = \"nope\"\npath = \"a\"\ncount = 1\n";
        assert!(matches!(
            Baseline::parse("b.toml", unknown).unwrap_err(),
            CheckError::Config {
                issue: ConfigIssue::UnknownRule(_),
                ..
            }
        ));
        let twice = "[[entry]]\nrule = \"size\"\npath = \"a\"\ncount = 1\n".repeat(2);
        assert!(matches!(
            Baseline::parse("b.toml", &twice).unwrap_err(),
            CheckError::Config {
                issue: ConfigIssue::DuplicateEntry { .. },
                ..
            }
        ));
        let negative = "[[entry]]\nrule = \"size\"\npath = \"a\"\ncount = -1\n";
        assert!(matches!(
            Baseline::parse("b.toml", negative).unwrap_err(),
            CheckError::Toml { .. }
        ));
        assert!(Baseline::parse("b.toml", "# only a comment\n")
            .unwrap()
            .is_empty());
    }
}
