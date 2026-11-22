//! Security review: what can a malicious or compromised server do to the device client?
//! Each test states the property the device must keep. A failing test is a finding.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// One canned answer per request path prefix; records every request head.
#[derive(Clone, Default)]
struct Fake {
    routes: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
    seen: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    fn start() -> (Fake, String) {
        let fake = Fake::default();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let f = fake.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                let f = f.clone();
                std::thread::spawn(move || {
                    let mut head = Vec::new();
                    let mut b = [0u8; 1];
                    while !head.ends_with(b"\r\n\r\n") {
                        if s.read(&mut b).unwrap_or(0) == 0 {
                            return;
                        }
                        head.push(b[0]);
                    }
                    let head = String::from_utf8_lossy(&head).to_string();
                    let path = head.split_whitespace().nth(1).unwrap_or("").to_string();
                    f.seen.lock().unwrap().push(head.clone());
                    let answer = f
                        .routes
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|(p, _)| path.starts_with(p.as_str()))
                        .max_by_key(|(p, _)| p.len())
                        .map(|(_, a)| a.clone());
                    let answer = answer.unwrap_or_else(|| b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".to_vec());
                    let _ = s.write_all(&answer);
                });
            }
        });
        (fake, base)
    }
    fn set(&self, prefix: &str, answer: Vec<u8>) {
        let mut r = self.routes.lock().unwrap();
        r.retain(|(p, _)| p != prefix);
        r.insert(0, (prefix.to_string(), answer));
    }
    fn ok(body: &[u8], ctype: &str) -> Vec<u8> {
        let mut v = format!("HTTP/1.1 200 OK\r\ncontent-type: {ctype}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n", body.len()).into_bytes();
        v.extend_from_slice(body);
        v
    }
    fn authorization_seen(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|h| h.to_ascii_lowercase().contains("authorization:"))
            .cloned()
            .collect()
    }
}

fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(bytes))
}

fn details(id: &str, version: &str, size: u64, sha256: &str, download: &str) -> Vec<u8> {
    serde_json::json!({"id":id,"name":"App","description":"d","category":"office","publisher":{"name":"P","verified":true},"premium":false,
        "version":version,"size":size,"installed_size":1,"sha256":sha256,"sig":"","permissions":[],"file_types":[],"download":download,
        "versions":[{"version":version,"size":size,"sha256":sha256,"changelog":"","created":1}]})
    .to_string()
    .into_bytes()
}

struct Env {
    dir: tempfile::TempDir,
    key: String,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let (key, public) = ultar_pkg::keygen();
    std::fs::create_dir_all(dir.path().join("trusted")).unwrap();
    std::fs::write(dir.path().join("trusted/p.ed25519"), public).unwrap();
    Env { dir, key }
}

fn package(e: &Env, id: &str, version: &str, key: &str) -> Vec<u8> {
    let src = e.dir.path().join(format!("src-{version}"));
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("version.txt"), version).unwrap();
    let manifest = e.dir.path().join("m.toml");
    std::fs::write(
        &manifest,
        format!("id = \"{id}\"\nname = \"App\"\nversion = \"{version}\"\nkind = \"data\"\n"),
    )
    .unwrap();
    let out = e.dir.path().join("out.uap");
    ultar_pkg::pack_and_sign(&manifest, &src, &out, key).unwrap();
    std::fs::read(out).unwrap()
}

fn dirs(e: &Env) -> (PathBuf, PathBuf, PathBuf) {
    (
        e.dir.path().join("work"),
        e.dir.path().join("trusted"),
        e.dir.path().join("root"),
    )
}

fn installed_version(root: &Path, id: &str) -> Option<String> {
    std::fs::read_to_string(root.join("data/ultar/apps").join(id).join("version.txt")).ok()
}

fn serve_app(fake: &Fake, id: &str, version: &str, bytes: &[u8]) {
    fake.set(
        &format!("/v1/store/apps/{id}/download"),
        Fake::ok(bytes, "application/octet-stream"),
    );
    fake.set(
        &format!("/v1/store/apps/{id}"),
        Fake::ok(
            &details(
                id,
                version,
                bytes.len() as u64,
                &sha(bytes),
                &format!("/v1/store/apps/{id}/download"),
            ),
            "application/json",
        ),
    );
}

/// A server that offers an older, genuinely signed version must not be able to replace a newer installed one.
#[test]
fn server_cannot_downgrade_an_installed_app() {
    let e = env();
    let (work, trusted, root) = dirs(&e);
    let (fake, base) = Fake::start();
    let client = ultar_cloud::Client::new(&base, None);
    let v2 = package(&e, "com.example.app", "2.0.0", &e.key);
    let v1 = package(&e, "com.example.app", "1.0.0", &e.key);
    serve_app(&fake, "com.example.app", "2.0.0", &v2);
    assert_eq!(
        client
            .install_app_at("com.example.app", &work, &trusted, &root)
            .unwrap()
            .version,
        "2.0.0"
    );
    assert_eq!(
        installed_version(&root, "com.example.app").as_deref(),
        Some("2.0.0")
    );
    // the server (compromised, or an attacker in its place) now offers the old version
    serve_app(&fake, "com.example.app", "1.0.0", &v1);
    let result = client.install_app_at("com.example.app", &work, &trusted, &root);
    assert!(
        result.is_err(),
        "downgrade 2.0.0 -> 1.0.0 was installed: {:?}",
        result.map(|m| m.version)
    );
    assert_eq!(
        installed_version(&root, "com.example.app").as_deref(),
        Some("2.0.0")
    );
}

/// Unsigned, wrongly signed and re-labelled packages are never installed, whatever the server says.
#[test]
fn server_cannot_get_an_untrusted_package_installed() {
    let e = env();
    let (work, trusted, root) = dirs(&e);
    let (fake, base) = Fake::start();
    let client = ultar_cloud::Client::new(&base, None);
    let (other_key, _) = ultar_pkg::keygen();
    let foreign = package(&e, "com.example.app", "9.0.0", &other_key);
    serve_app(&fake, "com.example.app", "9.0.0", &foreign);
    assert!(
        client
            .install_app_at("com.example.app", &work, &trusted, &root)
            .is_err(),
        "package of an unknown publisher"
    );
    let mut tampered = package(&e, "com.example.app", "9.0.0", &e.key);
    let n = tampered.len();
    tampered[n / 2] ^= 0x55;
    serve_app(&fake, "com.example.app", "9.0.0", &tampered);
    assert!(
        client
            .install_app_at("com.example.app", &work, &trusted, &root)
            .is_err(),
        "tampered package"
    );
    let other_app = package(&e, "com.example.other", "9.0.0", &e.key);
    serve_app(&fake, "com.example.app", "9.0.0", &other_app);
    assert!(
        client
            .install_app_at("com.example.app", &work, &trusted, &root)
            .is_err(),
        "signed package of another app"
    );
    serve_app(
        &fake,
        "com.example.app",
        "9.0.0",
        b"garbage, not a tar file at all",
    );
    assert!(
        client
            .install_app_at("com.example.app", &work, &trusted, &root)
            .is_err(),
        "garbage"
    );
    assert!(
        installed_version(&root, "com.example.app").is_none()
            && installed_version(&root, "com.example.other").is_none()
    );
    assert!(
        std::fs::read_dir(&work).map(|d| d.count()).unwrap_or(0) == 0,
        "no download left behind"
    );
}

/// The session token never leaves the configured server: not on a redirect, not for an absolute download location.
#[test]
fn token_is_not_sent_to_another_host() {
    let e = env();
    let (work, trusted, root) = dirs(&e);
    let (ours, base) = Fake::start();
    let (theirs, other) = Fake::start();
    let client = ultar_cloud::Client::new(&base, Some("SECRET-TOKEN".into()));
    let pkg = package(&e, "com.example.app", "1.0.0", &e.key);
    theirs.set("/", Fake::ok(&pkg, "application/octet-stream"));
    // 1. redirect to another host
    ours.set("/v1/store/apps/com.example.app/download", format!("HTTP/1.1 307 Temporary Redirect\r\nlocation: {other}/stolen\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").into_bytes());
    ours.set(
        "/v1/store/apps/com.example.app",
        Fake::ok(
            &details(
                "com.example.app",
                "1.0.0",
                pkg.len() as u64,
                &sha(&pkg),
                "/v1/store/apps/com.example.app/download",
            ),
            "application/json",
        ),
    );
    client
        .install_app_at("com.example.app", &work, &trusted, &root)
        .unwrap();
    // 2. absolute download location on another host, in several spellings
    let host = other.trim_start_matches("http://");
    for location in [
        format!("{other}/x"),
        format!("{base}@{host}/x"),
        format!("{base}.{host}/x"),
        format!("http://{host}/x#{base}"),
        format!("HTTP://{host}/x"),
        format!("//{host}/x"),
        format!("\\\\{host}\\x"),
    ] {
        ours.set(
            "/v1/store/apps/com.example.app",
            Fake::ok(
                &details(
                    "com.example.app",
                    "1.0.0",
                    pkg.len() as u64,
                    &sha(&pkg),
                    &location,
                ),
                "application/json",
            ),
        );
        let _ = client.install_app_at("com.example.app", &work, &trusted, &root);
    }
    assert!(
        !ours.authorization_seen().is_empty(),
        "control: our own server does get the token"
    );
    let leaked: Vec<String> = theirs
        .seen
        .lock()
        .unwrap()
        .iter()
        .filter(|h| h.contains("SECRET-TOKEN") || h.to_ascii_lowercase().contains("bearer"))
        .cloned()
        .collect();
    assert_eq!(leaked, Vec::<String>::new(), "token sent to a foreign host");
    assert!(
        !theirs.seen.lock().unwrap().is_empty(),
        "control: the foreign host was contacted"
    );
}

/// A server that announces an absurd size must not be able to fill the device's disk: the client has to
/// refuse before writing (no free-space check and no upper bound exist when this test fails).
#[test]
fn server_cannot_fill_the_disk_with_an_oversized_download() {
    let e = env();
    let (work, trusted, root) = dirs(&e);
    let (fake, base) = Fake::start();
    let client = ultar_cloud::Client::new(&base, None);
    // announces 1 TiB, then streams 48 MiB of zeros and hangs up
    let sent: usize = 48 << 20;
    let mut answer =
        b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\nconnection: close\r\n\r\n"
            .to_vec();
    answer.extend(std::iter::repeat(0u8).take(sent));
    fake.set("/v1/store/apps/com.example.big/download", answer);
    fake.set(
        "/v1/store/apps/com.example.big",
        Fake::ok(
            &details(
                "com.example.big",
                "1.0.0",
                1 << 40,
                &"0".repeat(64),
                "/v1/store/apps/com.example.big/download",
            ),
            "application/json",
        ),
    );
    let part = work.join("com.example.big.uap.part");
    let peak = Arc::new(Mutex::new(0u64));
    let (p2, part2) = (peak.clone(), part.clone());
    let stop = Arc::new(Mutex::new(false));
    let stop2 = stop.clone();
    let watcher = std::thread::spawn(move || {
        while !*stop2.lock().unwrap() {
            if let Ok(m) = std::fs::metadata(&part2) {
                let mut p = p2.lock().unwrap();
                *p = (*p).max(m.len());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    });
    let result = client.install_app_at("com.example.big", &work, &trusted, &root);
    *stop.lock().unwrap() = true;
    watcher.join().unwrap();
    assert!(result.is_err());
    let peak = *peak.lock().unwrap();
    assert!(
        peak < (16 << 20),
        "the client wrote {} MiB of a download announced as 1 TiB to disk before giving up",
        peak >> 20
    );
}
