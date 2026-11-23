//! The `uupdate` command: talks to the update service over the bus and prints through message
//! keys. `verify` works locally, without the service.

use crate::args::Args;
use crate::contract::{
    Accepted, Apply, BootPrevious, Cancel, Check, Download, GetPolicy, History, Offline,
    PolicyMessage, Progress, Status,
};
use crate::error::UpdateError;
use crate::keys::{load_burned, TrustStore};
use crate::signtool::verify_bundle;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;
use ultar_bus::{BusError, Call, Client, ClientOptions, ErrorCode};
use ultar_i18n::{Arg, Translator};

const CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Longest wait for the next progress event of a running operation.
const EVENT_TIMEOUT: Duration = Duration::from_secs(600);
const SWITCHES: [&str; 3] = ["--no-wait", "--metered", "--allow-downgrade"];

/// Exit code: success.
pub const EXIT_OK: u8 = 0;
/// Exit code: the operation failed.
pub const EXIT_FAILED: u8 = 1;
/// Exit code: wrong command line.
pub const EXIT_USAGE: u8 = 2;

/// The message key for a failed bus call.
pub fn bus_error_key(error: &BusError) -> &'static str {
    match error {
        BusError::Remote(ErrorCode::Service(code)) => {
            UpdateError::key_of_code(*code).unwrap_or("update.error.io")
        }
        BusError::Remote(ErrorCode::PermissionDenied) => UpdateError::PermissionDenied.key(),
        BusError::Remote(ErrorCode::ServiceBusy) => UpdateError::Busy.key(),
        BusError::Timeout => "update.cli.timeout",
        _ => "update.cli.no_bus",
    }
}

struct Session<'a> {
    client: Client,
    tr: &'a Translator,
    out: &'a mut dyn Write,
}

impl Session<'_> {
    fn say(&mut self, key: &str, args: &[(&str, Arg<'_>)]) {
        let _ = writeln!(self.out, "{}", self.tr.t_args(key, args));
    }

    fn call<C: Call>(&mut self, call: &C) -> Result<C::Reply, u8> {
        self.client.call(call, CALL_TIMEOUT).map_err(|error| {
            self.say(bus_error_key(&error), &[]);
            EXIT_FAILED
        })
    }

    fn print_progress(&mut self, progress: &Progress) {
        self.say(progress.phase.key(), &[]);
        if let Some(version) = &progress.version {
            self.say("update.cli.pending", &[("version", Arg::Str(version))]);
        }
        if progress.critical {
            self.say("update.cli.critical", &[]);
        }
        if let Some(key) = &progress.error_key {
            self.say(key, &[]);
        }
    }

    fn status(&mut self) -> Result<(), u8> {
        let status = self.call(&Status {})?;
        self.say(
            "update.cli.current",
            &[
                ("version", Arg::Str(&status.current_version)),
                ("slot", Arg::Str(&status.slot)),
            ],
        );
        self.say(
            "update.cli.channel",
            &[("channel", Arg::Str(&status.channel))],
        );
        let floor = i64::try_from(status.rollback_floor).unwrap_or(i64::MAX);
        self.say("update.cli.floor", &[("index", Arg::Int(floor))]);
        self.print_progress(&status.progress);
        match (&status.previous_version, status.previous_startable) {
            (Some(version), true) => {
                self.say("update.cli.previous", &[("version", Arg::Str(version))]);
            }
            _ => self.say("update.cli.previous_none", &[]),
        }
        Ok(())
    }

    /// Makes an "accepted" call and, unless `wait` is off, follows the operation to its end.
    fn operate<C: Call<Reply = Accepted>>(&mut self, call: &C, wait: bool) -> Result<(), u8> {
        let events = self.client.subscribe::<Progress>().map_err(|error| {
            self.say(bus_error_key(&error), &[]);
            EXIT_FAILED
        })?;
        self.call(call)?;
        self.say("update.cli.accepted", &[]);
        if !wait {
            return Ok(());
        }
        let (mut started, mut phase, mut percent) = (false, None, u64::MAX);
        loop {
            let Ok(event) = events.recv_timeout(EVENT_TIMEOUT) else {
                self.say("update.cli.timeout", &[]);
                return Err(EXIT_FAILED);
            };
            if !event.phase.busy() {
                if !started {
                    continue; // the retained state from before the call
                }
                self.print_progress(&event);
                return if event.error_key.is_some() {
                    Err(EXIT_FAILED)
                } else {
                    Ok(())
                };
            }
            started = true;
            if phase != Some(event.phase) {
                phase = Some(event.phase);
                self.say(event.phase.key(), &[]);
            }
            let now = (event.done.saturating_mul(100))
                .checked_div(event.total)
                .unwrap_or(0);
            if event.total > 0 && now / 10 != percent / 10 {
                percent = now;
                self.say("update.cli.progress", &[("percent", Arg::Int(now as i64))]);
            }
        }
    }

    fn history(&mut self, limit: u16) -> Result<(), u8> {
        let reply = self.call(&History { limit })?;
        if reply.entries.is_empty() {
            self.say("update.cli.history_empty", &[]);
        }
        for entry in reply.entries {
            let what = match &entry.detail_key {
                Some(detail) => format!("{} – {}", self.tr.t(&entry.kind_key), self.tr.t(detail)),
                None => self.tr.t(&entry.kind_key).into_owned(),
            };
            let time = i64::try_from(entry.time).unwrap_or(i64::MAX);
            let version = entry.version.unwrap_or_else(|| "–".to_string());
            self.say(
                "update.cli.history_line",
                &[
                    ("time", Arg::Int(time)),
                    ("what", Arg::Str(&what)),
                    ("version", Arg::Str(&version)),
                ],
            );
        }
        Ok(())
    }

    fn print_policy(&mut self, policy: &PolicyMessage) {
        let yes_no = |flag: bool| {
            if flag {
                "update.cli.yes"
            } else {
                "update.cli.no"
            }
        };
        let automatic = self.tr.t(yes_no(policy.automatic)).into_owned();
        let metered = self.tr.t(yes_no(policy.allow_metered)).into_owned();
        self.say(
            "update.cli.policy.automatic",
            &[("value", Arg::Str(&automatic))],
        );
        self.say(
            "update.cli.channel",
            &[("channel", Arg::Str(&policy.channel))],
        );
        match policy.window {
            Some((start, end)) => {
                let clock = |m: u16| format!("{:02}:{:02}", m / 60, m % 60);
                let (start, end) = (clock(start), clock(end));
                let args = [("start", Arg::Str(&start)), ("end", Arg::Str(&end))];
                self.say("update.cli.policy.window", &args);
            }
            None => self.say("update.cli.policy.window_any", &[]),
        }
        self.say(
            "update.cli.policy.metered",
            &[("value", Arg::Str(&metered))],
        );
        match policy.rate_limit_kib {
            0 => self.say("update.cli.policy.rate_unlimited", &[]),
            rate => self.say(
                "update.cli.policy.rate",
                &[("rate", Arg::Int(i64::from(rate)))],
            ),
        }
    }

    fn policy(&mut self, changes: &[String]) -> Result<(), u8> {
        let mut policy = self.call(&GetPolicy {})?;
        if changes.is_empty() {
            self.print_policy(&policy);
            return Ok(());
        }
        for change in changes {
            apply_policy_change(&mut policy, change).map_err(|_| EXIT_USAGE)?;
        }
        self.call(&policy)?;
        self.print_policy(&policy);
        Ok(())
    }
}

/// Applies one `key=value` of `uupdate policy set` to `policy`.
/// Keys: `automatic`, `metered` (`yes`/`no`), `channel`, `window` (`HH:MM-HH:MM` in UTC or
/// `any`), `rate` (KiB/s, 0 = unlimited).
pub fn apply_policy_change(policy: &mut PolicyMessage, change: &str) -> Result<(), UpdateError> {
    let (key, value) = change.split_once('=').ok_or(UpdateError::InvalidArgument)?;
    let flag = |text: &str| match text {
        "yes" | "1" | "on" => Ok(true),
        "no" | "0" | "off" => Ok(false),
        _ => Err(UpdateError::InvalidArgument),
    };
    let minutes = |text: &str| -> Option<u16> {
        let (h, m) = text.split_once(':')?;
        let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    match key {
        "automatic" => policy.automatic = flag(value)?,
        "metered" => policy.allow_metered = flag(value)?,
        "channel" => policy.channel = value.to_string(),
        "rate" => {
            policy.rate_limit_kib = value.parse().map_err(|_| UpdateError::InvalidArgument)?
        }
        "window" if value == "any" => policy.window = None,
        "window" => {
            let (start, end) = value.split_once('-').ok_or(UpdateError::InvalidArgument)?;
            let window = minutes(start)
                .zip(minutes(end))
                .ok_or(UpdateError::InvalidArgument)?;
            policy.window = Some(window);
        }
        _ => return Err(UpdateError::InvalidArgument),
    }
    Ok(())
}

/// Verifies a bundle file without the service. With `state_dir` the keys in force on this
/// device are used (burned-in set plus accepted rotations); without it only the burned-in
/// set – which may still contain a key that a rotation has retired since.
fn verify_local(
    file: &str,
    keys_dir: &str,
    state_dir: Option<&str>,
    tr: &Translator,
    out: &mut dyn Write,
) -> u8 {
    let keys = match state_dir {
        Some(state) => TrustStore::open(Path::new(keys_dir), Path::new(state))
            .map(|trust| trust.keys().to_vec()),
        None => load_burned(Path::new(keys_dir)).map(|set| set.keys),
    };
    match keys.and_then(|keys| verify_bundle(Path::new(file), &keys)) {
        Ok(authentic) => {
            let version = authentic.manifest().version.to_string();
            let line = tr.t_args("update.cli.verify_ok", &[("version", Arg::Str(&version))]);
            let _ = writeln!(out, "{line}");
            EXIT_OK
        }
        Err(error) => {
            let _ = writeln!(out, "{}", tr.t(error.key()));
            EXIT_FAILED
        }
    }
}

fn run_command(session: &mut Session<'_>, args: &Args) -> Result<(), u8> {
    let wait = !args.switch("--no-wait");
    let words: Vec<&str> = args.positional.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["status"] => session.status(),
        ["check"] => session.operate(&Check {}, wait),
        ["download"] => {
            let allow_metered = args.switch("--metered");
            session.operate(&Download { allow_metered }, wait)
        }
        ["apply"] => session.operate(&Apply {}, wait),
        ["offline", path] => {
            let call = Offline {
                path: (*path).to_string(),
                allow_downgrade: args.switch("--allow-downgrade"),
            };
            session.operate(&call, wait)
        }
        ["cancel"] => session.call(&Cancel {}).map(|_| ()),
        ["previous"] => session.call(&BootPrevious {}).map(|_| ()),
        ["history"] => session.history(20),
        ["history", count] => session.history(count.parse().map_err(|_| EXIT_USAGE)?),
        ["policy"] => session.policy(&[]),
        ["policy", "set", ..] if words.len() > 2 => session.policy(&args.positional[2..]),
        _ => Err(EXIT_USAGE),
    }
}

/// Runs the command line `words` and returns the exit code. `socket_env`: the value of
/// `ULTAR_BUS_SOCKET`, used when `--socket` is absent.
pub fn run(
    words: &[String],
    socket_env: Option<String>,
    tr: &Translator,
    out: &mut dyn Write,
) -> u8 {
    let usage = |out: &mut dyn Write| {
        let _ = writeln!(out, "{}", tr.t("update.cli.usage"));
        EXIT_USAGE
    };
    let Ok(args) = Args::parse(words, &SWITCHES) else {
        return usage(out);
    };
    if args
        .only(&["--socket", "--lang", "--keys", "--state"])
        .is_err()
        || args.positional.is_empty()
    {
        return usage(out);
    }
    if let ["verify", file] = args
        .positional
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()[..]
    {
        return match (args.one("--keys"), args.one("--state")) {
            (Ok(Some(keys)), Ok(state)) => verify_local(file, keys, state, tr, out),
            _ => usage(out),
        };
    }
    let socket = args
        .one("--socket")
        .ok()
        .flatten()
        .map(str::to_string)
        .or(socket_env);
    match socket {
        Some(socket) => run_on_bus(&socket, &args, tr, out),
        None => usage(out),
    }
}

/// Connects to the bus at `socket` and runs the service command in `args`.
fn run_on_bus(socket: &str, args: &Args, tr: &Translator, out: &mut dyn Write) -> u8 {
    let mut options = ClientOptions::named("uupdate");
    options.reconnect = false;
    options.connect_retry_for = Duration::ZERO;
    let client = match Client::connect(&PathBuf::from(socket), options) {
        Ok(client) => client,
        Err(_) => {
            let _ = writeln!(out, "{}", tr.t("update.cli.no_bus"));
            return EXIT_FAILED;
        }
    };
    let mut session = Session { client, tr, out };
    let code = match run_command(&mut session, args) {
        Ok(()) => EXIT_OK,
        Err(EXIT_USAGE) => {
            session.say("update.cli.usage", &[]);
            EXIT_USAGE
        }
        Err(code) => code,
    };
    session.client.close();
    code
}

#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
