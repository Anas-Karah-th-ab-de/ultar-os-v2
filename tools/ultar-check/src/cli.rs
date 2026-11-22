//! Command line parsing.

use crate::error::UsageError;
use crate::msg::Lang;
use crate::rules::Rule;
use std::path::PathBuf;

/// Output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// `path:line: rule: detail` lines.
    Text,
    /// One JSON document.
    Json,
}

/// What the tool should do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run the given rules.
    Check(Vec<Rule>),
    /// Print a baseline that tolerates the current state.
    Baseline,
    /// Print the help text.
    Help,
}

/// Parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    /// Selected command.
    pub command: Command,
    /// Repository root.
    pub root: PathBuf,
    /// Output format.
    pub format: Format,
    /// Baseline file; `None` means `<root>/tools/check-baseline.toml`.
    pub baseline: Option<PathBuf>,
    /// Whether the baseline is applied at all.
    pub use_baseline: bool,
    /// Cargo target directory; `None` means `<root>/target`.
    pub target_dir: Option<PathBuf>,
    /// Output language.
    pub lang: Lang,
}

impl Options {
    /// Options for running `rules` on `root` with all defaults.
    pub fn for_rules(root: impl Into<PathBuf>, rules: &[Rule]) -> Options {
        Options {
            command: Command::Check(rules.to_vec()),
            root: root.into(),
            format: Format::Text,
            baseline: None,
            use_baseline: true,
            target_dir: None,
            lang: Lang::Ar,
        }
    }
}

/// Parses the arguments after the program name.
///
/// `env_lang` is the value of `ULTAR_LANG`, used when `--lang` is absent.
/// Without both, output is Arabic (first language of the system).
pub fn parse(args: &[String], env_lang: Option<&str>) -> Result<Options, UsageError> {
    let mut options = Options::for_rules(".", &[]);
    if let Some(lang) = env_lang.and_then(Lang::from_code) {
        options.lang = lang;
    }
    let mut command = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        let mut value = || {
            iter.next()
                .cloned()
                .ok_or_else(|| UsageError::MissingValue(arg.clone()))
        };
        match arg.as_str() {
            "--help" | "-h" => command = Some(Command::Help),
            "--no-baseline" => options.use_baseline = false,
            "--root" => options.root = PathBuf::from(value()?),
            "--baseline" => options.baseline = Some(PathBuf::from(value()?)),
            "--target-dir" => options.target_dir = Some(PathBuf::from(value()?)),
            "--format" => options.format = parse_format(arg, &value()?)?,
            "--lang" => options.lang = parse_lang(arg, &value()?)?,
            other if other.starts_with('-') => {
                return Err(UsageError::UnknownOption(other.to_string()))
            }
            other => command = Some(merge(command, parse_command(other)?)),
        }
    }
    options.command = command.ok_or(UsageError::MissingCommand)?;
    Ok(options)
}

fn parse_format(option: &str, value: &str) -> Result<Format, UsageError> {
    match value {
        "text" => Ok(Format::Text),
        "json" => Ok(Format::Json),
        _ => Err(invalid(option, value)),
    }
}

fn parse_lang(option: &str, value: &str) -> Result<Lang, UsageError> {
    Lang::from_code(value).ok_or_else(|| invalid(option, value))
}

fn invalid(option: &str, value: &str) -> UsageError {
    UsageError::InvalidValue {
        option: option.to_string(),
        value: value.to_string(),
    }
}

fn parse_command(word: &str) -> Result<Command, UsageError> {
    match word {
        "all" => Ok(Command::Check(Rule::ALL.to_vec())),
        "baseline" => Ok(Command::Baseline),
        "help" => Ok(Command::Help),
        name => Rule::from_name(name)
            .map(|rule| Command::Check(vec![rule]))
            .ok_or_else(|| UsageError::UnknownCommand(name.to_string())),
    }
}

/// Combines several command words: rules accumulate, help wins, otherwise the last one counts.
fn merge(previous: Option<Command>, next: Command) -> Command {
    match (previous, next) {
        (Some(Command::Help), _) => Command::Help,
        (Some(Command::Check(mut rules)), Command::Check(more)) => {
            for rule in more {
                if !rules.contains(&rule) {
                    rules.push(rule);
                }
            }
            rules.sort();
            Command::Check(rules)
        }
        (_, next) => next,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn defaults() {
        let options = parse(&args(&["all"]), None).unwrap();
        assert_eq!(options.command, Command::Check(Rule::ALL.to_vec()));
        assert_eq!(options.root, PathBuf::from("."));
        assert_eq!(options.format, Format::Text);
        assert_eq!(options.baseline, None);
        assert!(options.use_baseline);
        assert_eq!(options.target_dir, None);
        assert_eq!(options.lang, Lang::Ar);
    }

    #[test]
    fn all_options_are_parsed_in_any_position() {
        let options = parse(
            &args(&[
                "--root",
                "/r",
                "paths",
                "--format",
                "json",
                "--baseline",
                "b.toml",
                "--target-dir",
                "/t",
                "--lang",
                "de",
                "--no-baseline",
            ]),
            Some("en"),
        )
        .unwrap();
        assert_eq!(options.command, Command::Check(vec![Rule::Paths]));
        assert_eq!(options.root, PathBuf::from("/r"));
        assert_eq!(options.format, Format::Json);
        assert_eq!(options.baseline, Some(PathBuf::from("b.toml")));
        assert_eq!(options.target_dir, Some(PathBuf::from("/t")));
        assert_eq!(options.lang, Lang::De);
        assert!(!options.use_baseline);
    }

    #[test]
    fn language_comes_from_the_environment_when_not_given() {
        assert_eq!(
            parse(&args(&["all"]), Some("en_US")).unwrap().lang,
            Lang::En
        );
        assert_eq!(parse(&args(&["all"]), Some("xx")).unwrap().lang, Lang::Ar);
    }

    #[test]
    fn several_rules_accumulate_in_rule_order() {
        let options = parse(&args(&["unwrap", "paths", "unwrap"]), None).unwrap();
        assert_eq!(
            options.command,
            Command::Check(vec![Rule::Paths, Rule::Unwrap])
        );
    }

    #[test]
    fn help_and_baseline_commands() {
        assert_eq!(
            parse(&args(&["--help"]), None).unwrap().command,
            Command::Help
        );
        assert_eq!(
            parse(&args(&["-h", "all"]), None).unwrap().command,
            Command::Help
        );
        assert_eq!(
            parse(&args(&["help"]), None).unwrap().command,
            Command::Help
        );
        assert_eq!(
            parse(&args(&["baseline"]), None).unwrap().command,
            Command::Baseline
        );
    }

    #[test]
    fn usage_errors() {
        assert_eq!(parse(&[], None).unwrap_err(), UsageError::MissingCommand);
        assert_eq!(
            parse(&args(&["--root", "x"]), None).unwrap_err(),
            UsageError::MissingCommand
        );
        assert_eq!(
            parse(&args(&["nope"]), None).unwrap_err(),
            UsageError::UnknownCommand("nope".into())
        );
        assert_eq!(
            parse(&args(&["--nope", "all"]), None).unwrap_err(),
            UsageError::UnknownOption("--nope".into())
        );
        assert_eq!(
            parse(&args(&["all", "--root"]), None).unwrap_err(),
            UsageError::MissingValue("--root".into())
        );
        assert_eq!(
            parse(&args(&["all", "--format", "xml"]), None).unwrap_err(),
            invalid("--format", "xml")
        );
        assert_eq!(
            parse(&args(&["all", "--lang", "fr"]), None).unwrap_err(),
            invalid("--lang", "fr")
        );
    }
}
