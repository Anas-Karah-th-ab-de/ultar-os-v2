//! Findings and messages produced by the rules.

use crate::msg::{self, Key, Lang};
use crate::rules::Rule;

/// A message key together with its positional arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message {
    /// Which message.
    pub key: Key,
    /// Values for `{0}`, `{1}` ...
    pub args: Vec<String>,
}

impl Message {
    /// Creates a message from a key and its arguments.
    pub fn new<S: AsRef<str>>(key: Key, args: &[S]) -> Message {
        Message {
            key,
            args: args.iter().map(|a| a.as_ref().to_string()).collect(),
        }
    }

    /// Creates a message without arguments.
    pub fn plain(key: Key) -> Message {
        Message {
            key,
            args: Vec::new(),
        }
    }

    /// Renders the message in the given language.
    pub fn render(&self, lang: Lang) -> String {
        msg::render(lang, self.key, &self.args)
    }
}

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Fails the check (unless covered by the baseline).
    Error,
    /// Reported but never fails the check.
    Warning,
}

/// One rule violation at a place in the repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// Rule that produced the finding.
    pub rule: Rule,
    /// Error or warning.
    pub severity: Severity,
    /// Repository-relative path with `/` separators (binary name for `budget`).
    pub path: String,
    /// 1-based line; 1 for whole-file findings, 0 when there is no line.
    pub line: u32,
    /// What is wrong.
    pub message: Message,
}

impl Finding {
    /// Creates an error finding.
    pub fn error(rule: Rule, path: &str, line: u32, message: Message) -> Finding {
        Finding {
            rule,
            severity: Severity::Error,
            path: path.to_string(),
            line,
            message,
        }
    }

    /// Creates a warning finding.
    pub fn warning(rule: Rule, path: &str, line: u32, message: Message) -> Finding {
        Finding {
            severity: Severity::Warning,
            ..Finding::error(rule, path, line, message)
        }
    }

    /// Formats the finding as `path:line: rule: detail`.
    pub fn to_line(&self, lang: Lang) -> String {
        let detail = self.message.render(lang);
        match self.severity {
            Severity::Error => {
                format!(
                    "{}:{}: {}: {}",
                    self.path,
                    self.line,
                    self.rule.name(),
                    detail
                )
            }
            Severity::Warning => format!(
                "{}:{}: {}: {}: {}",
                self.path,
                self.line,
                self.rule.name(),
                msg::render(lang, Key::LabelWarning, &[]),
                detail
            ),
        }
    }
}

/// Shortens a literal for display in a finding.
pub fn excerpt(text: &str) -> String {
    const MAX_CHARS: usize = 48;
    let flat: String = text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if flat.chars().count() <= MAX_CHARS {
        return flat;
    }
    let mut short: String = flat.chars().take(MAX_CHARS).collect();
    short.push('…');
    short
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_line_has_the_documented_shape() {
        let finding = Finding::error(
            Rule::Size,
            "apps/a/src/lib.rs",
            7,
            Message::new(Key::FileTooLong, &["700", "600"]),
        );
        assert_eq!(
            finding.to_line(Lang::En),
            "apps/a/src/lib.rs:7: size: file too long: 700 lines (max 600)"
        );
    }

    #[test]
    fn warning_line_carries_a_label() {
        let finding = Finding::warning(
            Rule::Size,
            "a.rs",
            1,
            Message::new(Key::FileLong, &["450", "400"]),
        );
        assert_eq!(finding.severity, Severity::Warning);
        assert!(finding
            .to_line(Lang::En)
            .starts_with("a.rs:1: size: warning: "));
    }

    #[test]
    fn excerpt_shortens_and_flattens() {
        assert_eq!(excerpt("a\nb"), "a b");
        let long = "x".repeat(100);
        let short = excerpt(&long);
        assert_eq!(short.chars().count(), 49);
        assert!(short.ends_with('…'));
    }
}
