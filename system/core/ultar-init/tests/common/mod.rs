//! A world for the real `ultar-init` (not PID 1): service files, a
//! configuration, a stand-in log service that collects the output lines,
//! the control socket.
#![allow(dead_code)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use ultar_service::log::output::{decode_body, OutputLine};
use ultar_service::log::wire::FrameReader;

pub const INIT: &str = env!("CARGO_BIN_EXE_ultar-init");
pub const UINIT: &str = env!("CARGO_BIN_EXE_uinit");
pub const WAIT: Duration = Duration::from_secs(20);

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Lines the stand-in log service received.
pub type Lines = Arc<Mutex<Vec<OutputLine>>>;

pub struct World {
    pub dir: PathBuf,
    pub lines: Lines,
}

fn collect(listener: UnixListener, lines: Lines) {
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { return };
        let lines = lines.clone();
        std::thread::spawn(move || {
            let mut frames = FrameReader::new();
            let mut buffer = [0u8; 4096];
            while let Ok(count) = stream.read(&mut buffer) {
                if count == 0 {
                    return;
                }
                let _ = frames.push(&buffer[..count], |body| {
                    if let Ok(line) = decode_body(body) {
                        lines.lock().unwrap().push(line);
                    }
                });
            }
        });
    }
}

impl World {
    pub fn new(tag: &str) -> Self {
        let number = NEXT.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("uinit-{}-{number}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("services")).unwrap();
        let lines: Lines = Arc::default();
        let listener = UnixListener::bind(dir.join("init.sock")).unwrap();
        let collected = lines.clone();
        std::thread::spawn(move || collect(listener, collected));
        Self { dir, lines }
    }

    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    pub fn service(&self, name: &str, text: &str) {
        std::fs::write(self.path("services").join(format!("{name}.toml")), text).unwrap();
    }

    /// Writes the configuration; `extra` are more lines of it.
    pub fn config(&self, extra: &str) -> PathBuf {
        let path = self.path("init.toml");
        let text = format!(
            "shutdown_grace_ms = 1000\nservice_dirs = [\"{}\"]\noutput_socket = \"{}\"\n\
             control_socket = \"{}\"\nbus_socket = \"{}\"\nbus_policy = \"{}\"\n{extra}",
            self.path("services").display(),
            self.path("init.sock").display(),
            self.path("ctl.sock").display(),
            self.path("bus-init.sock").display(),
            self.path("policy.toml").display(),
        );
        std::fs::write(&path, text).unwrap();
        path
    }

    pub fn start(&self, extra: &str) -> Child {
        let config = self.config(extra);
        Command::new(INIT)
            .arg("--config")
            .arg(config)
            .env("ULTAR_EDITION", "server")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    /// Sends one line to the control socket, returns the answer.
    pub fn ctl(&self, command: &str) -> String {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Ok(mut stream) = UnixStream::connect(self.path("ctl.sock")) {
                stream.write_all(format!("{command}\n").as_bytes()).unwrap();
                let mut answer = String::new();
                stream.read_to_string(&mut answer).unwrap();
                return answer;
            }
            assert!(Instant::now() < deadline, "no control socket");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The status line of `service` once `pred` holds for it.
    pub fn wait_status(&self, service: &str, pred: impl Fn(&[&str]) -> bool) -> Vec<String> {
        let deadline = Instant::now() + WAIT;
        loop {
            let status = self.ctl("status");
            for line in status.lines() {
                let columns: Vec<&str> = line.split_whitespace().collect();
                if columns.first() == Some(&service) && pred(&columns) {
                    return columns.iter().map(|c| (*c).to_owned()).collect();
                }
            }
            assert!(Instant::now() < deadline, "{service}: {status}");
            std::thread::sleep(Duration::from_millis(30));
        }
    }

    /// Waits until a line of `service` with `text` arrived.
    pub fn wait_line(&self, service: &str, text: &str) -> OutputLine {
        let deadline = Instant::now() + WAIT;
        loop {
            if let Some(line) = self
                .lines
                .lock()
                .unwrap()
                .iter()
                .find(|line| line.service == service && line.text.contains(text))
            {
                return line.clone();
            }
            assert!(Instant::now() < deadline, "no line {service}: {text}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn texts(&self, service: &str) -> Vec<String> {
        self.lines
            .lock()
            .unwrap()
            .iter()
            .filter(|line| line.service == service)
            .map(|line| line.text.clone())
            .collect()
    }
}

/// Ends init with SIGTERM; returns exit code and journal.
pub fn stop(child: Child) -> (Option<i32>, String) {
    send_signal(child.id(), "TERM");
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// The journal lines (`[ultar-init …] text`) without the time stamp.
pub fn journal(stderr: &str) -> Vec<String> {
    BufReader::new(stderr.as_bytes())
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| line.split_once("] ").map(|(_, text)| text.to_owned()))
        .collect()
}

pub fn is_root() -> bool {
    ultar_service::effective_uid() == 0
}

pub fn exists(path: &Path) -> bool {
    path.exists()
}
