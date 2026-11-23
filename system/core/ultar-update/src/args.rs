//! Minimal command-line reading shared by the three programs: `--name value` options
//! (repeatable), a fixed set of switches without value, and positional words.

use crate::error::UpdateError;

/// Parsed command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    /// Words that are not options, in order.
    pub positional: Vec<String>,
    options: Vec<(String, String)>,
    switches: Vec<String>,
}

impl Args {
    /// Splits `words`. Names in `switches` (given with their dashes, e.g. `--critical`) take
    /// no value; every other word starting with `--` takes the next word as its value.
    pub fn parse(words: &[String], switches: &[&str]) -> Result<Self, UpdateError> {
        let mut args = Self::default();
        let mut iter = words.iter();
        while let Some(word) = iter.next() {
            if switches.contains(&word.as_str()) {
                args.switches.push(word.clone());
            } else if word.starts_with("--") {
                let value = iter.next().ok_or(UpdateError::InvalidArgument)?;
                args.options.push((word.clone(), value.clone()));
            } else {
                args.positional.push(word.clone());
            }
        }
        Ok(args)
    }

    /// True when the switch was given.
    pub fn switch(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    /// All values of a repeatable option.
    pub fn all(&self, name: &str) -> Vec<&str> {
        self.options
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The value of an option that may be given at most once.
    pub fn one(&self, name: &str) -> Result<Option<&str>, UpdateError> {
        match self.all(name).as_slice() {
            [] => Ok(None),
            [value] => Ok(Some(value)),
            _ => Err(UpdateError::InvalidArgument),
        }
    }

    /// The value of an option that must be given exactly once.
    pub fn required(&self, name: &str) -> Result<&str, UpdateError> {
        self.one(name)?.ok_or(UpdateError::InvalidArgument)
    }

    /// Fails when an option outside `known` was given (typos must not be ignored).
    pub fn only(&self, known: &[&str]) -> Result<(), UpdateError> {
        if self
            .options
            .iter()
            .all(|(n, _)| known.contains(&n.as_str()))
        {
            Ok(())
        } else {
            Err(UpdateError::InvalidArgument)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn options_switches_and_positionals() {
        let args = Args::parse(
            &words("bundle --out x.uup --payload a --critical --payload b tail"),
            &["--critical"],
        )
        .unwrap();
        assert_eq!(args.positional, ["bundle", "tail"]);
        assert_eq!(args.required("--out").unwrap(), "x.uup");
        assert_eq!(args.all("--payload"), ["a", "b"]);
        assert!(args.switch("--critical"));
        assert!(!args.switch("--other"));
        assert_eq!(args.one("--missing").unwrap(), None);
        assert!(args.one("--payload").is_err());
        assert!(args.required("--missing").is_err());
        assert!(args.only(&["--out", "--payload"]).is_ok());
        assert!(args.only(&["--out"]).is_err());
    }

    #[test]
    fn option_without_value_is_an_error() {
        assert!(Args::parse(&words("sign --key"), &[]).is_err());
    }
}
