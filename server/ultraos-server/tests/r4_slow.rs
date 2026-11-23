//! Security review r4: limits of the server process (fix of R3-8) against the real binary.
//! `silent_and_idle_connections_are_closed` passes; `r4_4_*` FAILS on the reviewed code.

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

fn start(max_connections: &str) -> Server {
    let dir = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_ultraos-server"))
        .env("ULTRAOS_DB", dir.path().join("db.sqlite"))
        .env("ULTRAOS_FILES", dir.path().join("files"))
        .env("ULTRAOS_SECRET", "cd".repeat(32))
        .env("ULTRAOS_BIND", "127.0.0.1:0")
        .env("ULTRAOS_MAX_CONNECTIONS", max_connections)
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

/// Seconds until the server closes `stream` (`None`: still open after `limit`).
fn closed_within(stream: &mut TcpStream, limit: Duration) -> Option<u64> {
    stream.set_read_timeout(Some(limit)).unwrap();
    let begun = Instant::now();
    let mut buffer = [0u8; 1024];
    loop {
        match stream.read(&mut buffer) {
            Ok(0) => return Some(begun.elapsed().as_secs()),
            Ok(_) => {}
            Err(_) => return None,
        }
    }
}

const HEALTH: &str = "GET /v1/health HTTP/1.1\r\nHost: x\r\n\r\n";

#[test]
fn silent_and_idle_connections_are_closed() {
    let server = start("4096");
    let mut silent = TcpStream::connect(&server.addr).unwrap();
    let mut idle = TcpStream::connect(&server.addr).unwrap();
    idle.write_all(HEALTH.as_bytes()).unwrap();
    let limit = Duration::from_secs(45);
    assert!(
        closed_within(&mut silent, limit).is_some(),
        "silent connection kept"
    );
    assert!(
        closed_within(&mut idle, limit).is_some(),
        "idle keep-alive kept"
    );
}

/// R4-4 (minor). The connection cap of the fix of R3-8 is global: one client that opens
/// `max_connections` connections and sends nothing holds every slot; everybody else waits in
/// the accept queue. Renewing them every 10 s keeps the server unreachable for all.
#[test]
fn r4_4_one_client_cannot_take_every_connection() {
    let server = start("16");
    let hogs: Vec<TcpStream> = (0..16)
        .map(|_| TcpStream::connect(&server.addr).unwrap())
        .collect();
    std::thread::sleep(Duration::from_millis(300));
    let mut other = TcpStream::connect(&server.addr).unwrap();
    other.write_all(HEALTH.as_bytes()).unwrap();
    other
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut answer = [0u8; 64];
    let read = other.read(&mut answer).unwrap_or(0);
    assert!(
        read > 0,
        "{} silent connections of one client keep everybody else out",
        hogs.len()
    );
}
