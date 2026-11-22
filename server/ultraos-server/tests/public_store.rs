//! Anonymous access: catalogue, search, categories, details, free download, validators, ranges.

mod common;
use common::{pattern, TestServer};
use serde_json::json;

fn seeded() -> (TestServer, Vec<u8>) {
    let s = TestServer::start();
    ultraos_server::admin::set_publisher(&s.state.db, "KM Code Craft", true).unwrap();
    let content = pattern(200_000);
    s.publish_app(
        json!({"id":"com.kmcc.sheets","name":"KM Sheets","version":"1.0.0","description":"Tables and charts",
               "category":"office","publisher":"KM Code Craft","permissions":["files"],
               "file_types":["xlsx","csv"],"installed_size":500000,"changelog":"first release"}),
        b"old-version-bytes",
    );
    s.publish_app(
        json!({"id":"com.kmcc.sheets","name":"KM Sheets","version":"1.1.0","description":"Tables and charts",
               "category":"office","publisher":"KM Code Craft","permissions":["files","network"],
               "file_types":["xlsx","csv"],"installed_size":600000,"changelog":"faster"}),
        &content,
    );
    s.publish_app(
        json!({"id":"org.example.notes","name":"مفكّرة","version":"2.0","description":"ملاحظات سريعة",
               "category":"office","publisher":"Someone","file_types":["txt"]}),
        b"notes-package",
    );
    s.publish_app(
        json!({"id":"com.kmcc.pro","name":"KM Pro Suite","version":"3.0","category":"business",
               "publisher":"KM Code Craft","premium":true}),
        b"premium-package-secret-bytes",
    );
    (s, content)
}

#[test]
fn catalogue_lists_pages_and_marks_premium_without_any_token() {
    let (s, _) = seeded();
    let r = s.get("/v1/store/catalog");
    assert_eq!(r.status, 200);
    let v = r.json();
    assert_eq!(v["total"], 3);
    assert_eq!(
        (v["page"].as_u64(), v["per_page"].as_u64()),
        (Some(1), Some(20))
    );
    let names: Vec<&str> = v["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["id"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        vec!["com.kmcc.pro", "com.kmcc.sheets", "org.example.notes"]
    );
    let pro = &v["items"][0];
    assert_eq!(pro["premium"], true);
    assert_eq!(
        pro["publisher"],
        json!({"name":"KM Code Craft","verified":true})
    );
    assert_eq!(v["items"][2]["publisher"]["verified"], false);
    assert!(pro.get("price").is_none(), "no invented price fields");

    let page2 = s.get("/v1/store/catalog?per_page=2&page=2").json();
    assert_eq!(page2["items"].as_array().unwrap().len(), 1);
    assert_eq!(page2["total"], 3);
    let beyond = s.get("/v1/store/catalog?per_page=2&page=9").json();
    assert!(beyond["items"].as_array().unwrap().is_empty());
}

#[test]
fn search_by_name_description_file_type_and_category() {
    let (s, _) = seeded();
    let ids = |path: &str| -> Vec<String> {
        let r = s.get(path);
        assert_eq!(r.status, 200, "{path}: {}", r.text());
        r.json()["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["id"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(ids("/v1/store/catalog?q=sheets"), vec!["com.kmcc.sheets"]);
    assert_eq!(ids("/v1/store/catalog?q=CHARTS"), vec!["com.kmcc.sheets"]);
    assert_eq!(ids("/v1/store/catalog?q=.xlsx"), vec!["com.kmcc.sheets"]);
    assert_eq!(
        ids("/v1/store/catalog?filetype=txt"),
        vec!["org.example.notes"]
    );
    assert_eq!(
        ids("/v1/store/catalog?category=business"),
        vec!["com.kmcc.pro"]
    );
    assert_eq!(
        ids("/v1/store/catalog?q=km&category=office"),
        vec!["com.kmcc.sheets"]
    );
    // Arabic query without the shadda that the stored name carries.
    assert_eq!(
        ids("/v1/store/catalog?q=%D9%85%D9%81%D9%83%D8%B1%D8%A9"),
        vec!["org.example.notes"]
    );
    assert!(ids("/v1/store/catalog?q=nothing-matches").is_empty());
    assert!(
        ids("/v1/store/catalog?q=%25%27%20OR%201%3D1").is_empty(),
        "wildcards and quotes are literal"
    );
}

#[test]
fn invalid_catalogue_parameters_are_typed_errors() {
    let (s, _) = seeded();
    let long_q = format!("/v1/store/catalog?q={}", "x".repeat(101));
    for path in [
        "/v1/store/catalog?page=0",
        "/v1/store/catalog?page=abc",
        "/v1/store/catalog?per_page=101",
        "/v1/store/catalog?per_page=-1",
        "/v1/store/catalog?category=..%2F..",
        "/v1/store/catalog?filetype=a%2Fb",
        long_q.as_str(),
    ] {
        let r = s.get(path);
        assert_eq!(
            (r.status, r.error_code().as_str()),
            (400, "invalid_request"),
            "{path}"
        );
    }
}

#[test]
fn categories_and_details() {
    let (s, content) = seeded();
    let cats = s.get("/v1/store/categories").json();
    assert_eq!(
        cats["categories"],
        json!([{"id":"business","count":1},{"id":"office","count":2}])
    );

    let d = s.get("/v1/store/apps/com.kmcc.sheets").json();
    assert_eq!(d["name"], "KM Sheets");
    assert_eq!(d["version"], "1.1.0");
    assert_eq!(d["size"], content.len());
    assert_eq!(d["installed_size"], 600000);
    assert_eq!(d["permissions"], json!(["files", "network"]));
    assert_eq!(d["file_types"], json!(["csv", "xlsx"]));
    assert_eq!(d["premium"], false);
    assert_eq!(
        d["publisher"],
        json!({"name":"KM Code Craft","verified":true})
    );
    assert_eq!(d["download"], "/v1/store/apps/com.kmcc.sheets/download");
    let versions = d["versions"].as_array().unwrap();
    assert_eq!(versions.len(), 2);
    assert_eq!(
        (
            versions[0]["version"].as_str(),
            versions[0]["changelog"].as_str()
        ),
        (Some("1.1.0"), Some("faster"))
    );
    assert_eq!(versions[1]["version"], "1.0.0");
    assert_eq!(d["sha256"].as_str().unwrap().len(), 64);

    let premium = s.get("/v1/store/apps/com.kmcc.pro");
    assert_eq!(premium.status, 200, "details of premium apps are public");
    assert_eq!(premium.json()["premium"], true);

    let missing = s.get("/v1/store/apps/no.such.app");
    assert_eq!(
        (missing.status, missing.error_code().as_str()),
        (404, "not_found")
    );
}

#[test]
fn free_package_downloads_completely_without_token() {
    let (s, content) = seeded();
    let r = s.get("/v1/store/apps/com.kmcc.sheets/download");
    assert_eq!(r.status, 200);
    assert_eq!(r.body, content);
    assert_eq!(r.header("content-length"), Some("200000"));
    assert_eq!(r.header("accept-ranges"), Some("bytes"));
    assert_eq!(r.header("content-type"), Some("application/octet-stream"));
    let sha = s.get("/v1/store/apps/com.kmcc.sheets").json()["sha256"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(r.header("etag"), Some(format!("\"{sha}\"").as_str()));

    let old = s.get("/v1/store/apps/com.kmcc.sheets/download?version=1.0.0");
    assert_eq!(
        (old.status, old.body.as_slice()),
        (200, &b"old-version-bytes"[..]),
        "roll back to a version"
    );
    let none = s.get("/v1/store/apps/com.kmcc.sheets/download?version=9.9.9");
    assert_eq!(
        (none.status, none.error_code().as_str()),
        (404, "not_found")
    );
    // An invalid token does not matter for free content.
    let with_bad_token = s.get_as("/v1/store/apps/com.kmcc.sheets/download", "garbage");
    assert_eq!(with_bad_token.status, 200);
}

#[test]
fn ranges_resume_downloads() {
    let (s, content) = seeded();
    let path = "/v1/store/apps/com.kmcc.sheets/download";
    let r = s.get_with(path, &[("range", "bytes=0-99")]);
    assert_eq!(r.status, 206);
    assert_eq!(r.header("content-range"), Some("bytes 0-99/200000"));
    assert_eq!(r.header("content-length"), Some("100"));
    assert_eq!(r.body, &content[..100]);

    // Crosses the server's internal chunk boundary.
    let r = s.get_with(path, &[("range", "bytes=65000-140000")]);
    assert_eq!(
        (r.status, r.body.as_slice()),
        (206, &content[65000..=140000])
    );

    let r = s.get_with(path, &[("range", "bytes=199990-")]);
    assert_eq!((r.status, r.body.as_slice()), (206, &content[199_990..]));
    assert_eq!(
        r.header("content-range"),
        Some("bytes 199990-199999/200000")
    );

    let r = s.get_with(path, &[("range", "bytes=-5")]);
    assert_eq!((r.status, r.body.as_slice()), (206, &content[199_995..]));

    let r = s.get_with(path, &[("range", "bytes=200000-")]);
    assert_eq!(
        (r.status, r.error_code().as_str()),
        (416, "range_not_satisfiable")
    );
    assert_eq!(r.header("content-range"), Some("bytes */200000"));

    let r = s.get_with(path, &[("range", "bytes=0-1,5-6")]);
    assert_eq!(
        (r.status, r.body.len()),
        (200, 200_000),
        "multi-range is answered with the whole file"
    );

    let etag = s.get(path).header("etag").unwrap().to_string();
    let r = s.get_with(path, &[("range", "bytes=0-9"), ("if-range", &etag)]);
    assert_eq!(r.status, 206);
    let r = s.get_with(path, &[("range", "bytes=0-9"), ("if-range", "\"stale\"")]);
    assert_eq!(
        (r.status, r.body.len()),
        (200, 200_000),
        "changed file → start over"
    );
}

#[test]
fn etags_avoid_repeated_transfers() {
    let (s, _) = seeded();
    for path in [
        "/v1/store/catalog",
        "/v1/store/catalog?q=sheets",
        "/v1/store/categories",
        "/v1/store/apps",
        "/v1/store/apps/com.kmcc.sheets",
        "/v1/store/apps/com.kmcc.sheets/download",
    ] {
        let first = s.get(path);
        assert_eq!(first.status, 200, "{path}");
        let etag = first
            .header("etag")
            .unwrap_or_else(|| panic!("{path}: no etag"))
            .to_string();
        let again = s.get_with(path, &[("if-none-match", &etag)]);
        assert_eq!(again.status, 304, "{path}");
        assert!(again.body.is_empty());
        assert_eq!(again.header("etag"), Some(etag.as_str()));
        let stale = s.get_with(path, &[("if-none-match", "\"something-else\"")]);
        assert_eq!(stale.status, 200, "{path}");
    }
    // The catalogue tag changes when the catalogue changes.
    let before = s
        .get("/v1/store/catalog")
        .header("etag")
        .unwrap()
        .to_string();
    s.publish_app(json!({"id":"new.app","name":"New","version":"1"}), b"n");
    let after = s.get_with("/v1/store/catalog", &[("if-none-match", &before)]);
    assert_eq!(after.status, 200);
    assert_ne!(after.header("etag"), Some(before.as_str()));
}

#[test]
fn head_requests_and_legacy_listing() {
    let (s, _) = seeded();
    let head = common::request(
        "HEAD",
        &s.url("/v1/store/apps/com.kmcc.sheets/download"),
        &[],
        None,
    );
    assert_eq!(head.status, 200);
    assert_eq!(head.header("content-length"), Some("200000"));
    assert!(head.body.is_empty());

    // Shape older devices parse: a bare array with id, name, version, size, sha256, sig, url.
    let list = s.get("/v1/store/apps").json();
    let apps = list.as_array().unwrap();
    assert_eq!(apps.len(), 3);
    let sheets = apps.iter().find(|a| a["id"] == "com.kmcc.sheets").unwrap();
    for field in ["id", "name", "version", "size", "sha256", "sig", "url"] {
        assert!(sheets.get(field).is_some(), "{field}");
    }
    assert_eq!(sheets["url"], "/v1/store/apps/com.kmcc.sheets/download");
}

#[test]
fn health_and_unknown_routes() {
    let s = TestServer::start();
    let r = s.get("/v1/health");
    assert_eq!((r.status, r.text().as_str()), (200, "ok"));
    let r = s.get("/v1/nope");
    assert_eq!((r.status, r.error_code().as_str()), (404, "not_found"));
    let empty = s.get("/v1/store/catalog").json();
    assert_eq!(
        (
            empty["total"].as_i64(),
            empty["items"].as_array().map(Vec::len)
        ),
        (Some(0), Some(0))
    );
}
