//! The `usettings` command: get, set, reset, list, watch, confirm and
//! revert settings over the bus. Every line it prints comes from an i18n
//! key of this crate's catalogs.

use crate::error::message_key_of;
use crate::schema::{Registry, Risk, ValueError};
use crate::value::Value;
use std::fmt;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::settings::{
    Changed, Confirm, Get, ItemInfo, ListRequest, Reset, Revert, Set, SettingValue,
};
use ultar_bus::{BusError, Client, ErrorCode};
use ultar_i18n::{Arg, Lang, Translator};

/// Longest wait for an answer of the service.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Environment variable naming the bus socket.
pub const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";

/// What the user asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Print the usage line.
    Help,
    /// Print the value of a setting.
    Get(String),
    /// Change a setting; the value is still text.
    Set(String, String),
    /// Return a setting to its default.
    Reset(String),
    /// List the visible panels and items, or one panel.
    List(Option<String>),
    /// Print changes as they happen, optionally only the first `n`.
    Watch(Option<u64>),
    /// Keep one or all provisional changes.
    Confirm(Option<String>),
    /// Take one or all provisional changes back.
    Revert(Option<String>),
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invocation {
    /// Bus socket (option or environment).
    pub socket: Option<PathBuf>,
    /// Output language when given explicitly.
    pub lang: Option<Lang>,
    /// File with the secret for token listeners (never the secret itself).
    pub token_file: Option<PathBuf>,
    /// The command.
    pub command: Command,
}

/// Why a command line was not understood.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UsageError {
    /// Missing or surplus words, or an unknown command.
    Arguments,
    /// An option this program does not know.
    UnknownOption(String),
}

impl fmt::Display for UsageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UsageError::Arguments => f.write_str("invalid command"),
            UsageError::UnknownOption(option) => write!(f, "unknown option {option}"),
        }
    }
}

impl std::error::Error for UsageError {}

fn command_of(words: &[String]) -> Result<Command, UsageError> {
    let word = |index: usize| words.get(index).cloned();
    let optional = |limit: usize| (words.len() <= limit).then(|| word(1));
    let command = match (words.first().map(String::as_str), words.len()) {
        (Some("help"), 1) | (None, _) => Some(Command::Help),
        (Some("get"), 2) => word(1).map(Command::Get),
        (Some("set"), 3) => word(1)
            .zip(word(2))
            .map(|(key, value)| Command::Set(key, value)),
        (Some("reset"), 2) => word(1).map(Command::Reset),
        (Some("list"), _) => optional(2).map(Command::List),
        (Some("confirm"), _) => optional(2).map(Command::Confirm),
        (Some("revert"), _) => optional(2).map(Command::Revert),
        (Some("watch"), 1) => Some(Command::Watch(None)),
        _ => None,
    };
    command.ok_or(UsageError::Arguments)
}

/// The language named by `--lang`, found without parsing anything else:
/// lets a command line that is otherwise wrong be answered in the language
/// it asked for.
pub fn lang_hint(args: &[String]) -> Option<Lang> {
    let position = args.iter().position(|arg| arg == "--lang")?;
    Lang::parse(args.get(position + 1)?)
}

/// Parses the words after the program name.
pub fn parse(
    args: impl IntoIterator<Item = String>,
    env_socket: Option<String>,
) -> Result<Invocation, UsageError> {
    let mut args = args.into_iter();
    let mut socket = env_socket
        .filter(|path| !path.is_empty())
        .map(PathBuf::from);
    let (mut lang, mut token_file, mut count, mut help) = (None, None, None, false);
    let mut words = Vec::new();
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or(UsageError::Arguments);
        match arg.as_str() {
            "--help" | "-h" => help = true,
            "--socket" => socket = Some(PathBuf::from(value()?)),
            "--token-file" => token_file = Some(PathBuf::from(value()?)),
            "--lang" => lang = Some(Lang::parse(&value()?).ok_or(UsageError::Arguments)?),
            "--count" => {
                let number: u64 = value()?.parse().map_err(|_| UsageError::Arguments)?;
                count = Some(number).filter(|n| *n > 0);
                if count.is_none() {
                    return Err(UsageError::Arguments);
                }
            }
            option if option.starts_with("--") => {
                return Err(UsageError::UnknownOption(option.to_owned()))
            }
            _ => words.push(arg),
        }
    }
    let command = match command_of(&words)? {
        _ if help => Command::Help,
        Command::Watch(_) => Command::Watch(count),
        _ if count.is_some() => return Err(UsageError::Arguments),
        other => other,
    };
    Ok(Invocation {
        socket,
        lang,
        token_file,
        command,
    })
}

/// Why a command failed.
#[derive(Debug)]
pub enum CliError {
    /// The key is not declared (found locally, before asking the service).
    UnknownKey,
    /// The value does not fit the setting (found locally).
    Value(ValueError),
    /// The bus or the service refused.
    Bus(BusError),
    /// The result could not be printed.
    Output(io::ErrorKind),
}

impl CliError {
    /// Key of the message for the user.
    pub fn message_key(&self) -> &'static str {
        match self {
            CliError::UnknownKey => "settingsd.error.unknown_key",
            CliError::Value(error) => error.message_key(),
            CliError::Bus(BusError::Remote(code)) => message_key_of(*code),
            CliError::Bus(BusError::Timeout) => message_key_of(ErrorCode::ServiceGone),
            CliError::Bus(_) => "settingsd.error.bus",
            CliError::Output(_) => "usettings.error.output",
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CliError::UnknownKey => f.write_str("unknown setting"),
            CliError::Value(error) => write!(f, "{error}"),
            CliError::Bus(error) => write!(f, "{error}"),
            CliError::Output(kind) => write!(f, "output failed: {kind}"),
        }
    }
}

impl std::error::Error for CliError {}

impl From<BusError> for CliError {
    fn from(error: BusError) -> Self {
        CliError::Bus(error)
    }
}

impl From<io::Error> for CliError {
    fn from(error: io::Error) -> Self {
        CliError::Output(error.kind())
    }
}

/// Everything a command needs to run.
pub struct Session<'a> {
    /// Connection to the bus.
    pub bus: &'a Client,
    /// The declared settings (to read typed values from text).
    pub registry: &'a Registry,
    /// Translator with this crate's catalogs.
    pub translator: &'a Translator,
    /// Where the result goes.
    pub out: &'a mut dyn Write,
}

fn shown(value: SettingValue) -> String {
    Value::from(value).to_string()
}

impl Session<'_> {
    fn line(&mut self, key: &str, args: &[(&str, Arg<'_>)]) -> Result<(), CliError> {
        let text = self.translator.t_args(key, args);
        writeln!(self.out, "{text}")?;
        Ok(self.out.flush()?)
    }

    fn get(&mut self, key: &str) -> Result<(), CliError> {
        let reply = self.bus.call(
            &Get {
                key: key.to_owned(),
            },
            CALL_TIMEOUT,
        )?;
        let value = reply.value.map(shown).unwrap_or_default();
        self.line(
            "usettings.get",
            &[("key", Arg::Str(key)), ("value", Arg::Str(&value))],
        )
    }

    fn set(&mut self, key: &str, text: &str) -> Result<(), CliError> {
        let found = self
            .registry
            .resolve(key)
            .map_err(|_| CliError::UnknownKey)?;
        let value = found
            .def
            .value_type
            .parse_text(text)
            .map_err(CliError::Value)?;
        let shown_value = value.to_string();
        let call = Set {
            key: key.to_owned(),
            value: value.into(),
        };
        let reply = self.bus.call(&call, CALL_TIMEOUT)?;
        let args = [("key", Arg::Str(key)), ("value", Arg::Str(&shown_value))];
        if !reply.changed {
            return self.line("usettings.set.unchanged", &args);
        }
        self.line("usettings.set.changed", &args)?;
        if let Risk::ConfirmRevert { seconds } = found.def.risk {
            let args = [
                ("key", Arg::Str(key)),
                ("seconds", Arg::Int(i64::from(seconds))),
            ];
            self.line("usettings.set.pending", &args)?;
        }
        Ok(())
    }

    fn reset(&mut self, key: &str) -> Result<(), CliError> {
        let reply = self.bus.call(
            &Reset {
                key: key.to_owned(),
            },
            CALL_TIMEOUT,
        )?;
        let message = if reply.changed {
            "usettings.reset.changed"
        } else {
            "usettings.reset.unchanged"
        };
        self.line(message, &[("key", Arg::Str(key))])
    }

    fn item(&mut self, item: ItemInfo) -> Result<(), CliError> {
        let message = if item.pending {
            "usettings.list.item_pending"
        } else if !item.writable {
            "usettings.list.item_readonly"
        } else {
            "usettings.list.item"
        };
        let label = self.translator.t(&item.label.key).into_owned();
        let value = shown(item.value);
        let args = [
            ("key", Arg::Str(&item.key)),
            ("value", Arg::Str(&value)),
            ("label", Arg::Str(&label)),
        ];
        self.line(message, &args)
    }

    fn list(&mut self, panel: Option<&str>) -> Result<(), CliError> {
        let request = ListRequest {
            panel: panel.map(str::to_owned),
        };
        let reply = self.bus.call(&request, CALL_TIMEOUT)?;
        if reply.panels.is_empty() {
            return self.line("usettings.list.empty", &[]);
        }
        for panel in reply.panels {
            let title = self.translator.t(&panel.title.key).into_owned();
            self.line("usettings.list.panel", &[("title", Arg::Str(&title))])?;
            for item in panel.items {
                self.item(item)?;
            }
        }
        Ok(())
    }

    fn watch(&mut self, count: Option<u64>) -> Result<(), CliError> {
        let changes = self.bus.subscribe::<Changed>()?;
        self.line("usettings.watch.ready", &[])?;
        let mut seen = 0;
        while count.is_none_or(|count| seen < count) {
            let change = match changes.recv() {
                Ok(change) => change,
                Err(BusError::Lagged(_)) => continue,
                Err(error) => return Err(error.into()),
            };
            let value = shown(change.value);
            let args = [("key", Arg::Str(&change.key)), ("value", Arg::Str(&value))];
            self.line("usettings.watch.changed", &args)?;
            seen += 1;
        }
        Ok(())
    }

    fn resolve_pending(&mut self, key: Option<&str>, keep: bool) -> Result<(), CliError> {
        let key = key.map(str::to_owned);
        let (reply, message) = if keep {
            (
                self.bus.call(&Confirm { key }, CALL_TIMEOUT)?,
                "usettings.confirm.done",
            )
        } else {
            (
                self.bus.call(&Revert { key }, CALL_TIMEOUT)?,
                "usettings.revert.done",
            )
        };
        self.line(message, &[("count", Arg::Int(i64::from(reply.resolved)))])
    }

    /// Runs one command and prints its result.
    pub fn run(&mut self, command: &Command) -> Result<(), CliError> {
        match command {
            Command::Help => self.line("usettings.usage", &[]),
            Command::Get(key) => self.get(key),
            Command::Set(key, value) => self.set(key, value),
            Command::Reset(key) => self.reset(key),
            Command::List(panel) => self.list(panel.as_deref()),
            Command::Watch(count) => self.watch(*count),
            Command::Confirm(key) => self.resolve_pending(key.as_deref(), true),
            Command::Revert(key) => self.resolve_pending(key.as_deref(), false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<Invocation, UsageError> {
        parse(words.iter().map(|word| (*word).to_owned()), None)
    }

    fn command(words: &[&str]) -> Command {
        parsed(words).unwrap().command
    }

    #[test]
    fn commands() {
        assert_eq!(command(&[]), Command::Help);
        assert_eq!(command(&["help"]), Command::Help);
        assert_eq!(command(&["get", "a.b", "--help"]), Command::Help);
        assert_eq!(command(&["get", "a.b"]), Command::Get("a.b".into()));
        assert_eq!(
            command(&["set", "a.b", "on"]),
            Command::Set("a.b".into(), "on".into())
        );
        assert_eq!(command(&["reset", "a.b"]), Command::Reset("a.b".into()));
        assert_eq!(command(&["list"]), Command::List(None));
        assert_eq!(
            command(&["list", "power"]),
            Command::List(Some("power".into()))
        );
        assert_eq!(command(&["watch"]), Command::Watch(None));
        assert_eq!(command(&["watch", "--count", "3"]), Command::Watch(Some(3)));
        assert_eq!(command(&["--count", "3", "watch"]), Command::Watch(Some(3)));
        assert_eq!(command(&["confirm"]), Command::Confirm(None));
        assert_eq!(
            command(&["revert", "a.b"]),
            Command::Revert(Some("a.b".into()))
        );
    }

    #[test]
    fn options() {
        let invocation = parsed(&["--socket", "/tmp/s", "--lang", "de", "list"]).unwrap();
        assert_eq!(invocation.socket, Some(PathBuf::from("/tmp/s")));
        assert_eq!(invocation.lang, Some(Lang::De));
        let with_file = parsed(&["--token-file", "/t", "list"]).unwrap();
        assert_eq!(with_file.token_file, Some(PathBuf::from("/t")));
        assert!(parsed(&["--token", "t", "list"]).is_err());
        let env = |socket: &str| parse(["list".to_owned()], Some(socket.to_owned())).unwrap();
        assert_eq!(env("/run/bus").socket, Some(PathBuf::from("/run/bus")));
        assert_eq!(env("").socket, None);
        assert_eq!(parsed(&["list"]).unwrap().lang, None);
    }

    #[test]
    fn language_hint_survives_a_broken_command_line() {
        let words = |list: &[&str]| list.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        assert_eq!(
            lang_hint(&words(&["--lang", "en", "frobnicate"])),
            Some(Lang::En)
        );
        assert_eq!(
            lang_hint(&words(&["x", "--frob", "--lang", "de"])),
            Some(Lang::De)
        );
        assert_eq!(lang_hint(&words(&["--lang"])), None);
        assert_eq!(lang_hint(&words(&["--lang", "xx"])), None);
        assert_eq!(lang_hint(&words(&["list"])), None);
    }

    #[test]
    fn usage_errors() {
        for words in [
            &["get"][..],
            &["get", "a", "b"],
            &["set", "a"],
            &["set", "a", "b", "c"],
            &["reset"],
            &["list", "a", "b"],
            &["watch", "x"],
            &["frobnicate"],
            &["--socket"],
            &["--lang", "xx", "list"],
            &["--count", "0", "watch"],
            &["--count", "x", "watch"],
            &["--count", "2", "list"],
        ] {
            assert_eq!(parsed(words), Err(UsageError::Arguments), "{words:?}");
        }
        assert_eq!(
            parsed(&["--frob", "list"]),
            Err(UsageError::UnknownOption("--frob".into()))
        );
        assert!(!UsageError::Arguments.to_string().is_empty());
        assert!(UsageError::UnknownOption("--x".into())
            .to_string()
            .contains("--x"));
    }

    #[test]
    fn errors_map_to_message_keys() {
        let cases = [
            (CliError::UnknownKey, "settingsd.error.unknown_key"),
            (
                CliError::Value(ValueError::OutOfRange),
                "settingsd.error.out_of_range",
            ),
            (
                CliError::Bus(BusError::Remote(ErrorCode::PermissionDenied)),
                "settingsd.error.not_permitted",
            ),
            (
                CliError::Bus(BusError::Timeout),
                "settingsd.error.no_service",
            ),
            (CliError::Bus(BusError::Closed), "settingsd.error.bus"),
            (
                CliError::from(io::Error::from(io::ErrorKind::BrokenPipe)),
                "usettings.error.output",
            ),
        ];
        let translator = crate::i18n::translator(Lang::Ar).unwrap();
        for (error, key) in cases {
            assert_eq!(error.message_key(), key);
            assert!(translator.has(key), "{key}");
            assert!(!error.to_string().is_empty());
        }
    }
}
