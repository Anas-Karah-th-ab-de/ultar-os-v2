//! Abuse protection: rate limiting with an injected clock, size limits, traversal attempts.

mod common;
use common::TestServer;
use serde_json::json;
use ultraos_server::ratelimit::BucketConfig;

fn limited() -> TestServer {
    TestServer::start_with(|c| {
        c.limits.general = BucketConfig {
            capacity: 3,
            refill_per_minute: 60,
        };
        c.limits.download = BucketConfig {
            capacity: 2,
            refill_per_minute: 6,
        };
        c.limits.auth = BucketConfig {
            capacity: 2,
            refill_per_minute: 6,
        };
    })
}

#[test]
fn rate_limit_hits_and_recovers_with_the_injected_clock() {
    let s = limited();
    let ip = [("x-forwarded-for", "198.51.100.7")];
    for _ in 0..3 {
        assert_eq!(s.get_with("/v1/store/catalog", &ip).status, 200);
    }
    let blocked = s.get_with("/v1/store/catalog", &ip);
    assert_eq!(
        (blocked.status, blocked.error_code().as_str()),
        (429, "rate_limited")
    );
    assert_eq!(blocked.header("retry-after"), Some("1"));

    s.clock.advance_millis(999);
    assert_eq!(s.get_with("/v1/store/catalog", &ip).status, 429, "not yet");
    s.clock.advance_millis(1);
    assert_eq!(
        s.get_with("/v1/store/catalog", &ip).status,
        200,
        "one token refilled"
    );
    assert_eq!(s.get_with("/v1/store/catalog", &ip).status, 429);
    s.clock.advance_millis(60_000);
    for _ in 0..3 {
        assert_eq!(
            s.get_with("/v1/store/catalog", &ip).status,
            200,
            "full burst after a quiet window"
        );
    }
}

#[test]
fn limits_are_per_client_and_per_class() {
    let s = limited();
    s.publish_app(json!({"id":"a.free","name":"Free","version":"1"}), b"pkg");
    let a = [("x-forwarded-for", "198.51.100.7")];
    let b = [("x-forwarded-for", "198.51.100.8")];
    for _ in 0..3 {
        assert_eq!(s.get_with("/v1/store/catalog", &a).status, 200);
    }
    assert_eq!(s.get_with("/v1/store/catalog", &a).status, 429);
    assert_eq!(
        s.get_with("/v1/store/catalog", &b).status,
        200,
        "another client is unaffected"
    );
    // Downloads have their own (smaller) budget.
    assert_eq!(s.get_with("/v1/store/apps/a.free/download", &a).status, 200);
    assert_eq!(s.get_with("/v1/store/apps/a.free/download", &a).status, 200);
    let blocked = s.get_with("/v1/store/apps/a.free/download", &a);
    assert_eq!(
        (blocked.status, blocked.header("retry-after")),
        (429, Some("10"))
    );
    // The health check is never limited.
    for _ in 0..20 {
        assert_eq!(s.get_with("/v1/health", &a).status, 200);
    }
}

#[test]
fn login_and_registration_are_throttled() {
    let s = limited();
    let hdr = [
        ("x-forwarded-for", "203.0.113.9"),
        ("content-type", "application/json"),
    ];
    let body = json!({"username":"nobody","password":"wrong-password"}).to_string();
    let login = || common::request("POST", &s.url("/v1/account/login"), &hdr, Some(&body));
    assert_eq!(login().error_code(), "invalid_credentials");
    assert_eq!(login().error_code(), "invalid_credentials");
    let blocked = login();
    assert_eq!(
        (blocked.status, blocked.error_code().as_str()),
        (429, "rate_limited")
    );
    s.clock.advance_millis(10_000);
    assert_eq!(
        login().error_code(),
        "invalid_credentials",
        "one attempt per ten seconds afterwards"
    );
}

#[test]
fn forwarded_header_from_an_untrusted_peer_cannot_pick_a_bucket() {
    let s = TestServer::start_with(|c| {
        c.limits.general = BucketConfig {
            capacity: 2,
            refill_per_minute: 60,
        };
        c.trusted_proxies.clear();
    });
    // Every request claims a different address; all are charged to the real peer.
    assert_eq!(
        s.get_with("/v1/store/catalog", &[("x-forwarded-for", "1.1.1.1")])
            .status,
        200
    );
    assert_eq!(
        s.get_with("/v1/store/catalog", &[("x-forwarded-for", "2.2.2.2")])
            .status,
        200
    );
    assert_eq!(
        s.get_with("/v1/store/catalog", &[("x-forwarded-for", "3.3.3.3")])
            .status,
        429
    );
}

#[test]
fn client_keys_are_forgotten_after_the_window() {
    let s = limited();
    for n in 1..=5 {
        let ip = format!("198.51.100.{n}");
        assert_eq!(
            s.get_with("/v1/store/catalog", &[("x-forwarded-for", &ip)])
                .status,
            200
        );
    }
    assert_eq!(s.state.limiter.len(), 5);
    let window = s.state.limiter.config().general.window_millis();
    s.clock.advance_millis(window);
    assert_eq!(s.state.limiter.sweep(), 5);
    assert!(
        s.state.limiter.is_empty(),
        "no client address is kept beyond the refill window"
    );
}

#[test]
fn oversized_requests_are_rejected() {
    let s = TestServer::start();
    let long = format!("/v1/store/catalog?q={}", "a".repeat(3000));
    let r = s.get(&long);
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (413, "payload_too_large")
    );

    let big = json!({"username":"u".repeat(20_000),"password":"password-123"});
    let r = s.post("/v1/account/register", None, &big);
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (413, "payload_too_large")
    );

    let r = common::request(
        "POST",
        &s.url("/v1/account/register"),
        &[("content-type", "application/json")],
        Some("{not json"),
    );
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (400, "invalid_request")
    );
    let r = common::request("POST", &s.url("/v1/account/register"), &[], Some("{}"));
    assert_eq!(r.error_code(), "invalid_request", "missing content type");
}

fn traversal_targets(release: i64, premium_sha: &str, free_sha: &str) -> Vec<String> {
    vec![
        "/v1/store/apps/..%2F..%2Fsecret.txt/download".to_string(),
        "/v1/store/apps/%2e%2e/download".to_string(),
        "/v1/store/apps/../../secret.txt/download".to_string(),
        "/v1/store/apps/..%5C..%5Csecret.txt/download".to_string(),
        "/v1/store/apps/a.free/download?version=..%2F..%2Fsecret.txt".to_string(),
        "/v1/store/apps/a.free/download?version=%2Fetc%2Fpasswd".to_string(),
        "/v1/store/apps/a.free%00.txt/download".to_string(),
        "/v1/store/apps/%2Fetc%2Fpasswd".to_string(),
        "/v1/store/../../etc/passwd".to_string(),
        "/v1/store/apps/a.free/download/../../../../secret.txt".to_string(),
        "//etc/passwd".to_string(),
        "/v1/update/file/..%2F..%2Fsecret.txt".to_string(),
        "/v1/update/file/%2e%2e%2fserver.sqlite".to_string(),
        "/v1/update/file/1%2F..%2F2".to_string(),
        format!("/v1/update/file/{release}%00"),
        // File names of the store (digests) are not addresses.
        format!("/v1/store/apps/{premium_sha}/download"),
        format!("/v1/store/apps/a.free/download?version={premium_sha}"),
        format!("/v1/update/file/{premium_sha}"),
        format!("/files/{premium_sha}"),
        format!("/v1/store/apps/a.free/download/{free_sha}"),
    ]
}

#[test]
fn traversal_attempts_never_leave_the_file_store() {
    let s = TestServer::start();
    let free_sha = s.publish_app(
        json!({"id":"a.free","name":"Free","version":"1.0"}),
        b"free-package",
    );
    let premium_sha = s.publish_app(
        json!({"id":"a.pro","name":"Pro","version":"1.0","premium":true}),
        b"PREMIUM-SECRET",
    );
    let release = s.publish_release(
        json!({"channel":"preview","version":"1","roothash":"ab","sig":"s"}),
        b"GATED-IMAGE-SECRET",
    );
    // A secret next to the store and the database itself must stay unreachable.
    s.write_file("secret.txt", b"TOP-SECRET-OUTSIDE");
    let targets = traversal_targets(release, &premium_sha, &free_sha);
    for target in &targets {
        let raw = s.raw_get(target);
        let status: u16 = raw
            .split_whitespace()
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        assert!(
            status == 404 || status == 400,
            "{target}: unexpected status {status}\n{raw}"
        );
        for secret in [
            "TOP-SECRET",
            "PREMIUM-SECRET",
            "GATED-IMAGE",
            "root:",
            "SQLite format",
        ] {
            assert!(!raw.contains(secret), "{target} leaked {secret}");
        }
    }
    // The gated items are still gated, and the free one still works.
    assert_eq!(
        s.get("/v1/store/apps/a.pro/download").error_code(),
        "auth_required"
    );
    assert_eq!(
        s.get(&format!("/v1/update/file/{release}")).error_code(),
        "auth_required"
    );
    assert_eq!(
        s.get("/v1/store/apps/a.free/download").body,
        b"free-package"
    );
}

#[test]
fn file_missing_or_damaged_on_disk_is_reported_not_served() {
    let s = TestServer::start();
    let sha = s.publish_app(
        json!({"id":"a.free","name":"Free","version":"1.0"}),
        b"free-package",
    );
    let path = s.dir.path().join("files").join(&sha);
    std::fs::write(&path, b"truncated").unwrap();
    let r = s.get("/v1/store/apps/a.free/download");
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (500, "internal"),
        "size mismatch is detected"
    );
    std::fs::remove_file(&path).unwrap();
    let r = s.get("/v1/store/apps/a.free/download");
    assert_eq!((r.status, r.error_code().as_str()), (404, "not_found"));
}
