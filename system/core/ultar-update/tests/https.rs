//! The HTTPS path end to end: the real client (ureq + rustls + Graviola) against a TLS front
//! end written for this test, which terminates TLS and passes the plain request on to the
//! test server of `common/http.rs`.
//!
//! `tests/fixtures/tls/` holds a test-only CA certificate and two server certificates with
//! their private keys (ECDSA P-256, valid until 2126; the CA's key was deleted after signing):
//! `server.pem` is for `127.0.0.1`, `other.pem` for `other.example`. They protect nothing.

mod common;
#[path = "common/http.rs"]
mod http;

use common::*;
use http::{Behaviour, TestServer};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ultar_update::bootctl::Slot;
use ultar_update::error::{NetworkError, UpdateError};
use ultar_update::net::{tls_available, HttpSource, Roots, Timeouts};
use ultar_update::source::{Query, Remote, UpdateSource};
use ultar_update::stage::NoPacing;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tls");

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(format!("{FIXTURES}/{name}")).unwrap()
}

fn test_ca() -> Roots {
    let der = CertificateDer::from_pem_slice(&fixture("ca.pem")).unwrap();
    Roots::Certificates(vec![der.to_vec()])
}

/// A TLS front end on 127.0.0.1 presenting `cert`, forwarding to `backend` (plain HTTP).
struct TlsFront {
    base: String,
    handshakes_failed: Arc<AtomicUsize>,
}

fn server_config(cert: &str) -> Arc<rustls::ServerConfig> {
    let chain = vec![CertificateDer::from_pem_slice(&fixture(&format!("{cert}.pem"))).unwrap()];
    let key = PrivateKeyDer::from_pem_slice(&fixture(&format!("{cert}.key.pem"))).unwrap();
    let provider = Arc::new(rustls_graviola_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    Arc::new(config)
}

/// The same provider the client uses (re-exported through the update crate's dependency).
fn rustls_graviola_provider() -> rustls::crypto::CryptoProvider {
    rustls_graviola::default_provider()
}

/// Reads one request head, passes it to `backend`, returns the whole answer.
fn relay(tls: &mut impl Read, backend: std::net::SocketAddr) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if tls.read(&mut byte)? == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        head.push(byte[0]);
    }
    let mut plain = TcpStream::connect(backend)?;
    plain.write_all(&head)?;
    let mut answer = Vec::new();
    plain.read_to_end(&mut answer)?;
    Ok(answer)
}

impl TlsFront {
    fn start(cert: &str, backend: &TestServer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("https://{}", listener.local_addr().unwrap());
        let backend: std::net::SocketAddr =
            backend.base.trim_start_matches("http://").parse().unwrap();
        let config = server_config(cert);
        let handshakes_failed = Arc::new(AtomicUsize::new(0));
        let failed = handshakes_failed.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let connection = rustls::ServerConnection::new(config.clone()).unwrap();
                let mut tls = rustls::StreamOwned::new(connection, stream);
                match relay(&mut tls, backend) {
                    Ok(answer) => {
                        let _ = tls.write_all(&answer);
                        tls.conn.send_close_notify();
                        let _ = tls.flush();
                    }
                    Err(_) => {
                        failed.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }
        });
        Self {
            base,
            handshakes_failed,
        }
    }
}

fn backend_for(release: &Release) -> TestServer {
    TestServer::start(Behaviour {
        file: release.bundle.clone(),
        latest: Some(TestServer::latest_json(release.bundle.len() as u64)),
        ..Behaviour::default()
    })
}

fn source(base: &str, roots: &Roots) -> HttpSource {
    HttpSource::with(base, Some("tok".into()), roots, &Timeouts::default()).unwrap()
}

const QUERY: Query<'static> = Query {
    channel: "stable",
    arch: "x86_64",
    edition: "desktop",
};

#[test]
fn full_update_over_https_with_a_validated_certificate() {
    if !tls_available() {
        return; // documented: no TLS on this CPU; covered by `tls_unavailable_is_typed`
    }
    let device = Device::new();
    let new = release((0, 7, 1), 2);
    let backend = backend_for(&new);
    let front = TlsFront::start("server", &backend);
    let mut https = source(&front.base, &test_ca());
    let mut engine = device.engine();
    let offer = engine.check(&mut https, "stable").unwrap().unwrap();
    engine
        .download(&mut https, &offer, &mut NoPacing, &mut |_, _| {})
        .unwrap();
    engine.apply(&mut |_, _, _| {}).unwrap();
    assert_eq!(device.images(Slot::B), new.payloads);
    // Requests arrived through TLS, with the token, Range resume as over plain HTTP.
    let seen = backend.seen();
    assert_eq!(seen.len(), 3);
    assert!(seen
        .iter()
        .all(|s| s.authorization.as_deref() == Some("Bearer tok")));
    assert_eq!(front.handshakes_failed.load(Ordering::SeqCst), 0);
}

#[test]
fn certificate_of_an_unknown_authority_is_refused() {
    if !tls_available() {
        return;
    }
    let new = release((0, 7, 1), 2);
    let backend = backend_for(&new);
    let front = TlsFront::start("server", &backend);
    // The Mozilla roots do not know the test CA.
    let mut https = source(&front.base, &Roots::Mozilla);
    let error = https.latest(&QUERY).unwrap_err();
    assert!(
        matches!(error, UpdateError::Network(NetworkError::Transport)),
        "{error}"
    );
    // Nothing reached the server behind TLS – not even the request line with the token.
    assert!(backend.seen().is_empty());
    // Another, valid root list without the test CA: refused as well.
    let other_ca =
        Roots::Certificates(vec![CertificateDer::from_pem_slice(&fixture("server.pem"))
            .unwrap()
            .to_vec()]);
    let mut https = source(&front.base, &other_ca);
    assert!(https.latest(&QUERY).is_err());
    assert!(backend.seen().is_empty());
}

#[test]
fn certificate_for_another_name_is_refused() {
    if !tls_available() {
        return;
    }
    let new = release((0, 7, 1), 2);
    let backend = backend_for(&new);
    // Signed by the trusted test CA, but issued for other.example, not for 127.0.0.1.
    let front = TlsFront::start("other", &backend);
    let mut https = source(&front.base, &test_ca());
    let error = https.latest(&QUERY).unwrap_err();
    assert!(
        matches!(error, UpdateError::Network(NetworkError::Transport)),
        "{error}"
    );
    assert!(backend.seen().is_empty());
}

#[test]
fn https_source_never_follows_a_redirect_to_plain_http() {
    if !tls_available() {
        return;
    }
    let new = release((0, 7, 1), 2);
    let plain = backend_for(&new);
    let backend = backend_for(&new);
    backend.set(|b| b.redirect = Some(format!("{}/mirror/7", plain.base)));
    let front = TlsFront::start("server", &backend);
    let mut https = source(&front.base, &test_ca());
    let remote = https.latest(&QUERY).unwrap().unwrap();
    let error = https.open(&remote, 0, Some(64)).map(|_| ()).unwrap_err();
    assert!(
        matches!(error, UpdateError::Network(NetworkError::Protocol)),
        "{error}"
    );
    assert!(
        plain.seen().is_empty(),
        "the plain http server was contacted"
    );
}

#[test]
fn tls_handshake_that_never_comes_ends_with_a_time_out() {
    if !tls_available() {
        return;
    }
    // Accepts the connection, then says nothing.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("https://{}", listener.local_addr().unwrap());
    let held = std::thread::spawn(move || listener.accept().map(|(stream, _)| stream));
    let short = Timeouts {
        connect: Duration::from_secs(2),
        response: Duration::from_secs(2),
        per_read: Duration::from_secs(2),
    };
    let mut https = HttpSource::with(&base, None, &test_ca(), &short).unwrap();
    let started = Instant::now();
    let error = https.latest(&QUERY).unwrap_err();
    assert!(
        matches!(error, UpdateError::Network(NetworkError::Transport)),
        "{error}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "{:?}",
        started.elapsed()
    );
    drop(held.join());
}

#[test]
fn stalled_body_ends_with_a_time_out() {
    // Plain HTTP: the per-read limit is the same for both schemes.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut head = [0u8; 1024];
            let _ = stream.read(&mut head);
            let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100000\r\n\r\nUUP1");
            std::thread::sleep(Duration::from_secs(30));
        }
    });
    let short = Timeouts {
        connect: Duration::from_secs(2),
        response: Duration::from_secs(2),
        per_read: Duration::from_secs(2),
    };
    let mut plain = HttpSource::with(&base, None, &Roots::Mozilla, &short).unwrap();
    let remote = Remote {
        locator: "/v1/update/file/7".into(),
        size_hint: 0,
    };
    let (_, mut reader) = plain.open(&remote, 0, None).unwrap();
    let started = Instant::now();
    let mut buffer = vec![0u8; 100_000];
    let mut total = 0;
    let error = loop {
        match reader.read(&mut buffer[total..]) {
            Ok(0) => panic!("stream ended without an error"),
            Ok(n) => total += n,
            Err(error) => break error,
        }
    };
    assert_eq!(total, 4);
    assert!(started.elapsed() < Duration::from_secs(20), "{error}");
}

#[test]
fn tls_unavailable_is_typed_and_never_a_downgrade() {
    // Whatever this CPU can do: an https source is either fully validating or refused.
    match HttpSource::new("https://ultar.code-craft.tech", None) {
        Ok(_) => assert!(tls_available()),
        Err(error) => {
            assert!(!tls_available());
            assert!(matches!(error, UpdateError::TlsUnavailable));
            assert_eq!(error.key(), "update.error.tls_unavailable");
        }
    }
    // Plain http (local network server) never needs the TLS provider.
    assert!(HttpSource::new("http://192.168.1.10:8080", None).is_ok());
}
