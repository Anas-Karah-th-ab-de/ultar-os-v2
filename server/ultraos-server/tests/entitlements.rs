//! Premium apps and gated update channels: token + entitlement; free channels: no account.

mod common;
use common::TestServer;
use serde_json::json;
use ultraos_server::admin;
use ultraos_server::entitlements::EntitlementKind;

const PREMIUM: &[u8] = b"premium-package-bytes";

fn with_premium_app() -> TestServer {
    let s = TestServer::start();
    s.publish_app(
        json!({"id":"com.kmcc.pro","name":"Pro","version":"1.0","premium":true}),
        PREMIUM,
    );
    s.publish_app(
        json!({"id":"com.kmcc.free","name":"Free","version":"1.0"}),
        b"free-package",
    );
    s
}

#[test]
fn premium_app_needs_token_then_entitlement_and_revocation_takes_effect() {
    let s = with_premium_app();
    let path = "/v1/store/apps/com.kmcc.pro/download";

    let anonymous = s.get(path);
    assert_eq!(
        (anonymous.status, anonymous.error_code().as_str()),
        (401, "auth_required")
    );
    assert_eq!(anonymous.header("www-authenticate"), Some("Bearer"));
    assert!(!anonymous.text().contains("premium-package"));

    let token = s.register("anna");
    let signed_in = s.get_as(path, &token);
    assert_eq!(
        (signed_in.status, signed_in.error_code().as_str()),
        (403, "not_entitled")
    );

    assert!(admin::grant(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap());
    let entitled = s.get_as(path, &token);
    assert_eq!((entitled.status, entitled.body.as_slice()), (200, PREMIUM));
    assert_eq!(entitled.header("cache-control"), Some("private, no-store"));

    // Another account is not affected by anna's entitlement.
    let other = s.register("bert");
    assert_eq!(s.get_as(path, &other).error_code(), "not_entitled");

    assert!(admin::revoke(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap());
    let revoked = s.get_as(path, &token);
    assert_eq!(
        (revoked.status, revoked.error_code().as_str()),
        (403, "not_entitled")
    );
}

#[test]
fn forged_expired_and_foreign_tokens_count_as_missing() {
    let s = with_premium_app();
    let path = "/v1/store/apps/com.kmcc.pro/download";
    let token = s.register("anna");
    admin::grant(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap();

    let other_key = ultraos_server::auth::issue_token(&[0x22; 32], 1, 3600).unwrap();
    let expired = ultraos_server::auth::issue_token(&common::SECRET, 1, -10).unwrap();
    let mut tampered = token.clone();
    tampered.pop();
    tampered.push(if token.ends_with('A') { 'B' } else { 'A' });
    for bad in [
        other_key.as_str(),
        expired.as_str(),
        tampered.as_str(),
        "",
        "x.y",
        "not-a-token",
    ] {
        let r = s.get_as(path, bad);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (401, "auth_required"),
            "{bad}"
        );
    }
    let basic = s.get_with(path, &[("authorization", "Basic YW5uYTpwdw==")]);
    assert_eq!(basic.error_code(), "auth_required");
    assert_eq!(
        s.get_as(path, &token).status,
        200,
        "the real token still works"
    );
}

#[test]
fn entitlement_is_per_app_and_ranges_and_etags_work_behind_the_gate() {
    let s = with_premium_app();
    s.publish_app(
        json!({"id":"com.kmcc.other","name":"Other","version":"1","premium":true}),
        b"other",
    );
    let token = s.register("anna");
    admin::grant(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap();
    assert_eq!(
        s.get_as("/v1/store/apps/com.kmcc.other/download", &token)
            .error_code(),
        "not_entitled"
    );

    let path = "/v1/store/apps/com.kmcc.pro/download";
    let auth = format!("Bearer {token}");
    let part = s.get_with(path, &[("authorization", &auth), ("range", "bytes=0-6")]);
    assert_eq!((part.status, part.body.as_slice()), (206, &PREMIUM[..7]));
    let etag = part.header("etag").unwrap().to_string();
    let cached = s.get_with(path, &[("authorization", &auth), ("if-none-match", &etag)]);
    assert_eq!(cached.status, 304);
    // Validators never bypass the gate.
    let anonymous = s.get_with(path, &[("if-none-match", &etag), ("range", "bytes=0-6")]);
    assert_eq!(anonymous.error_code(), "auth_required");
}

#[test]
fn making_an_app_premium_later_closes_the_download() {
    let s = with_premium_app();
    let path = "/v1/store/apps/com.kmcc.free/download";
    assert_eq!(s.get(path).status, 200);
    admin::set_premium(&s.state.db, "com.kmcc.free", true).unwrap();
    assert_eq!(s.get(path).error_code(), "auth_required");
    assert_eq!(
        s.get(&format!("{path}?version=1.0")).error_code(),
        "auth_required",
        "old versions too"
    );
    admin::set_premium(&s.state.db, "com.kmcc.free", false).unwrap();
    assert_eq!(s.get(path).status, 200);
}

fn with_releases() -> (TestServer, i64, i64) {
    let s = TestServer::start();
    let free = s.publish_release(
        json!({"channel":"stable","version":"0.7.0","arch":"aarch64","edition":"phone","roothash":"aa11","sig":"c2ln"}),
        b"stable-image",
    );
    let gated = s.publish_release(
        json!({"channel":"preview","version":"0.8.0-pre","arch":"aarch64","edition":"phone","roothash":"bb22","sig":"c2ln"}),
        b"preview-image",
    );
    (s, free, gated)
}

#[test]
fn free_channel_update_works_without_account() {
    let (s, free, _) = with_releases();
    let r = s.get("/v1/update/latest?channel=stable&arch=aarch64&edition=phone");
    assert_eq!(r.status, 200);
    let v = r.json();
    assert_eq!(v["version"], "0.7.0");
    assert_eq!(v["channel"], "stable");
    assert_eq!(v["roothash"], "aa11");
    assert_eq!(v["sig"], "c2ln");
    assert_eq!(v["size"], 12);
    assert_eq!(v["url"], format!("/v1/update/file/{free}"));
    let file = s.get(v["url"].as_str().unwrap());
    assert_eq!(
        (file.status, file.body.as_slice()),
        (200, &b"stable-image"[..])
    );
    let part = s.get_with(v["url"].as_str().unwrap(), &[("range", "bytes=7-")]);
    assert_eq!((part.status, part.body.as_slice()), (206, &b"image"[..]));

    // Defaults: channel stable; a device of another architecture gets nothing (204).
    assert_eq!(
        s.get("/v1/update/latest?arch=aarch64&edition=phone").json()["version"],
        "0.7.0"
    );
    assert_eq!(
        s.get("/v1/update/latest?channel=stable&arch=x86_64&edition=phone")
            .status,
        204
    );
    assert_eq!(
        s.get("/v1/update/latest").status,
        204,
        "no arch given: only arch-neutral releases"
    );
    // Older devices send the channel as a header.
    let legacy = s.get_with(
        "/v1/update/latest?arch=aarch64&edition=phone",
        &[("x-ultar-channel", "stable")],
    );
    assert_eq!(legacy.json()["version"], "0.7.0");
    // ETag on the metadata.
    let etag = r.header("etag").unwrap().to_string();
    let again = s.get_with(
        "/v1/update/latest?channel=stable&arch=aarch64&edition=phone",
        &[("if-none-match", &etag)],
    );
    assert_eq!(again.status, 304);
}

#[test]
fn gated_channel_needs_token_and_entitlement_for_metadata_and_file() {
    let (s, _, gated) = with_releases();
    let latest = "/v1/update/latest?channel=preview&arch=aarch64&edition=phone";
    let file = format!("/v1/update/file/{gated}");

    for path in [latest, file.as_str()] {
        let r = s.get(path);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (401, "auth_required"),
            "{path}"
        );
        assert!(!r.text().contains("preview-image") && !r.text().contains("0.8.0"));
    }
    let token = s.register("anna");
    for path in [latest, file.as_str()] {
        let r = s.get_as(path, &token);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (403, "not_entitled"),
            "{path}"
        );
    }
    admin::grant(&s.state.db, "anna", EntitlementKind::Channel, "preview").unwrap();
    assert_eq!(s.get_as(latest, &token).json()["version"], "0.8.0-pre");
    let image = s.get_as(&file, &token);
    assert_eq!(
        (image.status, image.body.as_slice()),
        (200, &b"preview-image"[..])
    );
    // An entitlement for one channel does not open another gated channel.
    assert_eq!(
        s.get_as("/v1/update/latest?channel=lts", &token)
            .error_code(),
        "not_entitled"
    );
    // An app entitlement of the same name is not a channel entitlement.
    admin::revoke(&s.state.db, "anna", EntitlementKind::Channel, "preview").unwrap();
    for path in [latest, file.as_str()] {
        assert_eq!(
            s.get_as(path, &token).error_code(),
            "not_entitled",
            "{path}"
        );
    }
}

#[test]
fn unknown_and_malformed_channels() {
    let (s, _, _) = with_releases();
    let r = s.get("/v1/update/latest?channel=does-not-exist");
    assert_eq!((r.status, r.error_code().as_str()), (404, "not_found"));
    let r = s.get("/v1/update/latest?channel=..%2F..");
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (400, "invalid_request")
    );
    let r = s.get("/v1/update/latest?arch=a%20b");
    assert_eq!(r.error_code(), "invalid_request");
    let r = s.get("/v1/update/file/999999");
    assert_eq!((r.status, r.error_code().as_str()), (404, "not_found"));
}

#[test]
fn gating_a_channel_later_closes_it_and_opening_it_releases_it() {
    let (s, free, _) = with_releases();
    let file = format!("/v1/update/file/{free}");
    admin::set_channel(&s.state.db, "stable", true).unwrap();
    assert_eq!(s.get(&file).error_code(), "auth_required");
    assert_eq!(
        s.get("/v1/update/latest?channel=stable&arch=aarch64")
            .error_code(),
        "auth_required"
    );
    admin::set_channel(&s.state.db, "stable", false).unwrap();
    assert_eq!(s.get(&file).status, 200);
}

#[test]
fn account_can_list_its_own_entitlements() {
    let (s, _, _) = with_releases();
    s.publish_app(
        json!({"id":"com.kmcc.pro","name":"Pro","version":"1.0","premium":true}),
        PREMIUM,
    );
    let anonymous = s.get("/v1/account/entitlements");
    assert_eq!(
        (anonymous.status, anonymous.error_code().as_str()),
        (401, "auth_required")
    );
    let (anna, bert) = (s.register("anna"), s.register("bert"));
    assert_eq!(
        s.get_as("/v1/account/entitlements", &anna).json(),
        json!({"apps":[],"channels":[]})
    );
    admin::grant(&s.state.db, "anna", EntitlementKind::App, "com.kmcc.pro").unwrap();
    admin::grant(&s.state.db, "anna", EntitlementKind::Channel, "preview").unwrap();
    let listed = s.get_as("/v1/account/entitlements", &anna).json();
    assert_eq!(
        listed,
        json!({"apps":["com.kmcc.pro"],"channels":["preview"]})
    );
    assert_eq!(
        s.get_as("/v1/account/entitlements", &bert).json(),
        json!({"apps":[],"channels":[]})
    );
}
