//! Unit tests for the admin command line.

use super::*;
use crate::msg::{self, Lang};

struct Env {
    db: Db,
    files: FileStore,
    dir: tempfile::TempDir,
}

fn env() -> Env {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(":memory:").unwrap();
    db.lock()
        .execute(
            "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'anna','h',0)",
            [],
        )
        .unwrap();
    let files = FileStore::new(dir.path().join("store"));
    Env { db, files, dir }
}

fn run_words(e: &Env, words: &[&str]) -> Result<Outcome, CliError> {
    let args: Vec<String> = words.iter().map(|w| (*w).to_string()).collect();
    run(&args, &e.db, &e.files)
}

fn write(e: &Env, name: &str, content: &str) -> String {
    let path = e.dir.path().join(name);
    std::fs::write(&path, content).unwrap();
    path.to_string_lossy().into_owned()
}

fn key(e: &Env, words: &[&str]) -> &'static str {
    run_words(e, words).unwrap().key
}

fn publish_pro(e: &Env) -> String {
    let meta = r#"{"id":"a.pro","name":"Pro","version":"1.0","premium":true}"#;
    let meta = write(e, "app.json", meta);
    let pkg = write(e, "pro.uap", "package");
    let out = run_words(e, &["publish-app", &meta, &pkg]).unwrap();
    assert_eq!(out.key, "admin.published.app");
    assert_eq!(&out.args[..3], ["a.pro", "1.0", "7"]);
    pkg
}

#[test]
fn publication_and_entitlement_flow() {
    let e = env();
    publish_pro(&e);
    assert_eq!(key(&e, &["grant", "anna", "app", "a.pro"]), "admin.granted");
    assert_eq!(
        key(&e, &["grant", "anna", "app", "a.pro"]),
        "admin.already_granted"
    );
    assert_eq!(key(&e, &["channel", "lts", "gated"]), "admin.channel.gated");
    assert_eq!(
        key(&e, &["grant", "anna", "channel", "lts"]),
        "admin.granted"
    );
    let listed = run_words(&e, &["entitlements", "anna"]).unwrap();
    assert_eq!(listed.key, "admin.entitlements.header");
    assert_eq!(listed.lines, vec!["app a.pro", "channel lts"]);
    assert_eq!(
        key(&e, &["revoke", "anna", "app", "a.pro"]),
        "admin.revoked"
    );
    assert_eq!(
        key(&e, &["revoke", "anna", "app", "a.pro"]),
        "admin.not_granted"
    );
    assert_eq!(
        key(&e, &["revoke", "anna", "channel", "lts"]),
        "admin.revoked"
    );
    assert_eq!(
        key(&e, &["entitlements", "anna"]),
        "admin.entitlements.none"
    );
}

#[test]
fn settings_and_release_publication() {
    let e = env();
    let pkg = publish_pro(&e);
    assert_eq!(key(&e, &["channel", "lts", "open"]), "admin.channel.open");
    assert_eq!(
        key(&e, &["publisher", "KM", "verified"]),
        "admin.publisher.verified"
    );
    assert_eq!(
        key(&e, &["publisher", "KM", "unverified"]),
        "admin.publisher.unverified"
    );
    assert_eq!(key(&e, &["premium", "a.pro", "off"]), "admin.premium.off");
    assert_eq!(key(&e, &["premium", "a.pro", "on"]), "admin.premium.on");
    let meta = r#"{"channel":"stable","version":"1.0","roothash":"ab","sig":"s"}"#;
    let meta = write(&e, "rel.json", meta);
    let out = run_words(&e, &["publish-release", &meta, &pkg]).unwrap();
    assert_eq!(out.key, "admin.published.release");
    assert_eq!(&out.args[..2], ["1", "7"]);
}

#[test]
fn usage_errors() {
    let e = env();
    for words in [
        &[][..],
        &["grant", "anna", "app"],
        &["grant", "anna", "thing", "x"],
        &["channel", "lts", "maybe"],
        &["publisher", "KM", "yes"],
        &["premium", "a", "true"],
        &["frobnicate"],
    ] {
        let err = run_words(&e, words).unwrap_err();
        assert!(matches!(err, CliError::Usage), "{words:?}");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.message_key(), "admin.usage");
    }
}

#[test]
fn failures_carry_message_keys_that_exist_in_all_languages() {
    let e = env();
    let pkg = write(&e, "p.uap", "x");
    let broken = write(&e, "broken.json", "{not json");
    let invalid = write(
        &e,
        "invalid.json",
        r#"{"id":"../x","name":"N","version":"1"}"#,
    );
    let errors = [
        run_words(&e, &["grant", "nobody", "app", "x"]).unwrap_err(),
        run_words(&e, &["grant", "anna", "app", "missing"]).unwrap_err(),
        run_words(&e, &["publish-app", "/nonexistent/meta.json", &pkg]).unwrap_err(),
        run_words(&e, &["publish-app", &broken, &pkg]).unwrap_err(),
        run_words(&e, &["publish-app", &invalid, &pkg]).unwrap_err(),
        run_words(&e, &["publish-release", &broken, &pkg]).unwrap_err(),
    ];
    let keys: Vec<_> = errors.iter().map(CliError::message_key).collect();
    assert_eq!(
        keys,
        vec![
            "admin.error.unknown_account",
            "admin.error.unknown_item",
            "cli.error.read_meta",
            "cli.error.parse_meta",
            "admin.error.invalid_field",
            "cli.error.parse_meta",
        ]
    );
    for err in &errors {
        assert_eq!(err.exit_code(), 1);
        for lang in [Lang::Ar, Lang::En, Lang::De] {
            assert!(msg::text(lang, err.message_key()).is_some());
        }
        assert!(err.to_string().contains(err.message_key()));
    }
    assert_eq!(errors[4].detail(), "id");
}

#[test]
fn every_outcome_key_has_texts() {
    for key in [
        "admin.granted",
        "admin.already_granted",
        "admin.revoked",
        "admin.not_granted",
        "admin.entitlements.header",
        "admin.entitlements.none",
        "admin.channel.open",
        "admin.channel.gated",
        "admin.publisher.verified",
        "admin.publisher.unverified",
        "admin.premium.on",
        "admin.premium.off",
        "admin.published.app",
        "admin.published.release",
    ] {
        assert!(msg::text(Lang::Ar, key).is_some(), "{key}");
    }
}
