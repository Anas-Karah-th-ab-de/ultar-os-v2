//! Starting a cell's helper process and watching it: the status pipe tells
//! the app's process id, when it is about to run and how it ended; its
//! standard output and error are cut into lines for the log service.

use crate::cell::{CellFailure, CellPlan, StatusLine, CELL_ARG, STATUS_ENV};
use std::io::{PipeReader, Read};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::process::ExitStatusExt;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_bus::messages::launcher::Exit;
use ultar_init::output::LineCutter;
use ultar_service::log::output::Stream;

/// How long a cell may take from the start of the helper to `READY`.
pub const START_DEADLINE: Duration = Duration::from_secs(10);

/// A started helper.
#[derive(Debug)]
pub struct Spawned {
    child: Child,
    status: Lines,
    out: Lines,
    err: Lines,
    /// An end that arrived together with `READY` (a very short run).
    early_exit: Option<Exit>,
}

/// Lines from one pipe.
#[derive(Debug)]
struct Lines {
    pipe: Option<PipeReader>,
    cutter: LineCutter,
}

impl Lines {
    fn new(pipe: PipeReader) -> Self {
        Self {
            pipe: Some(pipe),
            cutter: LineCutter::default(),
        }
    }

    fn fd(&self) -> Option<BorrowedFd<'_>> {
        self.pipe.as_ref().map(AsFd::as_fd)
    }

    /// Reads what is there; at the end of the stream the pipe is closed and
    /// the unfinished line delivered.
    fn read(&mut self, mut line: impl FnMut(String)) {
        let mut buffer = [0u8; 4096];
        let Some(pipe) = &mut self.pipe else { return };
        match pipe.read(&mut buffer) {
            Ok(0) | Err(_) => {
                self.pipe = None;
                self.cutter.finish(line);
            }
            Ok(n) => self.cutter.push(&buffer[..n], &mut line),
        }
    }
}

/// What the watcher reports.
pub trait Events: Send + Sync {
    /// One line of the app's output.
    fn line(&self, stream: Stream, text: &str);
    /// The instance ended.
    fn ended(&self, exit: Exit);
}

/// Starts `helper --cell <plan>` with `env` (and nothing else of this
/// process's environment) and pipes for status, output and error.
pub fn spawn(helper: &Path, plan: &CellPlan, env: &[(String, String)]) -> std::io::Result<Spawned> {
    let (status, status_end) = std::io::pipe()?;
    let (out, out_end) = std::io::pipe()?;
    let (err, err_end) = std::io::pipe()?;
    ultar_sys::set_inherited(&status_end, true)
        .map_err(|e| std::io::Error::from_raw_os_error(e.0))?;
    let child = Command::new(helper)
        .arg(CELL_ARG)
        .args(plan.to_args())
        .env_clear()
        .envs(env.iter().map(|(k, v)| (k, v)))
        .env(STATUS_ENV, status_end.as_raw_fd().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::from(out_end))
        .stderr(Stdio::from(err_end))
        .spawn();
    drop(status_end);
    Ok(Spawned {
        child: child?,
        status: Lines::new(status),
        out: Lines::new(out),
        err: Lines::new(err),
        early_exit: None,
    })
}

/// How the start of a cell went.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Handshake {
    /// The app runs with this process id.
    Running(u32),
    /// A stage failed before the app ran.
    Failed(CellFailure),
    /// The helper ended or did not answer in time (it was killed).
    Lost,
}

impl Spawned {
    /// Waits for `READY` (or a failure) until `deadline`.
    pub fn handshake(&mut self, deadline: Instant) -> Handshake {
        let (mut pid, mut ready) = (None, false);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let Some(fd) = self.status.fd() else { break };
            match ultar_sys::poll_readable(&[fd], Some(left)) {
                Ok(answer) if answer.first() == Some(&true) => {}
                _ => break,
            }
            let mut lines = Vec::new();
            self.status.read(|l| lines.push(l));
            for line in lines {
                match StatusLine::parse(&line) {
                    Some(StatusLine::Cell(p)) => pid = Some(p),
                    Some(StatusLine::Ready) => ready = true,
                    Some(StatusLine::Fail(failure)) => {
                        self.early_exit = Some(Exit::Setup(failure as i32));
                        return Handshake::Failed(failure);
                    }
                    Some(StatusLine::Exit(code)) => {
                        self.early_exit = self.early_exit.or(Some(Exit::Code(code)));
                    }
                    Some(StatusLine::Signal(s)) => {
                        self.early_exit = self.early_exit.or(Some(Exit::Signal(s)));
                    }
                    None => {}
                }
            }
            if let (true, Some(pid)) = (ready, pid) {
                return Handshake::Running(pid);
            }
        }
        let _ = self.child.kill();
        self.early_exit = Some(Exit::Setup(CellFailure::Spawn as i32));
        Handshake::Lost
    }

    /// Watches the instance on a thread of its own until its output ends.
    pub fn watch(self, events: Arc<dyn Events>) -> std::io::Result<()> {
        std::thread::Builder::new()
            .name("ultar-launcher-watch".into())
            .stack_size(128 * 1024)
            .spawn(move || self.run(events.as_ref()))
            .map(|_| ())
    }

    fn run(mut self, events: &dyn Events) {
        let mut exit = self.early_exit.take();
        let mut reported = false;
        while self.status.pipe.is_some() || self.out.pipe.is_some() || self.err.pipe.is_some() {
            let Some(flags) = self.readable() else { break };
            if flags[0] {
                // the first end reported wins: the cell's pid 1 reports the
                // app's end, stage 1 then only the end of pid 1
                self.status
                    .read(|line| match (StatusLine::parse(&line), exit) {
                        (_, Some(_)) => {}
                        (Some(StatusLine::Exit(code)), None) => exit = Some(Exit::Code(code)),
                        (Some(StatusLine::Signal(signal)), None) => {
                            exit = Some(Exit::Signal(signal))
                        }
                        (Some(StatusLine::Fail(failure)), None) => {
                            exit = Some(Exit::Setup(failure as i32))
                        }
                        _ => {}
                    });
            }
            if flags[1] {
                self.out.read(|line| events.line(Stream::Out, &line));
            }
            if flags[2] {
                self.err.read(|line| events.line(Stream::Err, &line));
            }
            if self.status.pipe.is_none() && !reported {
                reported = true;
                events.ended(self.finish(exit));
            }
        }
        if !reported {
            events.ended(self.finish(exit));
        }
    }

    /// Which of status, output and error can be read (waits for one).
    fn readable(&self) -> Option<[bool; 3]> {
        let slots = [self.status.fd(), self.out.fd(), self.err.fd()];
        let open: Vec<BorrowedFd<'_>> = slots.iter().flatten().copied().collect();
        let mut answers = ultar_sys::poll_readable(&open, None).ok()?.into_iter();
        Some(slots.map(|slot| slot.is_some() && answers.next().unwrap_or(false)))
    }

    /// Reaps the helper; how the app ended (from the status lines, else from
    /// the helper itself).
    fn finish(&mut self, exit: Option<Exit>) -> Exit {
        let helper = self.child.wait();
        exit.unwrap_or(match helper {
            Ok(status) => match (status.code(), status.signal()) {
                (Some(code), _) => {
                    CellFailure::from_code(code).map_or(Exit::Code(code), |f| Exit::Setup(f as i32))
                }
                (None, Some(signal)) => Exit::Signal(signal),
                (None, None) => Exit::Code(-1),
            },
            Err(_) => Exit::Code(-1),
        })
    }
}
