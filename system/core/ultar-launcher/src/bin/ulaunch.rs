//! `ulaunch` – the administrator's tool for the app launcher (and the
//! permission prompts' stand-in until the shell exists).
//!
//! ```text
//! ulaunch [--bus PATH] [--lang ar|en|de] list
//!         | start APP [--service] [--token N] | stop APP [--service]
//!         | permissions APP | grant APP PERMISSION | revoke APP PERMISSION
//!         | install FILE.uap | uninstall APP [--keep-data]
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
use ultar_bus::messages::launcher::{
    AppStatus, Exit, Grant, Install, List, Permissions, Revoke, Role, Start, Stop, Uninstall,
};
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode};
use ultar_launcher::msg::{for_code, language, text, Lang, Msg, SYNOPSIS};

/// The administrator's bus socket.
const DEFAULT_BUS: &str = "/run/ultar/bus/system.sock";
/// How long an answer may take (a start waits for its cell).
const WAIT: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
enum Command {
    List,
    Start(String, Role, Option<u64>),
    Stop(String, Role),
    Permissions(String),
    Grant(String, String),
    Revoke(String, String),
    Install(String),
    Uninstall(String, bool),
}

fn command(rest: &[&str], service: bool, token: Option<u64>, keep: bool) -> Option<Command> {
    let role = if service { Role::Service } else { Role::Ui };
    let owned = |s: &&str| (*s).to_owned();
    Some(match rest {
        ["list"] => Command::List,
        ["start", app] => Command::Start(owned(app), role, token),
        ["stop", app] => Command::Stop(owned(app), role),
        ["permissions", app] => Command::Permissions(owned(app)),
        ["grant", app, p] => Command::Grant(owned(app), owned(p)),
        ["revoke", app, p] => Command::Revoke(owned(app), owned(p)),
        ["install", file] => Command::Install(owned(file)),
        ["uninstall", app] => Command::Uninstall(owned(app), keep),
        _ => return None,
    })
}

fn parse(words: &[String]) -> Option<(PathBuf, Command)> {
    let mut bus = PathBuf::from(DEFAULT_BUS);
    let (mut service, mut token, mut keep) = (false, None, false);
    let mut rest = Vec::new();
    let mut words = words.iter();
    while let Some(word) = words.next() {
        match word.as_str() {
            "--bus" => bus = PathBuf::from(words.next()?),
            "--lang" => {
                words.next()?;
            }
            "--service" => service = true,
            "--keep-data" => keep = true,
            "--token" => token = Some(words.next()?.parse::<u64>().ok().filter(|t| *t != 0)?),
            option if option.starts_with("--") => return None,
            other => rest.push(other),
        }
    }
    let command = command(&rest, service, token, keep)?;
    let allowed = match command {
        Command::Start(..) | Command::Stop(..) => !keep,
        Command::Uninstall(..) => !service && token.is_none(),
        _ => !service && token.is_none() && !keep,
    };
    allowed.then_some((bus, command))
}

fn exit_text(exit: Option<Exit>) -> String {
    match exit {
        None => "-".into(),
        Some(Exit::Code(code)) => format!("code {code}"),
        Some(Exit::Signal(signal)) => format!("signal {signal}"),
        Some(Exit::Setup(code)) => format!("setup {code}"),
    }
}

fn print_list(lang: Lang, apps: &[AppStatus]) {
    let mut out = std::io::stdout();
    let _ = writeln!(out, "{}", text(lang, Msg::ListHeader));
    for app in apps {
        let head = format!("{}\t{}\t{}\t{}", app.app, app.uid, app.profile, app.version);
        if app.instances.is_empty() {
            let _ = writeln!(out, "{head}\t-\tstopped\t0\t0\t-");
        }
        for i in &app.instances {
            let _ = writeln!(
                out,
                "{head}\t{}\t{}\t{}\t{}\t{}",
                i.role.word(),
                i.state.word(),
                i.pid,
                i.restarts,
                exit_text(i.last_exit)
            );
        }
    }
}

fn done(lang: Lang, detail: &str) -> ExitCode {
    let _ = writeln!(std::io::stdout(), "{} {detail}", text(lang, Msg::Done));
    ExitCode::SUCCESS
}

fn failure(lang: Lang, error: &BusError) -> ExitCode {
    let line = match error {
        BusError::Remote(ErrorCode::PermissionDenied) => text(lang, Msg::NotPermitted).to_owned(),
        BusError::Remote(ErrorCode::Service(code)) => {
            format!("{} ({code})", text(lang, for_code(*code)))
        }
        other => format!("{} {other}", text(lang, Msg::BusFailed)),
    };
    let _ = writeln!(std::io::stderr(), "{line}");
    ExitCode::from(1)
}

fn start(
    client: &Client,
    lang: Lang,
    app: String,
    role: Role,
    activation_token: Option<u64>,
) -> Result<ExitCode, BusError> {
    let start = Start {
        app,
        role,
        activation_token,
        uri: None,
        new_window: false,
    };
    let r = client.call(&start, WAIT)?;
    let line = format!(
        "{} pid={} already={} opened={}",
        r.state.word(),
        r.pid,
        r.already,
        r.opened
    );
    Ok(done(lang, &line))
}

fn permissions(client: &Client, app: String) -> Result<ExitCode, BusError> {
    let r = client.call(&Permissions { app }, WAIT)?;
    let mut out = std::io::stdout();
    for p in &r.declared {
        let _ = writeln!(out, "{}\t{}", p.name, p.granted);
    }
    for name in r.bus.iter().chain(&r.files) {
        let _ = writeln!(out, "{name}\t-");
    }
    Ok(ExitCode::SUCCESS)
}

fn run(client: &Client, lang: Lang, command: Command) -> Result<ExitCode, BusError> {
    Ok(match command {
        Command::List => {
            print_list(lang, &client.call(&List {}, WAIT)?.apps);
            ExitCode::SUCCESS
        }
        Command::Start(app, role, activation_token) => {
            start(client, lang, app, role, activation_token)?
        }
        Command::Stop(app, role) => {
            done(lang, client.call(&Stop { app, role }, WAIT)?.state.word())
        }
        Command::Permissions(app) => permissions(client, app)?,
        Command::Grant(app, permission) => {
            let r = client.call(&Grant { app, permission }, WAIT)?;
            done(
                lang,
                &format!("granted={} stopped={}", r.granted, r.stopped),
            )
        }
        Command::Revoke(app, permission) => {
            let r = client.call(&Revoke { app, permission }, WAIT)?;
            done(
                lang,
                &format!("granted={} stopped={}", r.granted, r.stopped),
            )
        }
        Command::Install(package) => {
            let r = client.call(&Install { package }, WAIT)?;
            done(lang, &format!("{} uid={} {}", r.app, r.uid, r.profile))
        }
        Command::Uninstall(app, keep_data) => {
            let r = client.call(&Uninstall { app, keep_data }, WAIT)?;
            done(lang, &format!("stopped={}", r.stopped))
        }
    })
}

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let lang = language(&words);
    let Some((bus, command)) = parse(&words) else {
        let mut err = std::io::stderr();
        let _ = writeln!(err, "{}", text(lang, Msg::BadArguments));
        let _ = writeln!(err, "{} {SYNOPSIS}", text(lang, Msg::Usage));
        return ExitCode::from(2);
    };
    let client = match Client::connect(&bus, ClientOptions::named("ulaunch")) {
        Ok(client) => client,
        Err(error) => return failure(lang, &error),
    };
    run(&client, lang, command).unwrap_or_else(|error| failure(lang, &error))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(list: &[&str]) -> Vec<String> {
        list.iter().map(|w| (*w).to_owned()).collect()
    }

    #[test]
    fn command_lines() {
        let (bus, cmd) =
            parse(&words(&["--bus", "/b", "start", "camera", "--token", "7"])).unwrap();
        assert_eq!(bus, PathBuf::from("/b"));
        assert_eq!(cmd, Command::Start("camera".into(), Role::Ui, Some(7)));
        let (_, cmd) = parse(&words(&["stop", "x", "--service"])).unwrap();
        assert_eq!(cmd, Command::Stop("x".into(), Role::Service));
        let (_, cmd) = parse(&words(&["uninstall", "x", "--keep-data", "--lang", "de"])).unwrap();
        assert_eq!(cmd, Command::Uninstall("x".into(), true));
        assert_eq!(parse(&words(&["list"])).unwrap().1, Command::List);
        assert_eq!(
            parse(&words(&["grant", "a", "network"])).unwrap().1,
            Command::Grant("a".into(), "network".into())
        );
        for bad in [
            &["start"][..],
            &["start", "x", "--token", "0"],
            &["list", "--service"],
            &["install", "f", "--keep-data"],
            &["grant", "a"],
            &["frob"],
            &["--frob", "list"],
            &[],
        ] {
            assert_eq!(parse(&words(bad)), None, "{bad:?}");
        }
        assert_eq!(exit_text(Some(Exit::Setup(113))), "setup 113");
        assert_eq!(exit_text(None), "-");
    }
}
