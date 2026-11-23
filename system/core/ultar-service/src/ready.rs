//! Readiness notification between a service and its supervisor.
//!
//! # Contract (what `ultar-init` has to do)
//!
//! 1. Create a pipe ([`ready_pipe`]).
//! 2. Start the service with the **write end inherited** under some
//!    descriptor number `N ≥ 3` and the environment variable
//!    [`READY_FD_ENV`]`=N`. (`ultar-init` already uses `pre_exec`; a
//!    `dup2(write_end, N)` there is all it takes.) Close the write end in
//!    the supervisor.
//! 3. Read the pipe with [`ReadyReader`] or [`wait_ready`]. Treat the
//!    service as started – and start the services ordered `after` it –
//!    only when [`ReadyEvent::Ready`] arrived. End of file before that
//!    means the service died or gave up during start.
//!
//! The service writes ASCII lines, each at most [`MAX_LINE`] bytes:
//!
//! ```text
//! READY <pid>\n        the service answers requests now
//! STOPPING\n           a stop was requested; the service is flushing
//! STOPPED <code>\n     the service is done and exits with <code>
//! ```
//!
//! The lines carry no secrets and no free text. Unknown lines are ignored
//! (room for later additions) up to [`MAX_UNKNOWN`] of them; a longer line
//! or more unknown lines end the reader with an error, so a faulty or
//! hostile child cannot make the supervisor buffer without bound.
//!
//! A service started without the variable (by hand, in tests) simply does
//! not notify.

use std::fmt;
use std::fs::File;
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::sync::mpsc::{sync_channel, RecvTimeoutError};
use std::sync::{Mutex, PoisonError};
use std::thread;
use std::time::Duration;
use ultar_sys as sys;

/// Environment variable that names the inherited write end of the pipe.
pub const READY_FD_ENV: &str = "ULTAR_READY_FD";
/// Longest line of the protocol, including the line feed.
pub const MAX_LINE: usize = 64;
/// Most lines a reader ignores before it gives up.
pub const MAX_UNKNOWN: usize = 16;
/// Largest descriptor number accepted from the environment.
pub const MAX_FD: i32 = 1023;

/// What a service reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ReadyEvent {
    /// The service answers requests; its process id as it sees it.
    Ready {
        /// Process id of the service.
        pid: u32,
    },
    /// A stop was requested.
    Stopping,
    /// The service is done.
    Stopped {
        /// Exit code the service is about to return.
        code: i32,
    },
}

impl ReadyEvent {
    /// The protocol line of this event, with its line feed.
    pub fn line(&self) -> String {
        match self {
            Self::Ready { pid } => format!("READY {pid}\n"),
            Self::Stopping => "STOPPING\n".to_owned(),
            Self::Stopped { code } => format!("STOPPED {code}\n"),
        }
    }

    /// Parses one line without its line feed; `None` for unknown lines.
    pub fn parse(line: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(line).ok()?;
        let mut words = text.split(' ');
        let event = match (words.next()?, words.next()) {
            ("READY", Some(pid)) => Self::Ready {
                pid: parse_number(pid)?,
            },
            ("STOPPING", None) => Self::Stopping,
            ("STOPPED", Some(code)) => Self::Stopped {
                code: parse_number(code)?,
            },
            _ => return None,
        };
        words.next().is_none().then_some(event)
    }
}

/// Decimal digits only (with an optional minus for signed types): no
/// plus sign, no spaces, nothing `str::parse` would additionally accept.
fn parse_number<T: std::str::FromStr>(word: &str) -> Option<T> {
    let digits = word.strip_prefix('-').unwrap_or(word);
    if digits.is_empty() || digits.len() > 10 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    word.parse().ok()
}

/// Service side: reports state changes to the supervisor, if there is one.
pub struct ReadyNotifier {
    sink: Mutex<Option<Box<dyn Write + Send>>>,
}

impl ReadyNotifier {
    /// A notifier that reports to nobody.
    pub fn disabled() -> Self {
        Self {
            sink: Mutex::new(None),
        }
    }

    /// A notifier that writes to `sink`.
    pub fn to_writer(sink: Box<dyn Write + Send>) -> Self {
        Self {
            sink: Mutex::new(Some(sink)),
        }
    }

    /// The descriptor number named by `value` (the content of
    /// [`READY_FD_ENV`]), if it is a plain number in `3..=MAX_FD`.
    pub fn fd_from_value(value: &str) -> Option<i32> {
        let fd: i32 = parse_number(value)?;
        (3..=MAX_FD).contains(&fd).then_some(fd)
    }

    /// Takes over the descriptor named in the environment. Anything
    /// unusable (no variable, not a number, descriptor not open) gives a
    /// disabled notifier: a service must start without a supervisor too.
    pub fn from_env() -> Self {
        let fd = std::env::var(READY_FD_ENV)
            .ok()
            .and_then(|value| Self::fd_from_value(&value));
        match fd.and_then(|fd| sys::adopt_fd(fd).ok()) {
            Some(owned) => Self::to_writer(Box::new(File::from(owned))),
            None => Self::disabled(),
        }
    }

    /// The descriptor number the environment names, to keep it open while
    /// inherited descriptors are closed.
    pub fn env_fd() -> Option<i32> {
        std::env::var(READY_FD_ENV)
            .ok()
            .and_then(|value| Self::fd_from_value(&value))
    }

    /// Whether a supervisor listens.
    pub fn is_enabled(&self) -> bool {
        self.sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_some()
    }

    /// Reports `event`. A supervisor that went away is not an error of the
    /// service: the notifier just turns itself off.
    pub fn notify(&self, event: ReadyEvent) {
        let mut sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let failed = match sink.as_mut() {
            Some(writer) => writer
                .write_all(event.line().as_bytes())
                .and_then(|()| writer.flush())
                .is_err(),
            None => false,
        };
        if failed {
            *sink = None;
        }
    }
}

/// Why the supervisor side stopped reading.
#[derive(Debug)]
pub enum ReadyError {
    /// The pipe was closed before the awaited event (service gone).
    Closed,
    /// A line was longer than [`MAX_LINE`].
    LineTooLong,
    /// More than [`MAX_UNKNOWN`] lines that are not part of the protocol.
    Garbage,
    /// The event did not arrive in time.
    Timeout,
    /// Reading the pipe failed.
    Io(std::io::Error),
}

impl fmt::Display for ReadyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Closed => f.write_str("service closed the readiness pipe"),
            Self::LineTooLong => f.write_str("readiness line too long"),
            Self::Garbage => f.write_str("too many unknown readiness lines"),
            Self::Timeout => f.write_str("service did not become ready in time"),
            Self::Io(error) => write!(f, "readiness pipe: {error}"),
        }
    }
}

impl std::error::Error for ReadyError {}

/// Supervisor side: parses the lines a service writes.
pub struct ReadyReader<R> {
    input: R,
    line: Vec<u8>,
    unknown: usize,
}

impl<R: Read> ReadyReader<R> {
    /// Reads events from `input` (the read end of the pipe).
    pub fn new(input: R) -> Self {
        Self {
            input,
            line: Vec::with_capacity(MAX_LINE),
            unknown: 0,
        }
    }

    /// Blocks until the next event. `Ok(None)` is the end of the pipe.
    pub fn next_event(&mut self) -> Result<Option<ReadyEvent>, ReadyError> {
        loop {
            let mut byte = [0u8; 1];
            match self.input.read(&mut byte) {
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(ReadyError::Io(error)),
            }
            if byte[0] != b'\n' {
                if self.line.len() + 1 >= MAX_LINE {
                    return Err(ReadyError::LineTooLong);
                }
                self.line.push(byte[0]);
                continue;
            }
            let event = ReadyEvent::parse(&self.line);
            self.line.clear();
            match event {
                Some(event) => return Ok(Some(event)),
                None => {
                    self.unknown += 1;
                    if self.unknown > MAX_UNKNOWN {
                        return Err(ReadyError::Garbage);
                    }
                }
            }
        }
    }

    /// Blocks until the service reports `READY` and returns its pid.
    pub fn until_ready(&mut self) -> Result<u32, ReadyError> {
        match self.next_event()? {
            Some(ReadyEvent::Ready { pid }) => Ok(pid),
            // Stopping or stopped before ready, or the end of the pipe:
            // the service will never be ready.
            Some(_) | None => Err(ReadyError::Closed),
        }
    }
}

/// A pipe for the readiness protocol: the supervisor keeps the reader and
/// hands the writer to the service (see the module documentation).
pub fn ready_pipe() -> std::io::Result<(PipeReader, PipeWriter)> {
    std::io::pipe()
}

/// Waits at most `timeout` for `READY` on `input`. On success the reader
/// is returned for the later `STOPPING`/`STOPPED` events.
///
/// The wait runs on a helper thread. After a timeout that thread stays
/// blocked until the service closes its end of the pipe – which it does at
/// the latest when the supervisor kills it, the usual reaction to a
/// service that does not become ready.
pub fn wait_ready<R: Read + Send + 'static>(
    input: R,
    timeout: Duration,
) -> Result<(u32, ReadyReader<R>), ReadyError> {
    let (done, result) = sync_channel(1);
    thread::Builder::new()
        .name("ultar-ready-wait".into())
        .stack_size(64 * 1024)
        .spawn(move || {
            let mut reader = ReadyReader::new(input);
            let outcome = reader.until_ready().map(|pid| (pid, reader));
            let _ = done.send(outcome);
        })
        .map_err(ReadyError::Io)?;
    match result.recv_timeout(timeout) {
        Ok(outcome) => outcome,
        Err(RecvTimeoutError::Timeout) => Err(ReadyError::Timeout),
        Err(RecvTimeoutError::Disconnected) => Err(ReadyError::Closed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn events_round_trip_through_their_lines() {
        for event in [
            ReadyEvent::Ready { pid: 4711 },
            ReadyEvent::Stopping,
            ReadyEvent::Stopped { code: 0 },
            ReadyEvent::Stopped { code: -3 },
        ] {
            let line = event.line();
            assert!(line.len() <= MAX_LINE && line.ends_with('\n'));
            assert_eq!(ReadyEvent::parse(line.trim_end().as_bytes()), Some(event));
        }
    }

    #[test]
    fn malformed_lines_are_unknown() {
        for line in [
            &b""[..],
            b"READY",
            b"READY x",
            b"READY +1",
            b"READY 1 2",
            b"READY 99999999999",
            b"STOPPING 1",
            b"STOPPED",
            b"ready 1",
            b"READY\t1",
            b"\xff\xfe",
        ] {
            assert_eq!(ReadyEvent::parse(line), None, "{line:?}");
        }
    }

    #[test]
    fn reader_yields_events_and_skips_unknown_lines() {
        let text = b"HELLO future\nREADY 12\nSTOPPING\nSTOPPED 3\n".to_vec();
        let mut reader = ReadyReader::new(Cursor::new(text));
        assert_eq!(
            reader.next_event().unwrap(),
            Some(ReadyEvent::Ready { pid: 12 })
        );
        assert_eq!(reader.next_event().unwrap(), Some(ReadyEvent::Stopping));
        assert_eq!(
            reader.next_event().unwrap(),
            Some(ReadyEvent::Stopped { code: 3 })
        );
        assert_eq!(reader.next_event().unwrap(), None);
    }

    #[test]
    fn hostile_input_is_bounded() {
        let long = vec![b'A'; 10_000];
        assert!(matches!(
            ReadyReader::new(Cursor::new(long)).next_event(),
            Err(ReadyError::LineTooLong)
        ));
        let noise = b"x\n".repeat(MAX_UNKNOWN + 1);
        assert!(matches!(
            ReadyReader::new(Cursor::new(noise)).next_event(),
            Err(ReadyError::Garbage)
        ));
        // Exactly the allowed number of unknown lines is still fine.
        let mut ok = b"x\n".repeat(MAX_UNKNOWN);
        ok.extend_from_slice(b"READY 1\n");
        assert_eq!(ReadyReader::new(Cursor::new(ok)).until_ready().unwrap(), 1);
    }

    #[test]
    fn end_or_stop_before_ready_is_closed() {
        for text in [&b""[..], b"STOPPING\n", b"STOPPED 1\n", b"READY 1"] {
            let mut reader = ReadyReader::new(Cursor::new(text.to_vec()));
            assert!(
                matches!(reader.until_ready(), Err(ReadyError::Closed)),
                "{text:?}"
            );
        }
    }

    #[test]
    fn notifier_writes_into_a_real_pipe_and_survives_a_gone_reader() {
        let (reader, writer) = ready_pipe().unwrap();
        let notifier = ReadyNotifier::to_writer(Box::new(writer));
        assert!(notifier.is_enabled());
        notifier.notify(ReadyEvent::Ready { pid: 7 });
        let (pid, mut events) = wait_ready(reader, Duration::from_secs(30)).unwrap();
        assert_eq!(pid, 7);
        notifier.notify(ReadyEvent::Stopping);
        assert_eq!(events.next_event().unwrap(), Some(ReadyEvent::Stopping));
        drop(events);
        // The reader is gone: the write fails, the notifier turns off.
        notifier.notify(ReadyEvent::Stopped { code: 0 });
        assert!(!notifier.is_enabled());
        notifier.notify(ReadyEvent::Stopped { code: 0 });
    }

    #[test]
    fn waiting_times_out_and_reports_a_closed_pipe() {
        let (reader, writer) = ready_pipe().unwrap();
        assert!(matches!(
            wait_ready(reader, Duration::ZERO),
            Err(ReadyError::Timeout)
        ));
        drop(writer);
        let (reader, writer) = ready_pipe().unwrap();
        drop(writer);
        assert!(matches!(
            wait_ready(reader, Duration::from_secs(30)),
            Err(ReadyError::Closed)
        ));
    }

    #[test]
    fn descriptor_numbers_from_the_environment_are_checked() {
        assert_eq!(ReadyNotifier::fd_from_value("3"), Some(3));
        assert_eq!(ReadyNotifier::fd_from_value("1023"), Some(1023));
        for bad in ["", "2", "0", "-1", "1024", "3 ", "0x3", "three"] {
            assert_eq!(ReadyNotifier::fd_from_value(bad), None, "{bad:?}");
        }
        let disabled = ReadyNotifier::disabled();
        assert!(!disabled.is_enabled());
        disabled.notify(ReadyEvent::Stopping);
        for error in [
            ReadyError::Closed,
            ReadyError::LineTooLong,
            ReadyError::Garbage,
            ReadyError::Timeout,
            ReadyError::Io(std::io::ErrorKind::Other.into()),
        ] {
            assert!(!error.to_string().is_empty());
        }
    }
}
