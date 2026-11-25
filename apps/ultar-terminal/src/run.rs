//! A program of the system started by the shell: its output and errors arrive line by line
//! (two reader threads); it can be stopped. No pseudo-terminal: the program sees pipes.

use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// Longest line kept (longer lines are cut).
pub const MAX_LINE: usize = 4096;

/// What a running program did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// A line of output (or of errors).
    Line(String),
    /// It ended with this exit code (`None`: killed by a signal).
    Ended(Option<i32>),
}

/// A program that runs.
pub struct Running {
    child: Child,
    lines: Receiver<String>,
    readers: usize,
    ended: bool,
}

impl Running {
    /// Starts `program` with `args` in `cwd`.
    pub fn start(program: &Path, args: &[String], cwd: &Path) -> std::io::Result<Running> {
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let (tx, lines) = channel();
        let mut readers = 0;
        let pipes: [Option<Box<dyn Read + Send>>; 2] = [
            child
                .stdout
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
            child
                .stderr
                .take()
                .map(|p| Box::new(p) as Box<dyn Read + Send>),
        ];
        for pipe in pipes.into_iter().flatten() {
            let tx = tx.clone();
            readers += 1;
            std::thread::spawn(move || {
                let mut reader = BufReader::new(pipe);
                let mut buf = Vec::new();
                while reader.read_until(b'\n', &mut buf).is_ok_and(|n| n > 0) {
                    let text = String::from_utf8_lossy(&buf);
                    let mut line = text.trim_end_matches(['\n', '\r']).to_owned();
                    if line.len() > MAX_LINE {
                        let mut cut = MAX_LINE;
                        while !line.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        line.truncate(cut);
                    }
                    if tx.send(line).is_err() {
                        return;
                    }
                    buf.clear();
                }
            });
        }
        Ok(Running {
            child,
            lines,
            readers,
            ended: false,
        })
    }

    /// What happened since the last call; [`Event::Ended`] comes once, after the last line.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        if self.ended {
            return out;
        }
        loop {
            match self.lines.try_recv() {
                Ok(line) => out.push(Event::Line(line)),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.readers = 0;
                    break;
                }
            }
        }
        if self.readers == 0 {
            if let Ok(Some(status)) = self.child.try_wait() {
                self.ended = true;
                out.push(Event::Ended(status.code()));
            }
        }
        out
    }

    /// Stops the program (Ctrl+C).
    pub fn stop(&mut self) {
        let _ = self.child.kill();
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.ended {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn until_end(r: &mut Running) -> Vec<Event> {
        let start = Instant::now();
        let mut all = Vec::new();
        while start.elapsed() < Duration::from_secs(10) {
            let got = r.poll();
            let done = got.iter().any(|e| matches!(e, Event::Ended(_)));
            all.extend(got);
            if done {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        all
    }

    #[test]
    fn output_lines_and_the_exit_code_arrive() {
        let mut r = Running::start(
            Path::new("/bin/sh"),
            &["-c".into(), "echo سطر; echo err >&2; exit 3".into()],
            Path::new("/"),
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let all = until_end(&mut r);
        assert!(all.contains(&Event::Line("سطر".into())));
        assert!(all.contains(&Event::Line("err".into())));
        assert_eq!(all.last(), Some(&Event::Ended(Some(3))));
    }

    #[test]
    fn a_program_can_be_stopped() {
        let mut r = Running::start(Path::new("/bin/sleep"), &["30".into()], Path::new("/"))
            .unwrap_or_else(|e| panic!("{e}"));
        r.stop();
        let all = until_end(&mut r);
        assert_eq!(all.last(), Some(&Event::Ended(None)));
    }
}
