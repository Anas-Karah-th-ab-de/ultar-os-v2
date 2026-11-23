//! Review round 4 (R4-1): a stranger's wrong passwords must not lock the owner out. Failures are
//! counted per account **and client**; per account they only slow checks down (and stop them at a
//! high cap); a managing device of the account is never slowed or stopped by others' failures.

mod common;
use common::device::TestDevice;
use common::TestServer;
use serde_json::json;
use std::time::{Duration, Instant};

const PASSWORD: &str = "password-123";

/// A login from `client` (through the trusted proxy on loopback, which names the client).
fn login_from(s: &TestServer, client: &str, password: &str) -> (u16, String) {
    let body = json!({"username": "anna", "password": password}).to_string();
    let headers = [
        ("content-type", "application/json"),
        ("x-forwarded-for", client),
    ];
    let reply = common::request("POST", &s.url("/v1/account/login"), &headers, Some(&body));
    let code = if reply.status == 200 {
        String::new()
    } else {
        reply.error_code()
    };
    (reply.status, code)
}

#[test]
fn the_stranger_is_stopped_the_owner_only_waits_a_little() {
    let s = TestServer::start_with(|c| c.devices.password_failures = 12);
    s.register("anna");
    let stranger = "203.0.113.66";
    for guess in 0..12 {
        assert_eq!(login_from(&s, stranger, &format!("guess-{guess}-x")).0, 401);
    }
    // the stranger's address is stopped for this account – also with the right password
    let (status, code) = login_from(&s, stranger, PASSWORD);
    assert_eq!((status, code.as_str()), (429, "rate_limited"));
    // the owner, from his own address, gets in; the account's failures only make him wait
    let begun = Instant::now();
    assert_eq!(login_from(&s, "198.51.100.20", PASSWORD).0, 200);
    let waited = begun.elapsed();
    assert!(
        waited >= Duration::from_secs(4) && waited < Duration::from_secs(9),
        "{waited:?}"
    );
    // a right password gives its place back: the owner's next login waits the same, not more
    assert_eq!(login_from(&s, "198.51.100.20", PASSWORD).0, 200);
}

#[test]
fn a_managing_device_changes_the_password_without_waiting() {
    let s = TestServer::start_with(|c| c.devices.password_failures = 12);
    let token = s.register("anna");
    let phone = TestDevice::new(1);
    phone.register(&s, &token);
    for guess in 0..12 {
        login_from(&s, "203.0.113.66", &format!("guess-{guess}-x"));
    }
    let device_token = phone.session(&s);
    let body = json!({"old_password": PASSWORD, "new_password": "a-new-password-1"});
    let begun = Instant::now();
    let reply = s.post("/v1/account/password", Some(&device_token), &body);
    assert_eq!(reply.status, 200, "{}", reply.text());
    assert!(
        begun.elapsed() < Duration::from_secs(1),
        "{:?}",
        begun.elapsed()
    );
}
