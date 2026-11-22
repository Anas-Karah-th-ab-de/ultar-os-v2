//! Typed errors of the tool (I/O, configuration and command line).

use crate::finding::Message;
use crate::msg::{Key, Lang};
use std::fmt;

/// A problem in a configuration or baseline file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigIssue {
    /// A baseline entry names a rule that does not exist.
    UnknownRule(String),
    /// The same rule + path pair is listed twice in the baseline.
    DuplicateEntry {
        /// Rule name of the duplicate.
        rule: String,
        /// Path of the duplicate.
        path: String,
    },
    /// An allow entry references an area that is not defined.
    UnknownArea(String),
    /// Two areas share one name.
    DuplicateArea(String),
}

/// A problem with the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsageError {
    /// No command was given.
    MissingCommand,
    /// The command is not known.
    UnknownCommand(String),
    /// The option is not known.
    UnknownOption(String),
    /// The option needs a value but none followed.
    MissingValue(String),
    /// The value is not valid for the option.
    InvalidValue {
        /// Option name.
        option: String,
        /// Rejected value.
        value: String,
    },
}

impl UsageError {
    /// Message key and arguments describing this error.
    pub fn message(&self) -> Message {
        match self {
            UsageError::MissingCommand => Message::plain(Key::ErrMissingCommand),
            UsageError::UnknownCommand(name) => Message::new(Key::ErrUnknownCommand, &[name]),
            UsageError::UnknownOption(name) => Message::new(Key::ErrUnknownOption, &[name]),
            UsageError::MissingValue(name) => Message::new(Key::ErrMissingValue, &[name]),
            UsageError::InvalidValue { option, value } => {
                Message::new(Key::ErrInvalidValue, &[option, value])
            }
        }
    }
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message().render(Lang::En))
    }
}

impl std::error::Error for UsageError {}

/// Any error that stops a check run (exit code 2).
#[derive(Debug)]
pub enum CheckError {
    /// A file or directory could not be read.
    Io {
        /// Path that failed.
        path: String,
        /// Underlying error.
        source: std::io::Error,
    },
    /// A TOML file could not be parsed.
    Toml {
        /// Path of the file.
        path: String,
        /// Underlying parse error.
        source: toml::de::Error,
    },
    /// A configuration file is well-formed but inconsistent.
    Config {
        /// Path of the file.
        path: String,
        /// What is wrong.
        issue: ConfigIssue,
    },
    /// The command line is invalid.
    Usage(UsageError),
}

impl CheckError {
    /// Message key and arguments describing this error.
    pub fn message(&self) -> Message {
        match self {
            CheckError::Io { path, source } => {
                Message::new(Key::ErrIo, &[path, &source.to_string()])
            }
            CheckError::Toml { path, source } => {
                Message::new(Key::ErrToml, &[path, source.message()])
            }
            CheckError::Config { path, issue } => match issue {
                ConfigIssue::UnknownRule(rule) => Message::new(Key::ErrUnknownRule, &[path, rule]),
                ConfigIssue::DuplicateEntry { rule, path: entry } => {
                    Message::new(Key::ErrDuplicateEntry, &[path, rule, entry])
                }
                ConfigIssue::UnknownArea(area) => Message::new(Key::ErrUnknownArea, &[path, area]),
                ConfigIssue::DuplicateArea(area) => {
                    Message::new(Key::ErrDuplicateArea, &[path, area])
                }
            },
            CheckError::Usage(usage) => usage.message(),
        }
    }
}

impl fmt::Display for CheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message().render(Lang::En))
    }
}

impl std::error::Error for CheckError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            CheckError::Io { source, .. } => Some(source),
            CheckError::Toml { source, .. } => Some(source),
            CheckError::Usage(usage) => Some(usage),
            CheckError::Config { .. } => None,
        }
    }
}

impl From<UsageError> for CheckError {
    fn from(error: UsageError) -> Self {
        CheckError::Usage(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn usage_errors_render_in_english() {
        let error = UsageError::InvalidValue {
            option: "--format".into(),
            value: "xml".into(),
        };
        assert_eq!(
            error.to_string(),
            "invalid value `xml` for option `--format`"
        );
        assert_eq!(
            UsageError::MissingCommand.to_string(),
            "no command given (try --help)"
        );
        assert!(UsageError::UnknownCommand("x".into())
            .to_string()
            .contains("`x`"));
        assert!(UsageError::UnknownOption("--y".into())
            .to_string()
            .contains("--y"));
        assert!(UsageError::MissingValue("--root".into())
            .to_string()
            .contains("--root"));
    }

    #[test]
    fn check_errors_have_sources_and_text() {
        let io = CheckError::Io {
            path: "a/b".into(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "gone"),
        };
        assert!(io.to_string().starts_with("cannot access a/b"));
        assert!(io.source().is_some());

        let toml_error = "a = ".parse::<toml::Table>().unwrap_err();
        let parse = CheckError::Toml {
            path: "x.toml".into(),
            source: toml_error,
        };
        assert!(parse.to_string().contains("x.toml"));
        assert!(parse.source().is_some());

        let usage: CheckError = UsageError::MissingCommand.into();
        assert!(usage.source().is_some());
    }

    #[test]
    fn config_issues_name_the_file() {
        let issues = [
            ConfigIssue::UnknownRule("nope".into()),
            ConfigIssue::DuplicateEntry {
                rule: "size".into(),
                path: "a.rs".into(),
            },
            ConfigIssue::UnknownArea("moon".into()),
            ConfigIssue::DuplicateArea("apps".into()),
        ];
        for issue in issues {
            let error = CheckError::Config {
                path: "tools/cfg.toml".into(),
                issue,
            };
            assert!(error.to_string().starts_with("tools/cfg.toml: "));
            assert!(error.source().is_none());
        }
    }
}
