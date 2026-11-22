//! The session token goes to the configured server only — never to an external host that an old
//! catalogue entry or a redirect points to.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use ultar_cloud_e2e::Stack;

/// A one-shot foreign HTTP host: records the request head and answers with `body`.
fn foreign_host(body: &'static [u8]) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/apps/legacy.uap", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") && stream.read(&mut byte).unwrap_or(0) == 1 {
            head.push(byte[0]);
        }
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.write_all(body);
        let _ = tx.send(String::from_utf8_lossy(&head).into_owned());
    });
    (url, rx)
}

#[test]
fn token_is_not_forwarded_to_an_external_download_location() {
    let s = Stack::start();
    let body: &'static [u8] = b"legacy-package-bytes";
    let (external_url, seen) = foreign_host(body);
    // A catalogue entry from the first server release: the file lives on another host.
    let sha = "5f1d3e0d0c6a1f3f3d6f8f4a1b6f4f3c9a1e2f6f7a3b0b1d6f4f0a2c3d4e5f60";
    {
        let conn = s.state.db.lock();
        conn.execute(
            "INSERT INTO store_apps(id,name,version,size,sha256,sig,url) VALUES('old.app','Old','1.0',?1,?2,'',?3)",
            (body.len() as i64, sha, &external_url),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO store_versions(app_id,version,size,sha256,url,stored,created) VALUES('old.app','1.0',?1,?2,?3,0,0)",
            (body.len() as i64, sha, &external_url),
        )
        .unwrap();
    }
    let anna = s.signed_in("anna");
    let details = anna.app("old.app").unwrap();
    let dest = s.dir.path().join("old.uap");
    // The recorded digest is deliberately wrong: the download reaches the foreign host and is
    // then rejected as damaged — what matters here is what that host got to see.
    let outcome = anna.download_app(&details, &dest);
    assert!(matches!(outcome, Err(ultar_cloud::CloudError::Integrity)));
    assert!(!dest.exists());
    let head = seen
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .to_ascii_lowercase();
    assert!(head.starts_with("get /apps/legacy.uap"), "{head}");
    assert!(
        !head.contains("authorization"),
        "token leaked to a foreign host:\n{head}"
    );
    assert!(!head.contains("bearer"));
}
