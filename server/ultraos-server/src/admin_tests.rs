//! Unit tests for administrative operations and the catalogue queries that read their results.

use super::*;
use crate::catalog::{self, CatalogQuery};
use std::path::PathBuf;

type Edit<T> = Box<dyn Fn(&mut T)>;

struct Fixture {
    db: Db,
    files: FileStore,
    dir: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(":memory:").unwrap();
        db.lock()
            .execute(
                "INSERT INTO accounts(id, username, pw_hash, created) VALUES(1,'anna','h',0)",
                [],
            )
            .unwrap();
        let files = FileStore::new(dir.path().join("store"));
        Self { db, files, dir }
    }

    fn file(&self, name: &str, content: &[u8]) -> PathBuf {
        let path = self.dir.path().join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn publish(&self, id: &str, name: &str, version: &str, edit: impl FnOnce(&mut AppPublication)) {
        let mut p = app(id, name, version);
        edit(&mut p);
        let file = self.file("pkg.uap", format!("{id}-{version}").as_bytes());
        publish_app(&self.db, &self.files, &p, &file).unwrap();
    }

    fn search(&self, text: &str, category: &str, file_type: &str) -> Vec<String> {
        let q = CatalogQuery {
            text: text.into(),
            category: category.into(),
            file_type: file_type.into(),
            page: 1,
            per_page: 50,
        };
        catalog::list(&self.db.lock(), &q)
            .unwrap()
            .items
            .into_iter()
            .map(|a| a.id)
            .collect()
    }
}

fn app(id: &str, name: &str, version: &str) -> AppPublication {
    serde_json::from_value(serde_json::json!({"id": id, "name": name, "version": version})).unwrap()
}

fn release(channel: &str, version: &str) -> ReleasePublication {
    serde_json::from_value(serde_json::json!({
        "channel": channel, "version": version, "roothash": "ab12", "sig": "c2ln"
    }))
    .unwrap()
}

#[test]
fn publish_app_records_everything_and_details_read_it_back() {
    let f = Fixture::new();
    set_publisher(&f.db, "KM Code Craft", true).unwrap();
    f.publish("com.kmcc.sheets", "KM Sheets", "1.0.0", |p| {
        p.description = "Spreadsheets".into();
        p.category = "office".into();
        p.publisher = "KM Code Craft".into();
        p.permissions = vec!["network".into(), "files".into()];
        p.file_types = vec![".XLSX".into(), "csv".into(), "xlsx".into()];
        p.installed_size = 4096;
        p.changelog = "first".into();
    });
    f.publish("com.kmcc.sheets", "KM Sheets", "1.1.0", |p| {
        p.category = "office".into();
        p.publisher = "KM Code Craft".into();
        p.changelog = "second".into();
        p.file_types = vec!["xlsx".into()];
    });
    let d = catalog::details(&f.db.lock(), "com.kmcc.sheets")
        .unwrap()
        .unwrap();
    assert_eq!(d.summary.version, "1.1.0");
    assert_eq!(
        d.summary.publisher,
        catalog::Publisher {
            name: "KM Code Craft".into(),
            verified: true
        }
    );
    assert!(!d.summary.premium);
    assert_eq!(
        d.file_types,
        vec!["xlsx"],
        "file types follow the newest publication"
    );
    assert_eq!(d.download, "/v1/store/apps/com.kmcc.sheets/download");
    let history: Vec<_> = d
        .versions
        .iter()
        .map(|v| (v.version.as_str(), v.changelog.as_str()))
        .collect();
    assert_eq!(history, vec![("1.1.0", "second"), ("1.0.0", "first")]);
    assert_eq!(d.summary.size, "com.kmcc.sheets-1.1.0".len() as i64);
    assert!(catalog::details(&f.db.lock(), "nope").unwrap().is_none());
}

#[test]
fn published_versions_are_immutable() {
    let f = Fixture::new();
    f.publish("a.one", "One", "1.0", |_| {});
    let file = f.file("other.uap", b"different content");
    let err = publish_app(&f.db, &f.files, &app("a.one", "One", "1.0"), &file).unwrap_err();
    assert!(matches!(err, AdminError::AlreadyPublished));
    assert_eq!(err.message_key(), "admin.error.already_published");
}

#[test]
fn publish_app_validates_every_field() {
    let f = Fixture::new();
    let file = f.file("p.uap", b"x");
    let cases: Vec<(&str, Edit<AppPublication>)> = vec![
        ("id", Box::new(|p| p.id = "../evil".into())),
        ("version", Box::new(|p| p.version = "1/2".into())),
        ("category", Box::new(|p| p.category = "has space".into())),
        ("name", Box::new(|p| p.name = "  ".into())),
        (
            "description",
            Box::new(|p| p.description = "x".repeat(4001)),
        ),
        ("publisher", Box::new(|p| p.publisher = "x".repeat(101))),
        ("changelog", Box::new(|p| p.changelog = "x".repeat(4001))),
        ("sig", Box::new(|p| p.sig = "x".repeat(201))),
        (
            "permissions",
            Box::new(|p| p.permissions = vec!["bad perm".into()]),
        ),
        (
            "file_types",
            Box::new(|p| p.file_types = vec!["a/b".into()]),
        ),
        ("installed_size", Box::new(|p| p.installed_size = u64::MAX)),
    ];
    for (field, edit) in cases {
        let mut p = app("a.ok", "Ok", "1.0");
        edit(&mut p);
        match publish_app(&f.db, &f.files, &p, &file) {
            Err(AdminError::InvalidField(got)) => assert_eq!(got, field),
            other => panic!("{field}: {other:?}"),
        }
    }
    assert!(f.search("", "", "").is_empty(), "nothing was published");
    let missing = f.dir.path().join("absent.uap");
    let err = publish_app(&f.db, &f.files, &app("a.ok", "Ok", "1.0"), &missing).unwrap_err();
    assert!(matches!(err, AdminError::File(_)));
    assert!(std::error::Error::source(&err).is_some());
}

#[test]
fn unknown_fields_in_a_publication_are_rejected() {
    let bad = serde_json::json!({"id":"a","name":"A","version":"1","price":"9.99"});
    assert!(serde_json::from_value::<AppPublication>(bad).is_err());
    let bad = serde_json::json!({"channel":"stable","version":"1","roothash":"ab","sig":"s","x":1});
    assert!(serde_json::from_value::<ReleasePublication>(bad).is_err());
}

#[test]
fn search_by_name_description_file_type_and_category() {
    let f = Fixture::new();
    f.publish("a.sheets", "KM Sheets", "1", |p| {
        p.description = "Tables and 100% charts".into();
        p.category = "office".into();
        p.file_types = vec!["xlsx".into(), "csv".into()];
    });
    f.publish("a.notes", "مُفَكِّرَة أحمد", "1", |p| {
        p.description = "ملاحظات سريعة".into();
        p.category = "office".into();
        p.file_types = vec!["txt".into()];
    });
    f.publish("a.game", "Snake_Game", "1", |p| p.category = "games".into());
    assert_eq!(
        f.search("", "", ""),
        vec!["a.sheets", "a.game", "a.notes"],
        "ordered by name"
    );
    assert_eq!(f.search("sheets", "", ""), vec!["a.sheets"]);
    assert_eq!(f.search("SHEETS", "", ""), vec!["a.sheets"]);
    assert_eq!(f.search("charts", "", ""), vec!["a.sheets"], "description");
    assert_eq!(
        f.search(".xlsx", "", ""),
        vec!["a.sheets"],
        "dot query = file type"
    );
    assert_eq!(f.search("csv", "", ""), vec!["a.sheets"], "bare file type");
    assert_eq!(f.search("", "", "txt"), vec!["a.notes"]);
    assert_eq!(f.search("", "games", ""), vec!["a.game"]);
    assert_eq!(f.search("km", "games", ""), Vec::<String>::new());
    assert_eq!(
        f.search("مفكرة احمد", "", ""),
        vec!["a.notes"],
        "Arabic without diacritics/hamza"
    );
    assert_eq!(f.search("ملاحظات", "", ""), vec!["a.notes"]);
    assert_eq!(
        f.search("100%", "", ""),
        vec!["a.sheets"],
        "percent is literal"
    );
    assert_eq!(f.search("%", "", ""), vec!["a.sheets"]);
    assert_eq!(
        f.search("snake_", "", ""),
        vec!["a.game"],
        "underscore is literal"
    );
    assert_eq!(
        f.search("s_a", "", ""),
        Vec::<String>::new(),
        "underscore is not a wildcard"
    );
    assert_eq!(f.search(".exe", "", ""), Vec::<String>::new());
    assert_eq!(f.search("' OR 1=1 --", "", ""), Vec::<String>::new());
}

#[test]
fn paging_and_categories() {
    let f = Fixture::new();
    for i in 0..7 {
        f.publish(&format!("a.app{i}"), &format!("App {i}"), "1", |p| {
            p.category = if i < 5 {
                "office".into()
            } else {
                "games".into()
            };
        });
    }
    let page = |page, per_page| {
        let q = CatalogQuery {
            page,
            per_page,
            ..CatalogQuery::default()
        };
        catalog::list(&f.db.lock(), &q).unwrap()
    };
    let first = page(1, 3);
    assert_eq!(
        (first.total, first.items.len(), first.page, first.per_page),
        (7, 3, 1, 3)
    );
    assert_eq!(first.items[0].id, "a.app0");
    assert_eq!(
        page(3, 3)
            .items
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        vec!["a.app6"]
    );
    assert!(page(4, 3).items.is_empty());
    let cats = catalog::categories(&f.db.lock()).unwrap();
    let pairs: Vec<_> = cats.iter().map(|c| (c.id.as_str(), c.count)).collect();
    assert_eq!(pairs, vec![("games", 2), ("office", 5)]);
}

#[test]
fn entitlement_grant_revoke_and_listing() {
    let f = Fixture::new();
    f.publish("a.pro", "Pro", "1", |p| p.premium = true);
    assert!(matches!(
        grant(&f.db, "nobody", EntitlementKind::App, "a.pro"),
        Err(AdminError::UnknownAccount)
    ));
    assert!(matches!(
        grant(&f.db, "anna", EntitlementKind::App, "a.none"),
        Err(AdminError::UnknownItem)
    ));
    assert!(matches!(
        grant(&f.db, "anna", EntitlementKind::Channel, "nope"),
        Err(AdminError::UnknownItem)
    ));
    assert!(grant(&f.db, "anna", EntitlementKind::App, "a.pro").unwrap());
    assert!(!grant(&f.db, "anna", EntitlementKind::App, "a.pro").unwrap());
    assert!(grant(&f.db, "anna", EntitlementKind::Channel, "lts").unwrap());
    let all = entitlements_of(&f.db, "anna").unwrap();
    assert_eq!(
        all,
        vec![
            (EntitlementKind::App, "a.pro".to_string()),
            (EntitlementKind::Channel, "lts".to_string())
        ]
    );
    assert!(revoke(&f.db, "anna", EntitlementKind::App, "a.pro").unwrap());
    assert!(!revoke(&f.db, "anna", EntitlementKind::App, "a.pro").unwrap());
    assert!(matches!(
        revoke(&f.db, "nobody", EntitlementKind::App, "a.pro"),
        Err(AdminError::UnknownAccount)
    ));
    assert!(matches!(
        entitlements_of(&f.db, "nobody"),
        Err(AdminError::UnknownAccount)
    ));
    let audit: Vec<String> = {
        let conn = f.db.lock();
        let mut s = conn
            .prepare("SELECT action FROM audit WHERE who='admin' ORDER BY id")
            .unwrap();
        let rows = s.query_map([], |r| r.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    };
    assert_eq!(
        audit,
        vec![
            "app.publish",
            "entitlement.grant",
            "entitlement.grant",
            "entitlement.revoke"
        ]
    );
}

#[test]
fn channels_publishers_and_premium_switch() {
    let f = Fixture::new();
    set_channel(&f.db, "nightly", true).unwrap();
    assert_eq!(
        crate::updates::channel_gated(&f.db.lock(), "nightly").unwrap(),
        Some(true)
    );
    set_channel(&f.db, "nightly", false).unwrap();
    assert_eq!(
        crate::updates::channel_gated(&f.db.lock(), "nightly").unwrap(),
        Some(false)
    );
    assert!(matches!(
        set_channel(&f.db, "bad name", true),
        Err(AdminError::InvalidField("channel"))
    ));
    assert!(matches!(
        set_publisher(&f.db, " ", true),
        Err(AdminError::InvalidField("publisher"))
    ));
    set_publisher(&f.db, "P", true).unwrap();
    set_publisher(&f.db, "P", false).unwrap();

    f.publish("a.app", "App", "1", |_| {});
    set_premium(&f.db, "a.app", true).unwrap();
    assert!(
        catalog::details(&f.db.lock(), "a.app")
            .unwrap()
            .unwrap()
            .summary
            .premium
    );
    set_premium(&f.db, "a.app", false).unwrap();
    assert!(matches!(
        set_premium(&f.db, "a.none", true),
        Err(AdminError::UnknownItem)
    ));
    f.db.lock()
        .execute("INSERT INTO store_apps(id,name,version,size,sha256,sig,url) VALUES('old','Old','1',1,'aa','s','https://x/y')", [])
        .unwrap();
    let err = set_premium(&f.db, "old", true).unwrap_err();
    assert!(
        matches!(err, AdminError::PremiumNeedsStoredFile),
        "external files cannot be gated"
    );
    set_premium(&f.db, "old", false).unwrap();
}

#[test]
fn publish_release_validates_and_is_immutable() {
    let f = Fixture::new();
    let image = f.file("os.img", b"image-bytes");
    let (id, stored) = publish_release(&f.db, &f.files, &release("stable", "1.0"), &image).unwrap();
    assert_eq!(id, 1);
    assert_eq!(stored.size, 11);
    let latest = crate::updates::latest(&f.db.lock(), "stable", "any", "any")
        .unwrap()
        .unwrap();
    assert_eq!(
        (latest.version.as_str(), latest.url.as_str()),
        ("1.0", "/v1/update/file/1")
    );
    assert_eq!(latest.sha256, stored.sha256);
    let again = publish_release(&f.db, &f.files, &release("stable", "1.0"), &image);
    assert!(matches!(again, Err(AdminError::AlreadyPublished)));
    let unknown = publish_release(&f.db, &f.files, &release("nope", "1.0"), &image);
    assert!(matches!(unknown, Err(AdminError::UnknownItem)));
    let cases: Vec<(&str, Edit<ReleasePublication>)> = vec![
        ("channel", Box::new(|p| p.channel = "a/b".into())),
        ("version", Box::new(|p| p.version = String::new())),
        ("arch", Box::new(|p| p.arch = "..".into())),
        ("edition", Box::new(|p| p.edition = "x".repeat(33))),
        ("roothash", Box::new(|p| p.roothash = "xyz".into())),
        ("sig", Box::new(|p| p.sig = String::new())),
    ];
    for (field, edit) in cases {
        let mut p = release("stable", "2.0");
        edit(&mut p);
        match publish_release(&f.db, &f.files, &p, &image) {
            Err(AdminError::InvalidField(got)) => assert_eq!(got, field),
            other => panic!("{field}: {other:?}"),
        }
    }
}

#[test]
fn error_keys_and_details() {
    let errors = [
        AdminError::UnknownAccount,
        AdminError::UnknownItem,
        AdminError::InvalidField("name"),
        AdminError::AlreadyPublished,
        AdminError::PremiumNeedsStoredFile,
        AdminError::Db(rusqlite::Error::QueryReturnedNoRows),
        AdminError::File(FileError::InvalidName),
    ];
    for e in &errors {
        assert!(
            crate::msg::text(crate::msg::Lang::Ar, e.message_key()).is_some(),
            "{e}"
        );
        assert!(e.to_string().starts_with("admin.error."));
    }
    assert_eq!(AdminError::InvalidField("name").detail(), "name");
    assert_eq!(AdminError::UnknownItem.detail(), "");
}
