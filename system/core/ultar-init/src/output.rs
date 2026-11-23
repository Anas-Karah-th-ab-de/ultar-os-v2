//! Standard output and error of the services, forwarded to the log
//! service (`ultar_service::log::output`).
//!
//! Each service writes into a pipe of init. Init cuts the bytes into lines
//! (a line longer than [`MAX_TEXT`] is cut, the rest up to its line feed is
//! dropped), and hands them to the log service over its output socket. While
//! the log service is not there yet – at boot, before `logd` is up, or
//! while it restarts – the lines wait in a ring bounded in lines and bytes;
//! what does not fit pushes the oldest out and is counted. There is no file
//! that could grow without bound.

use std::collections::VecDeque;
use std::io::{ErrorKind, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_service::log::output::{encode_frame, OutputLine, Stream, MAX_TEXT};

/// Most lines waiting for the log service.
pub const RING_LINES: usize = 512;
/// Most bytes of text waiting for the log service.
pub const RING_BYTES: usize = 128 * 1024;
/// Pause between two connection attempts.
const RETRY: Duration = Duration::from_millis(500);
/// Name under which init's own lines are stored.
pub const INIT_NAME: &str = "init";

/// Cuts a byte stream into lines of at most [`MAX_TEXT`] bytes.
#[derive(Default, Debug)]
pub struct LineCutter {
    current: Vec<u8>,
    /// The current line was cut; skip up to its line feed.
    skipping: bool,
}

impl LineCutter {
    /// Feeds `bytes`; calls `line` with every complete (or cut) line.
    pub fn push(&mut self, bytes: &[u8], mut line: impl FnMut(String)) {
        for &byte in bytes {
            if byte == b'\n' {
                if !self.skipping {
                    line(String::from_utf8_lossy(&self.current).into_owned());
                }
                self.current.clear();
                self.skipping = false;
            } else if !self.skipping {
                self.current.push(byte);
                if self.current.len() >= MAX_TEXT {
                    let text = String::from_utf8_lossy(&self.current).into_owned();
                    line(ultar_service::log::output::cut(&text).to_owned());
                    self.current.clear();
                    self.skipping = true;
                }
            }
        }
    }

    /// The unfinished last line (when the stream ended without a line feed).
    pub fn finish(&mut self, line: impl FnOnce(String)) {
        if !self.current.is_empty() && !self.skipping {
            line(String::from_utf8_lossy(&self.current).into_owned());
        }
        self.current.clear();
        self.skipping = false;
    }
}

/// Lines on their way to the log service.
#[derive(Debug)]
pub struct Forwarder {
    socket: Option<PathBuf>,
    ring: VecDeque<OutputLine>,
    ring_bytes: usize,
    link: Option<UnixStream>,
    /// Bytes of a frame that did not fit into the socket yet.
    pending: Vec<u8>,
    next_try: Instant,
    dropped: u64,
}

impl Forwarder {
    /// Forwards to the output socket of the log service at `socket`
    /// (`None`: keep the newest lines only).
    pub fn new(socket: Option<PathBuf>) -> Self {
        Self {
            socket,
            ring: VecDeque::new(),
            ring_bytes: 0,
            link: None,
            pending: Vec::new(),
            next_try: Instant::now(),
            dropped: 0,
        }
    }

    /// Queues one line.
    pub fn push(&mut self, service: &str, stream: Stream, text: &str) {
        let text = ultar_service::log::output::cut(text).replace('\n', " ");
        self.ring_bytes += text.len();
        self.ring.push_back(OutputLine {
            service: service.to_owned(),
            stream,
            text,
        });
        while self.ring.len() > RING_LINES || self.ring_bytes > RING_BYTES {
            if let Some(old) = self.ring.pop_front() {
                self.ring_bytes -= old.text.len();
                self.dropped += 1;
            }
        }
    }

    /// Lines waiting.
    pub fn waiting(&self) -> usize {
        self.ring.len()
    }

    /// Lines lost because the ring was full, since start.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Whether a connection to the log service is open.
    pub fn connected(&self) -> bool {
        self.link.is_some()
    }

    fn connect(&mut self, now: Instant) {
        if self.link.is_some() || now < self.next_try {
            return;
        }
        self.next_try = now + RETRY;
        let Some(path) = &self.socket else {
            return;
        };
        if let Ok(stream) = UnixStream::connect(path) {
            if stream.set_nonblocking(true).is_ok() {
                self.link = Some(stream);
            }
        }
    }

    /// Writes what fits now; never blocks. Returns whether lines wait.
    pub fn pump(&mut self, now: Instant) -> bool {
        self.connect(now);
        loop {
            if self.pending.is_empty() {
                let Some(line) = self.ring.pop_front() else {
                    return false;
                };
                self.ring_bytes -= line.text.len();
                match encode_frame(&line) {
                    Ok(frame) => self.pending = frame,
                    Err(_) => continue,
                }
            }
            let Some(link) = &mut self.link else {
                return true;
            };
            match link.write(&self.pending) {
                Ok(0) => self.lose_link(),
                Ok(written) => {
                    self.pending.drain(..written);
                }
                Err(error) if error.kind() == ErrorKind::WouldBlock => return true,
                Err(error) if error.kind() == ErrorKind::Interrupted => {}
                Err(_) => self.lose_link(),
            }
        }
    }

    /// The log service went away: a frame half written is lost (the next
    /// connection starts on a frame boundary).
    fn lose_link(&mut self) {
        self.link = None;
        if !self.pending.is_empty() {
            self.pending.clear();
            self.dropped += 1;
        }
    }
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
