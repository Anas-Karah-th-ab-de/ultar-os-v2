#![allow(dead_code)]
//! A very small HTTP/1.1 server for the end-to-end tests: one thread, one request per
//! connection, behaviour switched by shared state. It imitates the two update endpoints of
//! `ultraos-server` (API.md) and can misbehave on purpose.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Default, Clone)]
pub struct Behaviour {
    /// Bytes of the release file (`/v1/update/file/7`).
    pub file: Vec<u8>,
    /// Body of `update/latest`; `None` → 204.
    pub latest: Option<String>,
    /// Status to answer `update/latest` with instead (e.g. 401, 429, 500).
    pub latest_status: Option<u16>,
    /// Require this bearer token for everything (else 401).
    pub token: Option<String>,
    /// Ignore `Range` and always send the whole file with 200.
    pub ignore_range: bool,
    /// Close the connection after this many body bytes of a file answer that is not the
    /// header probe.
    pub drop_after: Option<usize>,
    /// Answer file requests with a redirect to this location.
    pub redirect: Option<String>,
    /// Lie in `Content-Range` (announce this start offset).
    pub fake_range_start: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub target: String,
    pub range: Option<String>,
    pub authorization: Option<String>,
    pub user_agent: Option<String>,
}

pub struct TestServer {
    pub base: String,
    pub behaviour: Arc<Mutex<Behaviour>>,
    pub seen: Arc<Mutex<Vec<Seen>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    addr: std::net::SocketAddr,
}

fn respond(
    stream: &mut TcpStream,
    status: &str,
    headers: &[String],
    body: &[u8],
    cut: Option<usize>,
) {
    let mut head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n",
        body.len()
    );
    for header in headers {
        head.push_str(header);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");
    let _ = stream.write_all(head.as_bytes());
    let body = match cut {
        Some(cut) => &body[..cut.min(body.len())],
        None => body,
    };
    let _ = stream.write_all(body);
    let _ = stream.flush();
}

fn parse_range(range: &str, len: usize) -> Option<(usize, usize)> {
    let spec = range.strip_prefix("bytes=")?;
    let (start, end) = spec.split_once('-')?;
    let start: usize = start.parse().ok()?;
    let end = match end {
        "" => len.checked_sub(1)?,
        text => text.parse::<usize>().ok()?.min(len.checked_sub(1)?),
    };
    (start <= end).then_some((start, end))
}

fn serve_file(stream: &mut TcpStream, b: &Behaviour, seen: &Seen) {
    if let Some(location) = &b.redirect {
        return respond(
            stream,
            "307 Temporary Redirect",
            &[format!("Location: {location}")],
            b"",
            None,
        );
    }
    let len = b.file.len();
    let probe = seen
        .range
        .as_deref()
        .is_some_and(|r| r.starts_with("bytes=0-"));
    let cut = if probe { None } else { b.drop_after };
    match seen.range.as_deref().filter(|_| !b.ignore_range) {
        None => respond(stream, "200 OK", &[], &b.file, cut),
        Some(range) => match parse_range(range, len) {
            None => respond(
                stream,
                "416 Range Not Satisfiable",
                &[format!("Content-Range: bytes */{len}")],
                b"",
                None,
            ),
            Some((start, end)) => {
                let announced = b.fake_range_start.unwrap_or(start as u64);
                let header = format!("Content-Range: bytes {announced}-{end}/{len}");
                respond(
                    stream,
                    "206 Partial Content",
                    &[header],
                    &b.file[start..=end],
                    cut,
                );
            }
        },
    }
}

/// Reads the request line and the headers the tests care about. `None`: not a request.
fn read_request(stream: &TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let target = line.split(' ').nth(1).unwrap_or("").to_string();
    let mut seen = Seen {
        target,
        range: None,
        authorization: None,
        user_agent: None,
    };
    loop {
        line.clear();
        if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
            break;
        }
        if let Some((name, value)) = line.trim_end().split_once(": ") {
            match name.to_ascii_lowercase().as_str() {
                "range" => seen.range = Some(value.to_string()),
                "authorization" => seen.authorization = Some(value.to_string()),
                "user-agent" => seen.user_agent = Some(value.to_string()),
                _ => {}
            }
        }
    }
    (!seen.target.is_empty()).then_some(seen)
}

fn error(stream: &mut TcpStream, status: &str, code: &str) {
    let body = format!(r#"{{"error":"{code}"}}"#);
    respond(stream, status, &[], body.as_bytes(), None);
}

fn serve_latest(stream: &mut TcpStream, b: &Behaviour) {
    if let Some(status) = b.latest_status {
        return error(stream, &format!("{status} X"), "x");
    }
    match &b.latest {
        Some(body) => {
            let headers = ["Content-Type: application/json".to_string()];
            respond(stream, "200 OK", &headers, body.as_bytes(), None);
        }
        None => respond(stream, "204 No Content", &[], b"", None),
    }
}

fn handle(mut stream: TcpStream, behaviour: &Mutex<Behaviour>, log: &Mutex<Vec<Seen>>) {
    let Some(seen) = read_request(&stream) else {
        return;
    };
    log.lock().unwrap().push(seen.clone());
    let b = behaviour.lock().unwrap().clone();
    if let Some(token) = &b.token {
        if seen.authorization.as_deref() != Some(&format!("Bearer {token}")) {
            return error(&mut stream, "401 Unauthorized", "auth_required");
        }
    }
    if seen.target.starts_with("/v1/update/latest") {
        serve_latest(&mut stream, &b);
    } else if seen.target.starts_with("/v1/update/file/") || seen.target.starts_with("/mirror/") {
        serve_file(&mut stream, &b, &seen);
    } else {
        error(&mut stream, "404 Not Found", "not_found");
    }
}

impl TestServer {
    pub fn start(behaviour: Behaviour) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let behaviour = Arc::new(Mutex::new(behaviour));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (b, s, st) = (behaviour.clone(), seen.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if st.load(Ordering::SeqCst) {
                    break;
                }
                if let Ok(stream) = stream {
                    handle(stream, &b, &s);
                }
            }
        });
        Self {
            base: format!("http://{addr}"),
            behaviour,
            seen,
            stop,
            thread: Some(thread),
            addr,
        }
    }

    /// A `update/latest` answer in the server's real shape, pointing at the release file.
    pub fn latest_json(size: u64) -> String {
        format!(
            r#"{{"id":7,"channel":"stable","version":"9.9.9","arch":"x86_64","edition":"desktop","size":{size},"sha256":"{}","roothash":"","sig":"","url":"/v1/update/file/7"}}"#,
            "00".repeat(32)
        )
    }

    pub fn set(&self, edit: impl FnOnce(&mut Behaviour)) {
        edit(&mut self.behaviour.lock().unwrap());
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(self.addr);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
