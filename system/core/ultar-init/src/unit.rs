//! One service under supervision: its state, its process and the pipes
//! init reads from it.

use crate::output::LineCutter;
use crate::spec::ServiceSpec;
use std::collections::VecDeque;
use std::io::{PipeReader, Read};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_bus::messages::init::{Exit, ServiceState};
use ultar_service::log::output::Stream;
use ultar_service::ready::{MAX_LINE, MAX_UNKNOWN};
use ultar_service::ReadyEvent;

/// A run that lasted shorter than this and failed counts towards the
/// crash-loop limit.
pub const STABLE_AFTER: Duration = Duration::from_secs(10);
/// Short failed runs in a row after which a service is given up.
pub const CRASH_LIMIT: usize = 10;
/// Longest pause before a restart.
pub const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Where a service is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitState {
    /// Waits for its dependencies.
    Waiting,
    /// Started, readiness not yet reported.
    Starting {
        /// Deadline for `READY`.
        deadline: Instant,
    },
    /// Running (and ready).
    Running,
    /// Stop sent.
    Stopping {
        /// When the hard end follows.
        deadline: Instant,
        /// Whether the hard end was sent.
        killed: bool,
    },
    /// Waits before the next start.
    Backoff {
        /// When it starts again.
        until: Instant,
    },
    /// A one-shot service ended.
    Done {
        /// Whether it ended with success.
        ok: bool,
    },
    /// Stopped on request or never started.
    Stopped,
    /// Given up.
    Failed,
}

impl UnitState {
    /// The state as the bus reports it.
    pub const fn public(self) -> ServiceState {
        match self {
            Self::Waiting => ServiceState::Waiting,
            Self::Starting { .. } => ServiceState::Starting,
            Self::Running => ServiceState::Running,
            Self::Stopping { .. } => ServiceState::Stopping,
            Self::Backoff { .. } => ServiceState::Backoff,
            Self::Done { .. } => ServiceState::Done,
            Self::Stopped => ServiceState::Stopped,
            Self::Failed => ServiceState::Failed,
        }
    }
}

/// What a pipe of a service delivered.
#[derive(Debug, PartialEq, Eq)]
pub enum PipeNews {
    /// Lines of output.
    Lines(Vec<String>),
    /// The service reported an event.
    Ready(Vec<ReadyEvent>),
    /// The pipe ended (or broke the protocol and was closed).
    Closed,
}

/// The readiness pipe of a starting service.
#[derive(Debug)]
pub struct ReadyPipe {
    reader: PipeReader,
    line: Vec<u8>,
    unknown: usize,
}

/// An output pipe of a service.
#[derive(Debug)]
pub struct OutputPipe {
    reader: PipeReader,
    cutter: LineCutter,
    /// Which stream it is.
    pub stream: Stream,
}

/// A running process of a service.
#[derive(Debug)]
pub struct Run {
    /// Its process id (also its process group: the launcher starts a
    /// session).
    pub pid: u32,
    /// When it was started.
    pub since: Instant,
    /// Its memory group.
    pub group: Option<PathBuf>,
    /// Readiness pipe, while open.
    pub ready: Option<ReadyPipe>,
    /// Output pipes, while open.
    pub outputs: Vec<OutputPipe>,
}

impl ReadyPipe {
    /// Wraps the read end of a readiness pipe.
    pub fn new(reader: PipeReader) -> Self {
        Self {
            reader,
            line: Vec::new(),
            unknown: 0,
        }
    }

    /// The pipe's descriptor, for `poll`.
    pub fn reader(&self) -> &PipeReader {
        &self.reader
    }

    /// Reads once (the pipe was reported readable).
    pub fn read(&mut self) -> PipeNews {
        let mut buffer = [0u8; 256];
        let count = match self.reader.read(&mut buffer) {
            Ok(0) | Err(_) => return PipeNews::Closed,
            Ok(count) => count,
        };
        let mut events = Vec::new();
        for &byte in buffer.iter().take(count) {
            if byte != b'\n' {
                self.line.push(byte);
                if self.line.len() > MAX_LINE {
                    return PipeNews::Closed;
                }
                continue;
            }
            match ReadyEvent::parse(&self.line) {
                Some(event) => events.push(event),
                None => self.unknown += 1,
            }
            self.line.clear();
            if self.unknown > MAX_UNKNOWN {
                return PipeNews::Closed;
            }
        }
        PipeNews::Ready(events)
    }
}

impl OutputPipe {
    /// Wraps the read end of an output pipe.
    pub fn new(reader: PipeReader, stream: Stream) -> Self {
        Self {
            reader,
            cutter: LineCutter::default(),
            stream,
        }
    }

    /// The pipe's descriptor, for `poll`.
    pub fn reader(&self) -> &PipeReader {
        &self.reader
    }

    /// Reads once (the pipe was reported readable).
    pub fn read(&mut self) -> PipeNews {
        let mut buffer = [0u8; 4096];
        let mut lines = Vec::new();
        match self.reader.read(&mut buffer) {
            Ok(0) | Err(_) => {
                self.cutter.finish(|line| lines.push(line));
                if lines.is_empty() {
                    PipeNews::Closed
                } else {
                    PipeNews::Lines(lines)
                }
            }
            Ok(count) => {
                let bytes = buffer.get(..count).unwrap_or_default();
                self.cutter.push(bytes, |line| lines.push(line));
                PipeNews::Lines(lines)
            }
        }
    }
}

/// A service and everything init knows about it.
#[derive(Debug)]
pub struct Unit {
    /// Its definition.
    pub spec: ServiceSpec,
    /// Its state.
    pub state: UnitState,
    /// Starts after the first one.
    pub restarts: u32,
    /// How the previous run ended.
    pub last_exit: Option<Exit>,
    /// Why it failed, when it did.
    pub reason: Option<String>,
    /// Its process, while one runs.
    pub run: Option<Run>,
    /// Start again once the requested stop is done.
    pub start_after_stop: bool,
    /// The process was ended because it failed (ready timeout, gave up
    /// before ready): its end counts as a failure, not as a stop.
    pub failing: bool,
    /// Ends of short failed runs, newest last.
    short_failures: VecDeque<Instant>,
    /// Starts so far.
    started: u32,
}

impl Unit {
    /// A unit for `spec`; manual services wait for a request.
    pub fn new(spec: ServiceSpec) -> Self {
        let state = if spec.life.manual {
            UnitState::Stopped
        } else {
            UnitState::Waiting
        };
        Self {
            spec,
            state,
            restarts: 0,
            last_exit: None,
            reason: None,
            run: None,
            start_after_stop: false,
            failing: false,
            short_failures: VecDeque::new(),
            started: 0,
        }
    }

    /// Counts a start.
    pub fn note_start(&mut self) {
        if self.started > 0 {
            self.restarts = self.restarts.saturating_add(1);
        }
        self.started = self.started.saturating_add(1);
    }

    /// Counts a failed run that lasted `ran`; returns the pause before the
    /// next start, or `None` when the crash-loop limit is reached.
    pub fn note_failure(&mut self, ran: Duration, now: Instant) -> Option<Duration> {
        if ran >= STABLE_AFTER {
            self.short_failures.clear();
        }
        self.short_failures.push_back(now);
        while self.short_failures.len() > CRASH_LIMIT {
            self.short_failures.pop_front();
        }
        let count = self.short_failures.len();
        if count >= CRASH_LIMIT {
            return None;
        }
        let shift = u32::try_from(count.saturating_sub(1)).unwrap_or(31).min(8);
        Some((Duration::from_millis(200) * (1 << shift)).min(MAX_BACKOFF))
    }

    /// Forgets earlier failures (a start on request).
    pub fn reset_failures(&mut self) {
        self.short_failures.clear();
        self.reason = None;
    }

    /// Whether its process runs.
    pub fn is_live(&self) -> bool {
        self.run.is_some()
    }

    /// Whether services ordered after it may start.
    pub fn satisfies_dependants(&self) -> bool {
        match self.state {
            UnitState::Running => !self.spec.life.oneshot,
            UnitState::Done { ok } => ok,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn unit() -> Unit {
        Unit::new(ServiceSpec::parse("exec = \"/bin/true\"", "t").unwrap())
    }

    #[test]
    fn back_off_grows_and_a_crash_loop_ends_the_restarts() {
        let mut unit = unit();
        let now = Instant::now();
        let short = Duration::from_millis(10);
        let pauses: Vec<Option<Duration>> = (0..CRASH_LIMIT)
            .map(|_| unit.note_failure(short, now))
            .collect();
        assert_eq!(pauses[0], Some(Duration::from_millis(200)));
        assert_eq!(pauses[1], Some(Duration::from_millis(400)));
        assert!(pauses[8].unwrap() <= MAX_BACKOFF);
        assert_eq!(pauses[CRASH_LIMIT - 1], None);
        // A long run in between starts the count anew.
        unit.reset_failures();
        assert!(unit.note_failure(STABLE_AFTER, now).is_some());
        assert_eq!(
            unit.note_failure(short, now),
            Some(Duration::from_millis(400))
        );
    }

    #[test]
    fn restarts_count_from_the_second_start() {
        let mut unit = unit();
        unit.note_start();
        assert_eq!(unit.restarts, 0);
        unit.note_start();
        unit.note_start();
        assert_eq!(unit.restarts, 2);
        assert_eq!(UnitState::Failed.public(), ServiceState::Failed);
    }

    #[test]
    fn readiness_lines_are_parsed_and_a_flooding_child_is_cut_off() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let mut pipe = ReadyPipe::new(reader);
        writer.write_all(b"HELLO\nREADY 42\n").unwrap();
        assert_eq!(
            pipe.read(),
            PipeNews::Ready(vec![ReadyEvent::Ready { pid: 42 }])
        );
        writer.write_all(&[b'x'; 200]).unwrap();
        assert_eq!(pipe.read(), PipeNews::Closed);
        drop(writer);
        let (reader, writer) = std::io::pipe().unwrap();
        drop(writer);
        assert_eq!(ReadyPipe::new(reader).read(), PipeNews::Closed);
    }

    #[test]
    fn output_pipes_give_lines_and_the_rest_at_the_end() {
        let (reader, mut writer) = std::io::pipe().unwrap();
        let mut pipe = OutputPipe::new(reader, Stream::Out);
        writer.write_all(b"one\ntw").unwrap();
        assert_eq!(pipe.read(), PipeNews::Lines(vec!["one".into()]));
        drop(writer);
        assert_eq!(pipe.read(), PipeNews::Lines(vec!["tw".into()]));
        assert_eq!(pipe.read(), PipeNews::Closed);
    }
}
