//! Logic tests (APP-RULES §11): catalogue parsing, search, state transitions.
//! These run the real signed catalogue asset and the model without a screen.

use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::catalog::Catalog;
use ultar_store::error::{BackendError, SourceError, StoreError};
use ultar_store::search::MatchKind;
use ultar_store::state::{Area, OpState, Surface};
use ultar_store::{search, source::LoadedCatalog, update, Store};

fn loaded() -> LoadedCatalog {
    let catalog = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    LoadedCatalog {
        catalog,
        verified: true,
        signer: Some("test".into()),
        dir: Some("/stick".into()),
    }
}

#[test]
fn built_in_catalog_parses_with_the_twelve_components() {
    let cat = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    assert_eq!(cat.format_version, ultar_store::catalog::FORMAT_VERSION);
    assert_eq!(cat.apps.len(), 12);
    let ids: Vec<&str> = cat.apps.iter().map(|a| a.id.as_str()).collect();
    for want in [
        "km-office",
        "coder",
        "media",
        "communication",
        "mesh",
        "assistant",
        "linux-programs",
        "firefox",
        "windows-programs",
        "android-apps",
        "containers-server",
        "remote-access",
    ] {
        assert!(ids.contains(&want), "missing {want}");
    }
    // Every entry carries the board's icon name and a verified-style publisher.
    for a in &cat.apps {
        assert!(!a.icon.is_empty());
        assert_eq!(a.publisher, "KM Code Craft");
    }
    // Firefox needs the Linux programs component (board note).
    assert_eq!(
        cat.app("firefox").unwrap().requires,
        vec!["linux-programs".to_string()]
    );
    // The app page of Office tools draws exactly three permission rows with
    // their plain-word reasons (board CompactStoreApp); this guards the
    // TOML key names of the asset against the serde field names.
    let office = cat.app("km-office").unwrap();
    assert_eq!(office.permissions.len(), 3, "permissions must parse");
    assert_eq!(office.permissions[0].id, "files");
    assert!(office.permissions[0].reason.ar.contains("يفتح فقط الملفات"));
    // The offline stick carries a package file per component.
    assert!(cat.apps.iter().all(|a| a.package.is_some()));
    // And file types for the search (.xlsx finds the office component).
    assert!(office.file_types.iter().any(|f| f == ".xlsx"));
}

#[test]
fn catalog_rejects_bad_input() {
    assert!(Catalog::parse("format_version = 999\n").is_err());
    // An id that is not a safe directory name.
    assert_eq!(
        Catalog::parse(
            "format_version = 1\n\n[[app]]\nid = \"a/../b\"\nname={ar=\"x\",en=\"y\"}\n"
        )
        .unwrap_err(),
        SourceError::Damaged
    );
}

#[test]
fn search_file_type_finds_the_component() {
    let cat = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    let hits = search::search(&cat, ".xlsx");
    assert!(!hits.is_empty());
    assert!(hits.iter().any(|h| h.app.id == "km-office"));
    // ".deb" finds Linux programs.
    assert!(search::search(&cat, ".deb")
        .iter()
        .any(|h| h.app.id == "linux-programs"));
}

#[test]
fn search_is_arabic_tolerant() {
    let cat = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    // "متجر" style Arabic name search; and diacritic-tolerant: strip harakat.
    let hits = search::search(&cat, "مكتبية");
    assert!(hits.iter().any(|h| h.app.id == "km-office"));
    // With a shadda/damma the folded form still matches.
    let hits2 = search::search(&cat, "مُكْتَبِيَّة");
    assert!(hits2.iter().any(|h| h.app.id == "km-office"));
}

#[test]
fn search_ranks_name_start_first() {
    let cat = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    let hits = search::search(&cat, "coder");
    assert_eq!(hits.first().map(|h| h.app.id.as_str()), Some("coder"));
    assert_eq!(hits.first().unwrap().kind, MatchKind::NameStart);
}

#[test]
fn loading_to_ready_to_installed_flow() {
    let mut s = Store::new();
    assert_eq!(s.surface, Surface::Loading);
    let cmds = s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    assert_eq!(s.surface, Surface::Ready);
    assert!(s.catalog.verified);
    assert_eq!(cmds, vec![Command::ReloadInstalled]);
    let cmds = s.event(StoreEvent::Installed(vec![
        ultar_store::state::InstalledApp {
            id: "km-office".into(),
            version: "1.0.0".into(),
            size: Some(1000),
            last_used_ms: None,
        },
    ]));
    assert!(cmds.is_empty());
    assert!(s.is_installed("km-office"));
    // The built-in catalogue carries no versions (the board: `—` = run time),
    // so no update can be claimed from it — never a fake "update available".
    assert!(!s.update_available("km-office"));
}

#[test]
fn version_update_detection_when_catalogue_carries_versions() {
    let mut s = Store::new();
    let mut cat = Catalog::parse(ultar_store::BUILT_IN_CATALOG).unwrap();
    for a in &mut cat.apps {
        if a.id == "km-office" {
            a.version = Some("2.0.0".into());
        }
    }
    s.catalog.data = Some(cat);
    s.catalog.dir = Some("/stick".into());
    s.surface = Surface::Ready;
    assert!(!s.update_available("km-office"));
    s.installed.push(ultar_store::state::InstalledApp {
        id: "km-office".into(),
        version: "1.4.0".into(),
        size: None,
        last_used_ms: None,
    });
    assert!(s.update_available("km-office"));
    s.installed[0].version = "2.0.0".into();
    assert!(!s.update_available("km-office"));
}

#[test]
fn offline_and_error_are_distinguished() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Err(StoreError::Source(
        SourceError::Unreadable,
    ))));
    assert_eq!(s.surface, Surface::Offline);
    let mut s2 = Store::new();
    s2.event(StoreEvent::CatalogLoaded(Err(StoreError::Source(
        SourceError::Damaged,
    ))));
    match &s2.surface {
        Surface::Error { key } => assert_eq!(key, "store.error.damaged"),
        other => panic!("expected error, got {other:?}"),
    }
}

#[test]
fn install_progress_open_remove_happy_path() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    // Install Media (id has a package file in the catalogue).
    let cmds = s.act(StoreAction::Install("media".into()));
    assert!(
        matches!(&cmds[0], Command::Install { app, path } if app == "media" && path.starts_with("/stick/"))
    );
    s.event(StoreEvent::Progress {
        app: "media".into(),
        percent: Some(0.5),
    });
    assert_eq!(
        s.operation.as_ref().unwrap().state("media"),
        Some(&OpState::Running(Some(0.5)))
    );
    s.event(StoreEvent::OperationDone {
        app: "media".into(),
        result: Ok(()),
    });
    assert!(s.operation.is_none());
    // Now it is installed (the launcher list refresh would add it); open it.
    s.installed.push(ultar_store::state::InstalledApp {
        id: "media".into(),
        version: "1.0.0".into(),
        size: None,
        last_used_ms: None,
    });
    assert_eq!(
        s.act(StoreAction::Launch("media".into())),
        vec![Command::Launch("media".into())]
    );
    // Remove asks first.
    assert_eq!(
        s.act(StoreAction::Remove("media".into())),
        vec![Command::AskRemove("media".into())]
    );
}

#[test]
fn requirement_blocks_install_until_present() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    // Firefox requires linux-programs.
    let cmds = s.act(StoreAction::Install("firefox".into()));
    assert!(
        cmds.is_empty(),
        "must not install while requirement missing"
    );
    s.installed.push(ultar_store::state::InstalledApp {
        id: "linux-programs".into(),
        version: "1".into(),
        size: None,
        last_used_ms: None,
    });
    let cmds = s.act(StoreAction::Install("firefox".into()));
    assert_eq!(cmds.len(), 1);
}

#[test]
fn package_refused_is_reported_not_swallowed() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    s.act(StoreAction::Install("media".into()));
    let cmds = s.event(StoreEvent::OperationDone {
        app: "media".into(),
        result: Err(StoreError::Backend(BackendError::PackageRefused)),
    });
    assert!(cmds.is_empty());
    assert!(matches!(
        s.operation.as_ref().unwrap().state("media"),
        Some(OpState::Failed)
    ));
    // Never reported as success.
    assert!(!s.is_installed("media"));
}

#[test]
fn search_page_and_clear() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    s.act(StoreAction::Search("coder".into()));
    assert_eq!(s.area, Area::Search);
    let hits = update::visible_results(&s);
    assert!(!hits.is_empty());
    s.act(StoreAction::SearchClear);
    assert_eq!(s.area, Area::Discover);
    assert!(s.query.is_empty());
}

#[test]
fn categories_navigation() {
    let mut s = Store::new();
    s.event(StoreEvent::CatalogLoaded(Ok(loaded())));
    s.act(StoreAction::Area(Area::Categories));
    assert_eq!(s.category, None);
    s.act(StoreAction::Category("office".into()));
    assert_eq!(s.area, Area::Categories);
    assert_eq!(s.category.as_deref(), Some("office"));
    let in_cat: Vec<_> = s.apps().iter().filter(|a| a.category == "office").collect();
    assert!(!in_cat.is_empty());
}
