//! Shared harness: starts the real server in-process on an ephemeral port and talks to it over
//! real TCP. Time for rate limiting comes from a manual clock.
#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use ultraos_server::admin::{self, AppPublication, ReleasePublication};
use ultraos_server::clock::ManualClock;
use ultraos_server::ratelimit::{BucketConfig, LimiterConfig};
use ultraos_server::{AppState, Config};

pub const SECRET: [u8; 32] = [0x11; 32];

/// A running server plus handles for administration and time.
pub struct TestServer {
    pub addr: SocketAddr,
    pub base: String,
    pub state: Arc<AppState>,
    pub clock: Arc<ManualClock>,
    pub dir: tempfile::TempDir,
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Limits so high that ordinary tests never touch them.
pub fn unlimited() -> LimiterConfig {
    let b = BucketConfig {
        capacity: 1_000_000,
        refill_per_minute: 1_000_000,
    };
    LimiterConfig {
        general: b,
        download: b,
        auth: b,
        max_entries: 1000,
    }
}

impl TestServer {
    pub fn start() -> Self {
        Self::start_with(|_| {})
    }

    pub fn start_with(edit: impl FnOnce(&mut Config)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("server.sqlite");
        Self::start_in(dir, &db, edit)
    }

    /// Starts on an existing database file inside `dir`.
    pub fn start_in(dir: tempfile::TempDir, db: &Path, edit: impl FnOnce(&mut Config)) -> Self {
        let mut config = Config::new(db.to_string_lossy(), dir.path().join("files"), SECRET);
        config.limits = unlimited();
        config.turn_secret = "turn-secret".into();
        config.wg_server_pubkey = "server-pubkey".into();
        edit(&mut config);
        let clock = Arc::new(ManualClock::new(0));
        let state = AppState::new(config, clock.clone()).unwrap();
        let std_listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        std_listener.set_nonblocking(true).unwrap();
        let addr = std_listener.local_addr().unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel::<()>();
        let served = state.clone();
        let thread = std::thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::from_std(std_listener).unwrap();
                let stop = async {
                    let _ = rx.await;
                };
                ultraos_server::serve(listener, served, stop).await.unwrap();
            });
        });
        Self {
            addr,
            base: format!("http://{addr}"),
            state,
            clock,
            dir,
            shutdown: Some(tx),
            thread: Some(thread),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub fn get(&self, path: &str) -> Reply {
        request("GET", &self.url(path), &[], None)
    }

    pub fn get_with(&self, path: &str, headers: &[(&str, &str)]) -> Reply {
        request("GET", &self.url(path), headers, None)
    }

    pub fn get_as(&self, path: &str, token: &str) -> Reply {
        request(
            "GET",
            &self.url(path),
            &[("authorization", &format!("Bearer {token}"))],
            None,
        )
    }

    pub fn post(&self, path: &str, token: Option<&str>, body: &serde_json::Value) -> Reply {
        let bearer = token.map(|t| format!("Bearer {t}"));
        let mut headers = vec![("content-type", "application/json")];
        if let Some(b) = &bearer {
            headers.push(("authorization", b));
        }
        request("POST", &self.url(path), &headers, Some(&body.to_string()))
    }

    /// Registers an account and returns its token.
    pub fn register(&self, username: &str) -> String {
        let body = serde_json::json!({"username": username, "password": "password-123"});
        let reply = self.post("/v1/account/register", None, &body);
        assert_eq!(reply.status, 200, "{}", reply.text());
        reply.json()["token"].as_str().unwrap().to_string()
    }

    pub fn write_file(&self, name: &str, content: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    /// Publishes an app version with the given package bytes.
    pub fn publish_app(&self, meta: serde_json::Value, content: &[u8]) -> String {
        let p: AppPublication = serde_json::from_value(meta).unwrap();
        let file = self.write_file("upload.uap", content);
        admin::publish_app(&self.state.db, &self.state.files, &p, &file)
            .unwrap()
            .sha256
    }

    /// Publishes a release image; returns its id.
    pub fn publish_release(&self, meta: serde_json::Value, content: &[u8]) -> i64 {
        let p: ReleasePublication = serde_json::from_value(meta).unwrap();
        let file = self.write_file("upload.img", content);
        admin::publish_release(&self.state.db, &self.state.files, &p, &file)
            .unwrap()
            .0
    }

    /// Sends raw bytes and returns the raw response (for requests a normal client would rewrite).
    pub fn raw(&self, request: &str) -> String {
        let mut stream = TcpStream::connect(self.addr).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut out = Vec::new();
        let _ = stream.read_to_end(&mut out);
        String::from_utf8_lossy(&out).into_owned()
    }

    /// Raw GET of an exact request target (no client-side normalisation).
    pub fn raw_get(&self, target: &str) -> String {
        self.raw(&format!(
            "GET {target} HTTP/1.1\r\nHost: test\r\nConnection: close\r\n\r\n"
        ))
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// An HTTP response.
pub struct Reply {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl Reply {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|e| panic!("not json ({e}): {}", self.text()))
    }

    /// The typed error code of an error response.
    pub fn error_code(&self) -> String {
        self.json()["error"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }
}

pub fn request(method: &str, url: &str, headers: &[(&str, &str)], body: Option<&str>) -> Reply {
    let agent = ureq::AgentBuilder::new().redirects(0).build();
    let mut req = agent.request(method, url);
    for (k, v) in headers {
        req = req.set(k, v);
    }
    let result = match body {
        Some(b) => req.send_string(b),
        None => req.call(),
    };
    let resp = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(e) => panic!("transport error for {method} {url}: {e}"),
    };
    let status = resp.status();
    let mut map = HashMap::new();
    for name in resp.headers_names() {
        if let Some(v) = resp.header(&name) {
            map.insert(name.to_ascii_lowercase(), v.to_string());
        }
    }
    let mut out = Vec::new();
    resp.into_reader().read_to_end(&mut out).unwrap();
    Reply {
        status,
        headers: map,
        body: out,
    }
}

/// Deterministic test content of `len` bytes.
pub fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}
