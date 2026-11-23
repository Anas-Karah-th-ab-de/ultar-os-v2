//! `ulog`: read the log over the bus, verify and export a log directory.

use crate::export::ExportOptions;
use crate::format::format_record;
use crate::msg::{text, Lang, Msg};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::log::{Query, Stats, MAX_WAIT_MS};
use ultar_bus::{BusError, Client, ClientOptions, ErrorCode};
use ultar_service::log::Level;
use ultar_service::{read_secret_source, SecretSource};

mod offline;

/// Default socket of the system bus.
pub const DEFAULT_BUS: &str = "/run/ultar/bus/system.sock";
/// Exit code: done.
pub const EXIT_OK: i32 = 0;
/// Exit code: failed, refused, or the log is not intact.
pub const EXIT_FAILED: i32 = 1;
/// Exit code: wrong command line.
pub const EXIT_USAGE: i32 = 2;
/// Longest wait for an answer of the log service beyond a follow wait.
const CALL_TIMEOUT: Duration = Duration::from_secs(10);

/// What `ulog` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Print matching records.
    Query(Query),
    /// Print matching records as they arrive; stop after `count` records
    /// when given.
    Follow {
        /// The filter.
        query: Query,
        /// Stop after this many records.
        count: Option<u64>,
    },
    /// Print the state of the log service.
    Stats,
    /// Check the hash chain of a log directory.
    Verify {
        /// The directory.
        dir: PathBuf,
        /// Chain key (`--key-file` or `--key-fd`).
        key: Option<SecretSource>,
    },
    /// Write a log directory to a text file for support.
    Export {
        /// The directory.
        dir: PathBuf,
        /// The file to write.
        out: PathBuf,
        /// Chain key (`--key-file` or `--key-fd`).
        key: Option<SecretSource>,
        /// Include the kernel's own lines.
        include_kernel_text: bool,
    },
}

/// The parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliArgs {
    /// Bus socket.
    pub bus: PathBuf,
    /// Token for a bus listener that authenticates by token
    /// (`--token-file` or `--token-fd`).
    pub token: Option<SecretSource>,
    /// The command.
    pub command: Command,
}

/// The command line is not understood.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UsageError;

#[derive(Default)]
struct Options {
    bus: Option<PathBuf>,
    token: Option<SecretSource>,
    key: Option<SecretSource>,
    query: Query,
    count: Option<u64>,
    include_kernel_text: bool,
    words: Vec<String>,
}

fn number<T: std::str::FromStr>(text: &str) -> Result<T, UsageError> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return Err(UsageError);
    }
    text.parse().map_err(|_| UsageError)
}

/// A secret has one source; naming it twice is a usage error.
fn set_secret(slot: &mut Option<SecretSource>, source: SecretSource) -> Result<(), UsageError> {
    if slot.replace(source).is_some() {
        return Err(UsageError);
    }
    Ok(())
}

/// An inherited descriptor number above the standard streams.
fn secret_fd(text: &str) -> Result<SecretSource, UsageError> {
    match number::<i32>(text)? {
        fd if fd >= 3 => Ok(SecretSource::Fd(fd)),
        _ => Err(UsageError),
    }
}

fn option(options: &mut Options, name: &str, value: String) -> Result<(), UsageError> {
    match name {
        "--bus" => options.bus = Some(PathBuf::from(value)),
        "--token-file" => set_secret(&mut options.token, SecretSource::File(value.into()))?,
        "--token-fd" => set_secret(&mut options.token, secret_fd(&value)?)?,
        "--key-file" => set_secret(&mut options.key, SecretSource::File(value.into()))?,
        "--key-fd" => set_secret(&mut options.key, secret_fd(&value)?)?,
        "--lang" => {}
        "--service" => options.query.service = Some(value),
        "--level" => {
            options.query.min_level = Level::from_name(&value).ok_or(UsageError)?.number();
        }
        "--since" => options.query.since_ms = Some(number(&value)?),
        "--until" => options.query.until_ms = Some(number(&value)?),
        "--after" => options.query.after_seq = number(&value)?,
        "--limit" => options.query.limit = number(&value)?,
        "--count" => options.count = Some(number(&value)?),
        _ => return Err(UsageError),
    }
    Ok(())
}

/// Parses the words after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<CliArgs, UsageError> {
    let mut args = args.into_iter();
    let mut options = Options::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--include-kernel-text" => options.include_kernel_text = true,
            name if name.starts_with("--") => {
                option(&mut options, name, args.next().ok_or(UsageError)?)?;
            }
            _ => options.words.push(arg),
        }
    }
    let words: Vec<&str> = options.words.iter().map(String::as_str).collect();
    let command = match words[..] {
        ["query"] => Command::Query(options.query),
        ["follow"] => Command::Follow {
            query: options.query,
            count: options.count,
        },
        ["stats"] => Command::Stats,
        ["verify", dir] => Command::Verify {
            dir: PathBuf::from(dir),
            key: options.key,
        },
        ["export", dir, out] => Command::Export {
            dir: PathBuf::from(dir),
            out: PathBuf::from(out),
            key: options.key,
            include_kernel_text: options.include_kernel_text,
        },
        _ => return Err(UsageError),
    };
    Ok(CliArgs {
        bus: options.bus.unwrap_or_else(|| PathBuf::from(DEFAULT_BUS)),
        token: options.token,
        command,
    })
}

/// Where the output of a run goes.
pub struct Streams<'a> {
    /// Results.
    pub out: &'a mut dyn Write,
    /// Messages for the operator.
    pub err: &'a mut dyn Write,
    /// Language of the messages.
    pub lang: Lang,
}

impl Streams<'_> {
    pub(crate) fn say(&mut self, msg: Msg) {
        let _ = writeln!(self.err, "{}", text(self.lang, msg));
    }

    pub(crate) fn say_detail(&mut self, msg: Msg, detail: &dyn std::fmt::Display) {
        let _ = writeln!(self.err, "{} {detail}", text(self.lang, msg));
    }

    fn bus_failure(&mut self, error: &BusError) -> i32 {
        match error {
            BusError::Remote(ErrorCode::PermissionDenied) => self.say(Msg::NotPermitted),
            other => self.say_detail(Msg::BusFailed, other),
        }
        EXIT_FAILED
    }
}

/// Largest token or chain key the tool reads.
pub(crate) const MAX_SECRET: usize = 4096;

fn connect(args: &CliArgs, io: &mut Streams<'_>) -> Result<Client, i32> {
    let mut options = ClientOptions::named("ulog");
    if let Some(source) = &args.token {
        match read_secret_source(source, MAX_SECRET) {
            Ok(secret) => options.token = secret.expose_str().map(str::to_owned),
            Err(error) => {
                io.say_detail(Msg::SecretRejected, &error);
                return Err(EXIT_FAILED);
            }
        }
    }
    Client::connect(&args.bus, options).map_err(|error| io.bus_failure(&error))
}

/// Asks until `count` records were printed (or, without `follow`, until
/// the service has no more).
fn read(
    client: &Client,
    mut query: Query,
    follow: bool,
    count: Option<u64>,
    io: &mut Streams<'_>,
) -> i32 {
    let mut printed = 0u64;
    loop {
        query.wait_ms = if follow { MAX_WAIT_MS } else { 0 };
        let timeout = CALL_TIMEOUT + Duration::from_millis(u64::from(query.wait_ms));
        let reply = match client.call(&query, timeout) {
            Ok(reply) => reply,
            Err(error) => return io.bus_failure(&error),
        };
        for record in &reply.records {
            if writeln!(io.out, "{}", format_record(record)).is_err() {
                // The reader of our output went away: done.
                return EXIT_OK;
            }
            printed += 1;
            if count.is_some_and(|count| printed >= count) {
                return EXIT_OK;
            }
        }
        let _ = io.out.flush();
        query.after_seq = reply.next_seq;
        if !follow && !reply.more {
            return EXIT_OK;
        }
    }
}

fn stats(client: &Client, io: &mut Streams<'_>) -> i32 {
    let reply = match client.call(&Stats {}, CALL_TIMEOUT) {
        Ok(reply) => reply,
        Err(error) => return io.bus_failure(&error),
    };
    let _ = writeln!(
        io.out,
        "last_seq={} ring_records={} disk_bytes={} disk_limit={} degraded={} not_persisted={}",
        reply.last_seq,
        reply.ring_records,
        reply.disk_bytes,
        reply.disk_limit,
        reply.degraded,
        reply.not_persisted
    );
    for source in &reply.sources {
        let _ = writeln!(
            io.out,
            "source={} stored={} dropped={} malformed={}",
            source.service, source.stored, source.dropped, source.malformed
        );
    }
    EXIT_OK
}

/// Runs `ulog` and returns its exit code.
pub fn run(args: &CliArgs, io: &mut Streams<'_>) -> i32 {
    match &args.command {
        Command::Verify { dir, key } => offline::run_verify(dir, key.as_ref(), io),
        Command::Export {
            dir,
            out,
            key,
            include_kernel_text,
        } => {
            let options = ExportOptions {
                include_kernel_text: *include_kernel_text,
            };
            offline::run_export(dir, out, key.as_ref(), options, io)
        }
        Command::Query(_) | Command::Follow { .. } | Command::Stats => {
            let client = match connect(args, io) {
                Ok(client) => client,
                Err(code) => return code,
            };
            match &args.command {
                Command::Query(query) => read(&client, query.clone(), false, None, io),
                Command::Follow { query, count } => read(&client, query.clone(), true, *count, io),
                _ => stats(&client, io),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(words: &[&str]) -> Result<CliArgs, UsageError> {
        parse(words.iter().map(|w| (*w).to_owned()))
    }

    #[test]
    fn commands_and_filters_are_parsed() {
        let query = parsed(&[
            "--bus",
            "/b",
            "query",
            "--service",
            "app.phone",
            "--level",
            "warn",
            "--since",
            "10",
            "--until",
            "20",
            "--after",
            "5",
            "--limit",
            "7",
            "--lang",
            "de",
        ])
        .unwrap();
        assert_eq!(query.bus, PathBuf::from("/b"));
        let expected = Query {
            service: Some("app.phone".into()),
            min_level: 3,
            since_ms: Some(10),
            until_ms: Some(20),
            after_seq: 5,
            limit: 7,
            wait_ms: 0,
        };
        assert_eq!(query.command, Command::Query(expected));
        let follow = parsed(&["follow", "--count", "3", "--token-file", "/t"]).unwrap();
        assert_eq!(follow.bus, PathBuf::from(DEFAULT_BUS));
        assert_eq!(follow.token, Some(SecretSource::File("/t".into())));
        let by_fd = parsed(&["stats", "--token-fd", "5"]).unwrap();
        assert_eq!(by_fd.token, Some(SecretSource::Fd(5)));
        assert!(matches!(
            follow.command,
            Command::Follow { count: Some(3), .. }
        ));
    }

    #[test]
    fn offline_commands_are_parsed() {
        assert_eq!(parsed(&["stats"]).unwrap().command, Command::Stats);
        assert_eq!(
            parsed(&["verify", "/d", "--key-file", "/k"])
                .unwrap()
                .command,
            Command::Verify {
                dir: "/d".into(),
                key: Some(SecretSource::File("/k".into()))
            }
        );
        assert_eq!(
            parsed(&["export", "/d", "/o", "--include-kernel-text"])
                .unwrap()
                .command,
            Command::Export {
                dir: "/d".into(),
                out: "/o".into(),
                key: None,
                include_kernel_text: true
            }
        );
    }

    #[test]
    fn bad_command_lines_are_usage_errors() {
        for bad in [
            &[][..],
            &["query", "extra"],
            &["verify"],
            &["export", "/d"],
            &["query", "--level", "loud"],
            &["query", "--since", "yesterday"],
            &["query", "--limit"],
            &["query", "--frob", "1"],
            &["query", "--token", "abc"],
            &["verify", "/d", "--key-fd", "1"],
            &["verify", "/d", "--key-fd", "3", "--key-file", "/k"],
            &["stats", "--token-file", "/t", "--token-fd", "4"],
            &["bogus"],
        ] {
            assert_eq!(parsed(bad), Err(UsageError), "{bad:?}");
        }
    }
}
