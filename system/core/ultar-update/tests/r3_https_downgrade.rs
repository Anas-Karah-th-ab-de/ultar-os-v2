//! r3 finding F7 (minor): a source configured with an `https://` server follows a location
//! (from `update/latest` or from a redirect) to plain `http://` on any host, including
//! loopback and LAN addresses. Integrity still rests on the signature, but the transfer loses
//! its confidentiality, an on-path attacker can make every download fail after the full
//! transfer (hash mismatch, discarded, fetched again), and the server can make the daemon
//! send GET requests into the local network.
//!
//! Expected: an https source refuses non-https locations.

mod common;
#[path = "common/http.rs"]
mod http;

use common::*;
use http::{Behaviour, TestServer};
use ultar_update::net::HttpSource;
use ultar_update::source::{Remote, UpdateSource};

#[test]
fn https_source_does_not_fetch_over_plain_http() {
    let new = release((0, 7, 1), 2);
    let plain = TestServer::start(Behaviour {
        file: new.bundle.clone(),
        ..Behaviour::default()
    });
    // Port 9 (discard) is never contacted: the location below is absolute.
    let mut source = HttpSource::new("https://127.0.0.1:9", None).unwrap();
    let remote = Remote {
        locator: format!("{}/mirror/7", plain.base),
        size_hint: 0,
    };
    let opened = source.open(&remote, 0, Some(16)).map(|(start, _)| start);
    let seen = plain.seen();
    assert!(
        opened.is_err() && seen.is_empty(),
        "fetched over http: {opened:?}, requests {seen:?}"
    );
}
