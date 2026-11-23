//! Security review r3 (device identity), finding R3-8 (minor), against the real server binary:
//! how the process treats a client that never finishes its request. FAILS on the reviewed code.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Server {
    child: Child,
    addr: String,
    _dir: tempfile::TempDir,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start() -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "cd".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines.next().expect("server ended").unwrap();
        if let Some(i) = line.find("addr=") {
            break line[i + 5..].trim().trim_matches('"').to_string();
        }
    };
    std::thread::spawn(move || for _ in lines {});
    Server {
        child,
        addr,
        _dir: dir,
    }
}

/// Seconds until the server closes a connection whose request never completes (`None`: still
/// open after `limit`).
fn closed_after(server: &Server, partial: &str, limit: Duration) -> Option<u64> {
    let mut stream = TcpStream::connect(&server.addr).unwrap();
    stream.write_all(partial.as_bytes()).unwrap();
    stream.set_read_timeout(Some(limit)).unwrap();
    let begun = Instant::now();
    let mut buffer = [0u8; 256];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Some(begun.elapsed().as_secs()),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

/// R3-8 (minor). The process sets no time limit for receiving a request: a client that sends
/// half a request holds its connection (and its task) for as long as it likes. Only the proxy in
/// front protects the server; the rules ask for every limit to be explicit and tested.
#[test]
fn r3_8_a_request_that_never_completes_is_dropped() {
    let server = start();
    let limit = Duration::from_secs(45);
    let headers =
        "POST /v1/device/challenge HTTP/1.1\r\nHost: x\r\nContent-Type: application/json\r\n";
    let body = format!("{headers}Content-Length: 500\r\n\r\n{{\"purpose\":");
    for (what, partial) in [("unfinished headers", headers), ("unfinished body", &body)] {
        let closed = closed_after(&server, partial, limit);
        assert!(closed.is_some(), "{what}: still open after {limit:?}");
    }
}
