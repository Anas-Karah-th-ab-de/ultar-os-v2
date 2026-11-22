//! `ultar-busd` – runs the Ultar OS bus broker.
//!
//! ```text
//! ultar-busd [--listen PATH=IDENTITY]... [--policy FILE | --allow-all] [PATH]
//! ```
//!
//! Every `--listen` socket gives all its peers the stated identity; the
//! operating system restricts who can reach each socket file. A bare `PATH`
//! (or the environment variable `ULTAR_BUS_SOCKET`) is shorthand for
//! `--listen PATH=system`.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use ultar_bus::msg::{synopsis, text, Lang, MsgKey};
use ultar_bus::{Broker, FixedIdentity, Identity, Policy};

/// Identity of peers on a socket given without explicit identity.
const DEFAULT_IDENTITY: &str = "system";
/// Environment variable naming the default socket path.
const SOCKET_ENV: &str = "ULTAR_BUS_SOCKET";

/// What the command line asked for.
#[derive(Debug, PartialEq, Eq)]
struct Arguments {
    listeners: Vec<(PathBuf, String)>,
    policy: Option<PathBuf>,
    allow_all: bool,
    help: bool,
}

/// Why the command line was rejected.
#[derive(Debug, PartialEq, Eq)]
enum ArgumentError {
    /// An option needs a value that is missing.
    MissingValue,
    /// An option this program does not know.
    UnknownOption,
    /// `--listen` without `PATH=IDENTITY`.
    BadListener,
    /// No socket was named.
    NoSocket,
}

fn parse_listener(spec: &str) -> Result<(PathBuf, String), ArgumentError> {
    match spec.rsplit_once('=') {
        Some((path, identity)) if !path.is_empty() && !identity.is_empty() => {
            Ok((PathBuf::from(path), identity.to_owned()))
        }
        _ => Err(ArgumentError::BadListener),
    }
}

fn parse_arguments(
    mut args: impl Iterator<Item = String>,
    env_socket: Option<String>,
) -> Result<Arguments, ArgumentError> {
    let mut parsed = Arguments {
        listeners: Vec::new(),
        policy: None,
        allow_all: false,
        help: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => parsed.help = true,
            "--allow-all" => parsed.allow_all = true,
            "--policy" => {
                parsed.policy = Some(PathBuf::from(
                    args.next().ok_or(ArgumentError::MissingValue)?,
                ));
            }
            "--listen" => {
                let spec = args.next().ok_or(ArgumentError::MissingValue)?;
                parsed.listeners.push(parse_listener(&spec)?);
            }
            option if option.starts_with('-') => return Err(ArgumentError::UnknownOption),
            path => parsed
                .listeners
                .push((PathBuf::from(path), DEFAULT_IDENTITY.to_owned())),
        }
    }
    if parsed.listeners.is_empty() {
        if let Some(path) = env_socket.filter(|path| !path.is_empty()) {
            parsed
                .listeners
                .push((PathBuf::from(path), DEFAULT_IDENTITY.to_owned()));
        }
    }
    if parsed.listeners.is_empty() && !parsed.help {
        return Err(ArgumentError::NoSocket);
    }
    Ok(parsed)
}

fn print_usage(lang: Lang) {
    eprintln!("{}\n  {}", text(lang, MsgKey::Usage), synopsis());
}

fn load_policy(arguments: &Arguments, lang: Lang) -> Option<Policy> {
    match (&arguments.policy, arguments.allow_all) {
        (Some(path), _) => match Policy::load(path) {
            Ok(policy) => Some(policy),
            Err(err) => {
                eprintln!(
                    "{} {}: {err}",
                    text(lang, MsgKey::PolicyRejected),
                    path.display()
                );
                None
            }
        },
        (None, true) => {
            eprintln!("{}", text(lang, MsgKey::AllowAllWarning));
            Some(Policy::allow_all())
        }
        (None, false) => {
            eprintln!("{}", text(lang, MsgKey::PolicyRequired));
            None
        }
    }
}

fn start(arguments: &Arguments, policy: Policy, lang: Lang) -> Option<Broker> {
    let mut builder = Broker::builder(policy);
    for (path, identity) in &arguments.listeners {
        let Ok(identity) = Identity::new(identity) else {
            eprintln!("{}", text(lang, MsgKey::BadArguments));
            return None;
        };
        builder = builder.listen(path, Arc::new(FixedIdentity::new(identity)));
    }
    match builder.start() {
        Ok(broker) => Some(broker),
        Err(err) => {
            eprintln!("{} {err}", text(lang, MsgKey::StartFailed));
            None
        }
    }
}

fn main() -> ExitCode {
    let lang = Lang::from_env();
    let arguments = match parse_arguments(std::env::args().skip(1), std::env::var(SOCKET_ENV).ok())
    {
        Ok(arguments) => arguments,
        Err(_) => {
            eprintln!("{}", text(lang, MsgKey::BadArguments));
            print_usage(lang);
            return ExitCode::from(2);
        }
    };
    if arguments.help {
        print_usage(lang);
        return ExitCode::SUCCESS;
    }
    let Some(policy) = load_policy(&arguments, lang) else {
        return ExitCode::from(2);
    };
    let Some(broker) = start(&arguments, policy, lang) else {
        return ExitCode::FAILURE;
    };
    for path in broker.paths() {
        eprintln!("{} {}", text(lang, MsgKey::Listening), path.display());
    }
    // The broker works on its own threads; this one only keeps it alive and
    // never wakes up on its own.
    loop {
        std::thread::park();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str], env: Option<&str>) -> Result<Arguments, ArgumentError> {
        parse_arguments(
            args.iter().map(|arg| (*arg).to_owned()),
            env.map(str::to_owned),
        )
    }

    #[test]
    fn positional_path_gets_the_default_identity() {
        let parsed = parse(&["/tmp/bus.sock", "--allow-all"], None).unwrap();
        assert_eq!(
            parsed.listeners,
            vec![(PathBuf::from("/tmp/bus.sock"), "system".to_owned())]
        );
        assert!(parsed.allow_all);
        assert!(parsed.policy.is_none());
    }

    #[test]
    fn listen_and_policy_options_are_read() {
        let parsed = parse(
            &[
                "--listen",
                "/a/x.sock=ui",
                "--listen",
                "/b=c/y.sock=app.phone",
                "--policy",
                "p.toml",
            ],
            None,
        )
        .unwrap();
        assert_eq!(
            parsed.listeners[0],
            (PathBuf::from("/a/x.sock"), "ui".to_owned())
        );
        assert_eq!(
            parsed.listeners[1],
            (PathBuf::from("/b=c/y.sock"), "app.phone".to_owned())
        );
        assert_eq!(parsed.policy, Some(PathBuf::from("p.toml")));
    }

    #[test]
    fn environment_is_the_fallback_socket() {
        let parsed = parse(&[], Some("/run/bus.sock")).unwrap();
        assert_eq!(parsed.listeners.len(), 1);
        let explicit = parse(&["/explicit.sock"], Some("/run/bus.sock")).unwrap();
        assert_eq!(explicit.listeners[0].0, PathBuf::from("/explicit.sock"));
    }

    #[test]
    fn bad_command_lines_are_rejected() {
        assert_eq!(parse(&[], None), Err(ArgumentError::NoSocket));
        assert_eq!(parse(&[], Some("")), Err(ArgumentError::NoSocket));
        assert_eq!(parse(&["--policy"], None), Err(ArgumentError::MissingValue));
        assert_eq!(parse(&["--listen"], None), Err(ArgumentError::MissingValue));
        assert_eq!(
            parse(&["--listen", "nopath"], None),
            Err(ArgumentError::BadListener)
        );
        assert_eq!(
            parse(&["--listen", "=id"], None),
            Err(ArgumentError::BadListener)
        );
        assert_eq!(
            parse(&["--listen", "/p="], None),
            Err(ArgumentError::BadListener)
        );
        assert_eq!(
            parse(&["--frobnicate"], None),
            Err(ArgumentError::UnknownOption)
        );
    }

    #[test]
    fn help_needs_no_socket() {
        assert!(parse(&["--help"], None).unwrap().help);
        assert!(parse(&["-h"], None).unwrap().help);
    }
}
