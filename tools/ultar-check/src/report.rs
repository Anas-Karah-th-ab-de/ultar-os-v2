//! Final result of a run and its text / JSON rendering.

use crate::finding::{Finding, Message};
use crate::msg::{self, Key, Lang};
use serde_json::{json, Value};

/// Result of a check run after the baseline was applied.
#[derive(Debug, Default)]
pub struct Report {
    /// Violations that fail the run.
    pub violations: Vec<Finding>,
    /// Warnings.
    pub warnings: Vec<Finding>,
    /// Notes: skipped checks and baseline hints.
    pub notes: Vec<Message>,
    /// Number of violations tolerated by the baseline.
    pub baselined: usize,
}

impl Report {
    /// Whether the run passed (no violations).
    pub fn passed(&self) -> bool {
        self.violations.is_empty()
    }

    /// Renders the report as lines of `path:line: rule: detail` plus notes and a summary.
    pub fn to_text(&self, lang: Lang) -> String {
        let mut lines: Vec<String> = Vec::new();
        lines.extend(self.violations.iter().map(|f| f.to_line(lang)));
        lines.extend(self.warnings.iter().map(|f| f.to_line(lang)));
        let label = msg::render(lang, Key::LabelNote, &[]);
        lines.extend(
            self.notes
                .iter()
                .map(|note| format!("{label}: {}", note.render(lang))),
        );
        lines.push(self.summary().render(lang));
        let mut text = lines.join("\n");
        text.push('\n');
        text
    }

    /// Renders the report as a JSON document.
    pub fn to_json(&self, lang: Lang) -> String {
        let findings = |list: &[Finding]| -> Vec<Value> {
            list.iter().map(|f| finding_json(f, lang)).collect()
        };
        let notes: Vec<Value> = self.notes.iter().map(|n| message_json(n, lang)).collect();
        let document = json!({
            "passed": self.passed(),
            "baselined": self.baselined,
            "violations": findings(&self.violations),
            "warnings": findings(&self.warnings),
            "notes": notes,
            "summary": self.summary().render(lang),
        });
        let mut text = format!("{document:#}");
        text.push('\n');
        text
    }

    fn summary(&self) -> Message {
        let baselined = self.baselined.to_string();
        let warnings = self.warnings.len().to_string();
        if self.passed() {
            Message::new(Key::SummaryOk, &[baselined, warnings])
        } else {
            let violations = self.violations.len().to_string();
            Message::new(Key::SummaryFail, &[violations, baselined, warnings])
        }
    }
}

fn message_json(message: &Message, lang: Lang) -> Value {
    json!({
        "key": message.key.id(),
        "args": message.args,
        "detail": message.render(lang),
    })
}

fn finding_json(finding: &Finding, lang: Lang) -> Value {
    json!({
        "rule": finding.rule.name(),
        "path": finding.path,
        "line": finding.line,
        "key": finding.message.key.id(),
        "args": finding.message.args,
        "detail": finding.message.render(lang),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rule;

    fn sample() -> Report {
        Report {
            violations: vec![Finding::error(
                Rule::Unwrap,
                "a.rs",
                3,
                Message::new(Key::PanicCall, &[".unwrap()"]),
            )],
            warnings: vec![Finding::warning(
                Rule::Size,
                "b.rs",
                1,
                Message::new(Key::FileLong, &["450", "400"]),
            )],
            notes: vec![Message::new(Key::BudgetNotBuilt, &["ultar-init"])],
            baselined: 4,
        }
    }

    #[test]
    fn text_lists_violations_warnings_notes_and_summary() {
        let text = sample().to_text(Lang::En);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], "a.rs:3: unwrap: `.unwrap()` outside test code");
        assert_eq!(
            lines[1],
            "b.rs:1: size: warning: file is getting long: 450 lines (warn above 400)"
        );
        assert_eq!(lines[2], "note: budget: `ultar-init` is not built, skipped");
        assert_eq!(
            lines[3],
            "ultar-check: FAILED - 1 violations (4 baselined, 1 warnings)"
        );
        assert!(text.ends_with('\n'));
    }

    #[test]
    fn empty_report_passes() {
        let report = Report::default();
        assert!(report.passed());
        assert_eq!(
            report.to_text(Lang::En),
            "ultar-check: ok (0 baselined, 0 warnings)\n"
        );
        assert!(report.to_text(Lang::Ar).contains("ultar-check"));
        assert!(report.to_text(Lang::De).contains("in Ordnung"));
    }

    #[test]
    fn json_is_valid_and_complete() {
        let value: Value = serde_json::from_str(&sample().to_json(Lang::En)).unwrap();
        assert_eq!(value["passed"], false);
        assert_eq!(value["baselined"], 4);
        let violation = &value["violations"][0];
        assert_eq!(violation["rule"], "unwrap");
        assert_eq!(violation["path"], "a.rs");
        assert_eq!(violation["line"], 3);
        assert_eq!(violation["key"], "PanicCall");
        assert_eq!(violation["args"][0], ".unwrap()");
        assert_eq!(value["warnings"][0]["key"], "FileLong");
        assert_eq!(value["notes"][0]["key"], "BudgetNotBuilt");
        assert!(value["summary"].as_str().unwrap().contains("FAILED"));
    }
}
