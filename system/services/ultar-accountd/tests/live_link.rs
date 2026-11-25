//! Live check of the account link against the release server (not part of the normal run):
//! the service's own code (`Link` + `CloudServer`) signs in, registers this device, hands the
//! relay its token, lists the devices and signs out.
//!
//! ```text
//! ULTAR_LIVE_USER=<account> ULTAR_LIVE_PASSWORD=<password> \
//!   cargo test --release -p ultar-accountd --test live_link -- --ignored --nocapture
//! ```
//! `ULTAR_LIVE_SERVER` defaults to `ultar.code-craft.tech`. Every run registers a new device in
//! the account (a fresh key each time); the server limits how many an account may ever register,
//! so run it rarely or remove the test devices.

use std::time::Instant;
use ultar_accountd::link::{Link, LinkCall, DEFAULT_SERVER};
use ultar_accountd::link_cloud::CloudServer;
use ultar_accountd::link_store::Store;
use ultar_bus::messages::account_service::{Phase, SignIn};

#[test]
#[ignore = "needs the network and an account on the live server"]
fn the_account_link_works_with_the_live_server() {
    let var = |n: &str| std::env::var(n).unwrap_or_else(|_| panic!("{n} is not set"));
    let server = std::env::var("ULTAR_LIVE_SERVER").unwrap_or_else(|_| DEFAULT_SERVER.into());
    let dir = tempfile::tempdir().unwrap();
    let relay = dir.path().join("relay/relay.token");
    let store = Store::new(dir.path(), Some(relay.clone()), None);
    let mut link = Link::open(
        CloudServer::new(dir.path()),
        &server,
        store,
        "live-link",
        "desktop",
    );
    let now = Instant::now();
    let st = link
        .call(
            LinkCall::SignIn(SignIn {
                user: var("ULTAR_LIVE_USER"),
                password: var("ULTAR_LIVE_PASSWORD"),
            }),
            now,
        )
        .outcome
        .expect("sign in");
    assert_eq!(st.phase, Phase::SignedIn);
    assert!(st.number.starts_with("UOS-"), "{}", st.number);
    assert!(st.devices.iter().any(|d| d.this_device), "{:?}", st.devices);
    assert!(!std::fs::read_to_string(&relay).unwrap().is_empty());
    println!(
        "ok signed in as {} on {}, number {}, {} devices",
        st.user,
        st.server,
        st.number,
        st.devices.len()
    );
    // a new start finds the account and the server again
    drop(link);
    let store = Store::new(dir.path(), Some(relay.clone()), None);
    let mut again = Link::open(
        CloudServer::new(dir.path()),
        &server,
        store,
        "live-link",
        "desktop",
    );
    again.refresh();
    assert_eq!(again.state(now).phase, Phase::SignedIn);
    // signing out ends the link here; the test device stays in the account
    let me = again
        .state(now)
        .devices
        .iter()
        .find(|d| d.this_device)
        .map(|d| d.id.clone())
        .unwrap();
    let st = again.call(LinkCall::SignOut, now).outcome.unwrap();
    assert_eq!(st.phase, Phase::SignedOut);
    assert!(!relay.exists());
    println!("ok second start signed in again; signed out (device {me})");
}
