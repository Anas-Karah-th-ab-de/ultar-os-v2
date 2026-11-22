//! Upgrade of a database written by the first server release (schema 0).
//!
//! `fixtures/old-v0.sqlite` was produced by building and running the ORIGINAL server code
//! (`/home/claude/ultar-os-v2/server/ultraos-server`, version 0.1.0): one account
//! (`fixture-user` / `fixture-pass-1`) with a device, its number, a WireGuard peer and a Verbund
//! device, created through its HTTP API; plus two store apps and four releases inserted with SQL.

mod common;
use common::TestServer;
use serde_json::json;

const NUMBER: &str = "UOS-C7E9-136D";
const WG_KEY: &str = "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

fn fixture_copy() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let source =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/old-v0.sqlite");
    let db = dir.path().join("live.sqlite");
    std::fs::copy(source, &db).unwrap();
    (dir, db)
}

fn user_version(path: &std::path::Path) -> i64 {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap()
}

#[test]
fn old_database_is_upgraded_in_place_and_account_data_survives() {
    let (dir, db) = fixture_copy();
    assert_eq!(user_version(&db), 0, "fixture really is the old schema");
    let s = TestServer::start_in(dir, &db, |_| {});
    assert_eq!(user_version(&db), 1);

    let login = s.post(
        "/v1/account/login",
        None,
        &json!({"username":"fixture-user","password":"fixture-pass-1"}),
    );
    assert_eq!(login.status, 200, "old password hash still verifies");
    let token = login.json()["token"].as_str().unwrap().to_string();
    assert_eq!(login.json()["account_id"], 1);

    let device = json!({"device_id":"dev-fixture-1","pubkey":"cHVia2V5"});
    assert_eq!(
        s.post("/v1/device/register", Some(&token), &device).json()["number"],
        NUMBER
    );
    let peer = s
        .post("/v1/vpn/peer", Some(&token), &json!({"pubkey": WG_KEY}))
        .json();
    assert_eq!(
        peer["address"], "10.80.0.2/32",
        "existing peer keeps its address"
    );
    let next = s.post(
        "/v1/vpn/peer",
        Some(&token),
        &json!({"pubkey": "AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE="}),
    );
    assert_eq!(
        next.json()["address"],
        "10.80.0.3/32",
        "allocation continues after the old peers"
    );
    let verbund = s.get_as("/v1/verbund/geraete", &token).json();
    assert_eq!(verbund["geraete"][0]["name"], "laptop");
}

#[test]
fn old_store_rows_become_public_catalogue_entries() {
    let (dir, db) = fixture_copy();
    let s = TestServer::start_in(dir, &db, |_| {});

    let legacy = s.get("/v1/store/apps").json();
    let apps = legacy.as_array().unwrap();
    assert_eq!(apps.len(), 2);
    assert_eq!(apps[0]["id"], "com.kmcc.notes");
    assert_eq!(
        apps[0]["url"], "https://cdn.example.invalid/apps/notes-1.0.0.uap",
        "old location is kept"
    );
    assert_eq!(apps[1]["url"], "/static/pos.uap");
    assert_eq!(apps[0]["premium"], false);

    let found = s.get("/v1/store/catalog?q=notes").json();
    assert_eq!(found["total"], 1, "search text was filled in for old rows");
    let details = s.get("/v1/store/apps/com.kmcc.pos").json();
    assert_eq!(details["category"], "other");
    assert_eq!(details["versions"].as_array().unwrap().len(), 1);
    assert_eq!(details["versions"][0]["version"], "1.2.0");
    assert_eq!(details["permissions"], json!([]));

    // Files of old rows live outside the server: free ones are redirected.
    let r = s.get("/v1/store/apps/com.kmcc.notes/download");
    assert_eq!(r.status, 307);
    assert_eq!(
        r.header("location"),
        Some("https://cdn.example.invalid/apps/notes-1.0.0.uap")
    );

    // Publishing a new version on top of an old row works and keeps the history.
    s.publish_app(
        json!({"id":"com.kmcc.notes","name":"KM Notes","version":"1.1.0"}),
        b"new-notes",
    );
    let current = s.get("/v1/store/apps/com.kmcc.notes/download");
    assert_eq!(
        (current.status, current.body.as_slice()),
        (200, &b"new-notes"[..])
    );
    assert_eq!(
        s.get("/v1/store/apps/com.kmcc.notes/download?version=1.0.0")
            .status,
        307
    );
    let versions = s.get("/v1/store/apps/com.kmcc.notes").json()["versions"]
        .as_array()
        .unwrap()
        .len();
    assert_eq!(versions, 2);
}

#[test]
fn old_releases_keep_their_channels_and_new_gated_channels_exist() {
    let (dir, db) = fixture_copy();
    let s = TestServer::start_in(dir, &db, |_| {});

    let stable = s.get("/v1/update/latest").json();
    assert_eq!(
        stable["version"], "0.6.1",
        "newest of the two old stable releases"
    );
    assert_eq!(stable["url"], "https://cdn.example.invalid/os/0.6.1.img");
    assert_eq!(stable["roothash"], "22".repeat(32));
    assert_eq!(
        (stable["arch"].as_str(), stable["edition"].as_str()),
        (Some("any"), Some("any"))
    );
    assert_eq!(
        s.get("/v1/update/latest?channel=stable&arch=aarch64&edition=phone")
            .json()["version"],
        "0.6.1"
    );
    assert_eq!(
        s.get_with("/v1/update/latest", &[("x-ultar-channel", "beta")])
            .json()["version"],
        "0.7.0-b1"
    );
    assert_eq!(
        s.get("/v1/update/latest?channel=nightly").json()["version"],
        "0.7.0-n5",
        "custom channel stays open"
    );

    let id = stable["id"].as_i64().unwrap();
    let file = s.get(&format!("/v1/update/file/{id}"));
    assert_eq!(
        (file.status, file.header("location")),
        (307, Some("https://cdn.example.invalid/os/0.6.1.img"))
    );

    assert_eq!(
        s.get("/v1/update/latest?channel=preview").error_code(),
        "auth_required"
    );
    assert_eq!(
        s.get("/v1/update/latest?channel=lts").error_code(),
        "auth_required"
    );
}

#[test]
fn restarting_on_the_upgraded_file_changes_nothing() {
    let (dir, db) = fixture_copy();
    drop(ultraos_server::db::Db::open(&db.to_string_lossy()).unwrap());
    assert_eq!(user_version(&db), 1);
    let s = TestServer::start_in(dir, &db, |_| {});
    assert_eq!(
        s.get("/v1/store/apps").json().as_array().unwrap().len(),
        2,
        "second start is a no-op"
    );
    assert_eq!(s.get("/v1/update/latest").json()["version"], "0.6.1");
    let login = s.post(
        "/v1/account/login",
        None,
        &json!({"username":"fixture-user","password":"fixture-pass-1"}),
    );
    assert_eq!(login.status, 200);
}

#[test]
fn database_of_a_newer_server_is_refused() {
    let (_dir, db) = fixture_copy();
    {
        let conn = rusqlite::Connection::open(&db).unwrap();
        conn.pragma_update(None, "user_version", 99).unwrap();
    }
    let err = ultraos_server::db::Db::open(&db.to_string_lossy())
        .err()
        .unwrap();
    assert!(matches!(
        err,
        ultraos_server::db::DbError::NewerSchema {
            found: 99,
            supported: 1
        }
    ));
    assert_eq!(user_version(&db), 99, "the file is left untouched");
}
