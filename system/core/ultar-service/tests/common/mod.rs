//! Helpers shared by the integration tests.
#![allow(dead_code)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Upper bound for anything that should happen at once.
pub const WAIT: Duration = Duration::from_secs(30);
pub const PROBE: &str = env!("CARGO_BIN_EXE_ultar-service-probe");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh private directory below the system temp directory.
pub fn temp_dir(tag: &str) -> PathBuf {
    let number = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("usvc-{}-{number}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Sends `signal` (e.g. `TERM`) to `pid` with the system's `kill` tool.
pub fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{signal} {pid}");
}

/// A running probe with its standard output as lines.
pub struct Probe {
    pub child: Child,
    pub lines: BufReader<ChildStdout>,
    pub seen: Vec<String>,
}

/// The probe arguments every test needs: root must be declared when the
/// tests themselves run as root.
pub fn base_args() -> Vec<&'static str> {
    if ultar_service::effective_uid() == 0 {
        vec!["--allow-root"]
    } else {
        Vec::new()
    }
}

impl Probe {
    pub fn spawn(mut command: Command) -> Self {
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
        let lines = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            lines,
            seen: Vec::new(),
        }
    }

    pub fn start(args: &[&str]) -> Self {
        let mut command = Command::new(PROBE);
        command.args(base_args()).args(args);
        Self::spawn(command)
    }

    /// Reads lines until `wanted` appears; panics at end of output.
    pub fn until(&mut self, wanted: &str) {
        loop {
            let mut line = String::new();
            let read = self.lines.read_line(&mut line).unwrap();
            assert!(read > 0, "output ended before {wanted:?}: {:?}", self.seen);
            let line = line.trim_end().to_owned();
            let found = line == wanted;
            self.seen.push(line);
            if found {
                return;
            }
        }
    }

    pub fn value(&self, key: &str) -> Option<&str> {
        self.seen
            .iter()
            .find_map(|line| line.strip_prefix(key)?.strip_prefix('='))
    }

    pub fn signal(&self, signal: &str) {
        send_signal(self.child.id(), signal);
    }

    /// Waits for the end; returns the exit code and standard error.
    pub fn finish(self) -> (Option<i32>, String) {
        let output = self.child.wait_with_output().unwrap();
        (
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )
    }
}
