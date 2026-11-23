//! `uinit` – status and control of the services over the bus (service
//! `init`, administrators only).
//!
//! ```text
//! uinit [--bus PATH] [--token-file FILE | --token-fd N] [--lang ar|en|de]
//!       (status | start SERVICE | stop SERVICE | restart SERVICE)
//! ```
//!
//! Exit codes: 0 done, 1 refused or failed, 2 invalid arguments.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;
use ultar_bus::messages::init::{
    Action, Control, Status, ERROR_ALREADY, ERROR_SHUTTING_DOWN, ERROR_UNKNOWN_SERVICE,
};
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode};
use ultar_init::msg::{language, text, Lang, Msg, UINIT_SYNOPSIS};
use ultar_init::supervisor::exit_text;
use ultar_service::{read_secret_source, SecretSource};

/// The administrator's bus socket.
const DEFAULT_BUS: &str = "/run/ultar/bus/system.sock";
/// How long an answer may take.
const WAIT: Duration = Duration::from_secs(10);
/// Largest token read.
const MAX_TOKEN: usize = 4096;

/// The command line.
#[derive(Debug, PartialEq, Eq)]
struct Args {
    bus: PathBuf,
    token: Option<SecretSource>,
    command: Command,
}

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Status,
    Control(Action, String),
}

fn parse(words: &[String]) -> Option<Args> {
    let mut bus = PathBuf::from(DEFAULT_BUS);
    let mut token = None;
    let mut rest = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--bus" => bus = PathBuf::from(words.next()?),
            "--lang" => {
                words.next()?;
            }
            "--token-file" | "--token-fd" if token.is_some() => return None,
            "--token-file" => token = Some(SecretSource::File(PathBuf::from(words.next()?))),
            "--token-fd" => match words.next()?.parse::<i32>() {
                Ok(fd) if fd >= 3 => token = Some(SecretSource::Fd(fd)),
                _ => return None,
            },
            option if option.starts_with("--") => return None,
            other => rest.push(other.to_owned()),
        }
    }
    let command = match rest.as_slice() {
        [status] if status == "status" => Command::Status,
        [action, service] => {
            let action = match action.as_str() {
                "start" => Action::Start,
                "stop" => Action::Stop,
                "restart" => Action::Restart,
                _ => return None,
            };
            Command::Control(action, service.clone())
        }
        _ => return None,
    };
    Some(Args {
        bus,
        token,
        command,
    })
}

fn say(line: &str) {
    let _ = writeln!(std::io::stderr(), "{line}");
}

fn failure(lang: Lang, error: &BusError) -> ExitCode {
    match error {
        BusError::Remote(ErrorCode::PermissionDenied) => say(text(lang, Msg::NotPermitted)),
        BusError::Remote(ErrorCode::Service(ERROR_UNKNOWN_SERVICE)) => {
            say(text(lang, Msg::UnknownService))
        }
        BusError::Remote(ErrorCode::Service(ERROR_ALREADY)) => say(text(lang, Msg::Already)),
        BusError::Remote(ErrorCode::Service(ERROR_SHUTTING_DOWN)) => {
            say(text(lang, Msg::ShuttingDown))
        }
        other => say(&format!("{} {other}", text(lang, Msg::BusFailed))),
    }
    ExitCode::from(1)
}

fn connect(args: &Args, lang: Lang) -> Result<Client, ExitCode> {
    let mut options = ClientOptions::named("uinit");
    if let Some(source) = &args.token {
        match read_secret_source(source, MAX_TOKEN) {
            Ok(secret) => options.token = secret.expose_str().map(str::to_owned),
            Err(error) => {
                say(&format!("{} {error}", text(lang, Msg::TokenRejected)));
                return Err(ExitCode::from(1));
            }
        }
    }
    Client::connect(&args.bus, options).map_err(|error| failure(lang, &error))
}

fn run(args: &Args, lang: Lang) -> ExitCode {
    let client = match connect(args, lang) {
        Ok(client) => client,
        Err(code) => return code,
    };
    let mut out = std::io::stdout();
    match &args.command {
        Command::Status => match client.call(&Status {}, WAIT) {
            Ok(reply) => {
                let _ = writeln!(out, "{}", text(lang, Msg::StatusHeader));
                for service in &reply.services {
                    let _ = writeln!(
                        out,
                        "{}\t{}\t{}\t{}\t{}\t{}",
                        service.name,
                        service.state.word(),
                        service.pid,
                        service.restarts,
                        service.uid,
                        exit_text(service)
                    );
                }
                ExitCode::SUCCESS
            }
            Err(error) => failure(lang, &error),
        },
        Command::Control(action, service) => {
            let request = Control {
                action: *action,
                service: service.clone(),
            };
            match client.call(&request, WAIT) {
                Ok(reply) => {
                    let _ = writeln!(out, "{} {}", text(lang, Msg::Accepted), reply.state.word());
                    ExitCode::SUCCESS
                }
                Err(error) => failure(lang, &error),
            }
        }
    }
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let lang = language(&words);
    match parse(&words) {
        Some(args) => run(&args, lang),
        None => {
            say(text(lang, Msg::BadArguments));
            say(&format!("{}\n  {UINIT_SYNOPSIS}", text(lang, Msg::Usage)));
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn command_lines_are_read_strictly() {
        let status = parse(&words("--lang de status")).unwrap();
        assert_eq!(status.command, Command::Status);
        assert_eq!(status.bus, PathBuf::from(DEFAULT_BUS));
        let restart = parse(&words("--bus /b --token-fd 3 restart logd")).unwrap();
        assert_eq!(
            restart.command,
            Command::Control(Action::Restart, "logd".into())
        );
        assert_eq!(restart.token, Some(SecretSource::Fd(3)));
        for bad in [
            "",
            "status extra",
            "frob logd",
            "start",
            "--token x status",
            "--token-fd 2 status",
            "--token-fd 3 --token-file /t status",
            "--bus",
        ] {
            assert_eq!(parse(&words(bad)), None, "{bad:?}");
        }
    }
}
