//! Helpers of the daemon tests: start the real binary, a stand-in for the
//! log service, signals.
#![allow(dead_code)]

use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use ultar_bus::{Client, ClientOptions};
use ultar_service::log::wire::{decode_body, FrameReader};
use ultar_service::log::OwnedRecord;

/// Upper bound for anything that should happen at once.
pub const WAIT: Duration = Duration::from_secs(30);
pub const BUSD: &str = env!("CARGO_BIN_EXE_ultar-busd");

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub fn temp_dir(tag: &str) -> PathBuf {
    let number = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir().join(format!("ubusd-{}-{number}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn send_signal(pid: u32, signal: &str) {
    let status = Command::new("kill")
        .args([&format!("-{signal}"), &pid.to_string()])
        .status()
        .unwrap();
    assert!(status.success(), "kill -{signal} {pid}");
}

/// The broker binary with the options every test needs.
pub fn busd() -> Command {
    let mut command = Command::new(BUSD);
    command
        .env_remove("ULTAR_BUS_SOCKET")
        .args(["--lang", "en"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if ultar_service::effective_uid() == 0 {
        command.arg("--allow-root");
    }
    command
}

/// Connects as soon as the broker listens (the client retries by itself).
pub fn connect(socket: &Path, name: &str) -> Client {
    let options = ClientOptions {
        connect_retry_for: WAIT,
        ..ClientOptions::named(name)
    };
    Client::connect(socket, options).unwrap()
}

/// Ends the child with SIGTERM; returns exit code and standard error.
pub fn terminate(child: Child) -> (Option<i32>, String) {
    send_signal(child.id(), "TERM");
    let output = child.wait_with_output().unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

/// A stand-in for the log service: accepts connections on `path` and
/// hands every decoded record to the test.
pub struct LogSink {
    pub path: PathBuf,
    records: Receiver<OwnedRecord>,
}

impl LogSink {
    pub fn start(path: &Path) -> Self {
        let listener = UnixListener::bind(path).unwrap();
        let (sender, records) = channel();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let sender = sender.clone();
                std::thread::spawn(move || {
                    let mut reader = FrameReader::new();
                    let mut buffer = [0u8; 4096];
                    while let Ok(read @ 1..) = stream.read(&mut buffer) {
                        let pushed = reader.push(&buffer[..read], |body| {
                            let _ = sender.send(decode_body(body).unwrap());
                        });
                        pushed.unwrap();
                    }
                });
            }
        });
        Self {
            path: path.to_owned(),
            records,
        }
    }

    /// Waits for the next record with message key `key`; records with
    /// other keys before it are skipped.
    pub fn expect(&self, key: &str) -> OwnedRecord {
        loop {
            let record = self
                .records
                .recv_timeout(WAIT)
                .unwrap_or_else(|_| panic!("no log record {key}"));
            if record.key == key {
                return record;
            }
        }
    }
}

/// The text value of field `name` (word or identifier).
pub fn text_field<'a>(record: &'a OwnedRecord, name: &str) -> Option<&'a str> {
    use ultar_service::log::OwnedValue;
    record.fields.iter().find_map(|(field, value)| match value {
        OwnedValue::Word(text) | OwnedValue::Id(text) if field == name => Some(text.as_str()),
        _ => None,
    })
}
