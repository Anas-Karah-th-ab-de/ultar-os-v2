//! State-transition tests over the pure model (`ultar_store::update`):
//! every event, every consequence, no screen, no I/O.

use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::catalog::{Catalog, CatalogApp, Loc, Source};
use ultar_store::error::{BackendError, SourceError, StoreError};
use ultar_store::source::LoadedCatalog;
use ultar_store::state::{Area, InstalledApp, OpKind, OpState, Surface};
use ultar_store::update::visible_results;
use ultar_store::Store;

fn loc(s: &str) -> Loc {
    Loc {
        ar: s.into(),
        en: s.into(),
        de: None,
    }
}

fn entry(id: &str, version: &str, requires: &[&str], package: Option<&str>) -> CatalogApp {
    CatalogApp {
        id: id.into(),
        name: loc(id),
        summary: loc("s"),
        description: loc("d"),
        publisher: "p".into(),
        category: "c".into(),
        icon: "office".into(),
        source: Source::Ultar,
        version: Some(version.into()),
        size_download: None,
        size_installed: None,
        file_types: Vec::new(),
        permissions: Vec::new(),
        requires: requires.iter().map(|s| s.to_string()).collect(),
        pin: ultar_store::catalog::PackagePin {
            package: package.map(|s| s.to_string()),
            sha256: None,
        },
    }
}

fn store_with(ids: &[(&str, &str, &[&str], Option<&str>)]) -> Store {
    let mut s = Store::new();
    s.catalog.data = Some(Catalog {
        format_version: ultar_store::catalog::FORMAT_VERSION,
        generated: None,
        apps: ids
            .iter()
            .map(|(id, v, r, p)| entry(id, v, r, *p))
            .collect(),
    });
    s.catalog.dir = Some("/usb".into());
    s.surface = Surface::Ready;
    s
}

#[test]
fn area_navigation_clears_search_and_opens_lists() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    s.act(StoreAction::Search("x".into()));
    assert_eq!(s.area, Area::Search);
    let cmds = s.act(StoreAction::Area(Area::Installed));
    assert_eq!(s.area, Area::Installed);
    assert!(s.query.is_empty());
    assert_eq!(cmds, vec![Command::ReloadInstalled]);
    let cmds = s.act(StoreAction::Area(Area::OfflineSources));
    assert_eq!(cmds, vec![Command::DiscoverSources]);
}

#[test]
fn open_back_returns_to_the_same_list() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    s.act(StoreAction::Search("office".into()));
    s.act(StoreAction::Open("a".into()));
    assert_eq!(s.area, Area::AppPage);
    s.act(StoreAction::Back);
    assert_eq!(s.area, Area::Search);
    assert_eq!(s.query, "office");
    assert_eq!(s.open_app, None);
}

#[test]
fn install_needs_a_package_file_or_reports_missing() {
    // A standalone store where only b (no package file) is present.
    let mut s = store_with(&[("b", "1.0", &[], None)]);
    let cmds = s.act(StoreAction::Install("b".into()));
    assert_eq!(cmds, vec![Command::PackageMissing("b".into())]);
    assert!(s.operation.is_none());
    // With a package file it starts and the operation is the install.
    let mut ok = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
    let cmds = ok.act(StoreAction::Install("a".into()));
    assert_eq!(
        cmds,
        vec![Command::Install {
            app: "a".into(),
            path: "/usb/a.uap".into()
        }]
    );
    assert!(ok.operation.is_some());
}

#[test]
fn install_waits_when_a_requirement_is_missing() {
    let mut s = store_with(&[("ff", "1.0", &["linux"], Some("ff.uap"))]);
    let cmds = s.act(StoreAction::Install("ff".into()));
    assert!(cmds.is_empty());
    assert!(s.operation.is_none());
    s.installed.push(InstalledApp {
        id: "linux".into(),
        version: "0.1".into(),
        size: None,
        last_used_ms: None,
    });
    let cmds = s.act(StoreAction::Install("ff".into()));
    assert_eq!(cmds.len(), 1);
}

#[test]
fn no_space_shows_the_free_list_and_go_continues() {
    let mut s = store_with(&[("media", "1.0", &[], Some("media.uap"))]);
    s.act(StoreAction::Install("media".into()));
    let cmds = s.event(StoreEvent::OperationDone {
        app: "media".into(),
        result: Err(StoreError::Backend(BackendError::NoSpace {
            required: Some(900),
            available: Some(100),
        })),
    });
    assert_eq!(
        cmds,
        vec![Command::FreeItems {
            for_app: Some("media".into())
        }]
    );
    assert_eq!(s.free.required, Some(900));
    assert_eq!(s.free.entries.len(), 3);
    assert!(
        s.free.entries[0].selected && s.free.entries[1].selected && !s.free.entries[2].selected
    );
    assert_eq!(s.free.selected_total(), 0); // sizes unknown on the board

    s.act(StoreAction::FreeItem("old-updates".into()));
    assert!(s.free.entries[2].selected);
    let cmds = s.act(StoreAction::FreeGo);
    assert_eq!(
        cmds,
        vec![Command::Free {
            ids: vec!["trash".into(), "temp".into(), "old-updates".into()],
            resume: Some("media".into())
        }]
    );
    let cmds = s.event(StoreEvent::Freed {
        freed: 1000,
        resumed: Some("media".into()),
    });
    assert!(cmds.contains(&Command::Install {
        app: "media".into(),
        path: "/usb/media.uap".into()
    }));
}

#[test]
fn free_cancel_frees_nothing() {
    let mut s = store_with(&[("media", "1.0", &[], Some("m.uap"))]);
    s.act(StoreAction::Install("media".into()));
    let _ = s.event(StoreEvent::OperationDone {
        app: "media".into(),
        result: Err(StoreError::Backend(BackendError::NoSpace {
            required: None,
            available: None,
        })),
    });
    let cmds = s.act(StoreAction::FreeCancel);
    assert!(s.operation.is_none());
    assert!(s.free.entries.is_empty());
    assert!(cmds.iter().any(|c| matches!(c, Command::CancelOp { .. })));
}

#[test]
fn cancel_stops_a_running_operation() {
    let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
    s.act(StoreAction::Install("a".into()));
    let cmds = s.act(StoreAction::Cancel);
    assert_eq!(
        cmds,
        vec![Command::CancelOp {
            kind: OpKind::Install
        }]
    );
    assert!(s.operation.is_none());
}

#[test]
fn retry_logs_and_offline_paths() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    assert_eq!(s.act(StoreAction::Retry), vec![Command::ReloadCatalog]);
    s.last_error = Some(StoreError::from(SourceError::Damaged));
    let cmds = s.act(StoreAction::Log);
    assert_eq!(cmds.len(), 1);
    if let Command::SaveLog { report } = &cmds[0] {
        assert!(report.contains("Damaged"));
    } else {
        panic!("expected SaveLog");
    }
    let cmds = s.act(StoreAction::Offline);
    assert_eq!(s.area, Area::OfflineSources);
    assert_eq!(cmds, vec![Command::DiscoverSources]);
    s.act(StoreAction::UseSource("/media/USB2".into()));
}

#[test]
fn offline_and_error_surfaces_still_show_installed() {
    let mut s = Store::new();
    let cmds = s.event(StoreEvent::CatalogLoaded(Err(StoreError::from(
        SourceError::Unreadable,
    ))));
    assert_eq!(s.surface, Surface::Offline);
    assert_eq!(cmds, vec![Command::ReloadInstalled]);
    let cmds = s.event(StoreEvent::CatalogLoaded(Err(StoreError::from(
        SourceError::Damaged,
    ))));
    assert!(matches!(s.surface, Surface::Error { .. }));
    assert_eq!(cmds, vec![Command::ReloadInstalled]);
}

#[test]
fn catalogue_ready_then_installed_refresh() {
    let mut s = Store::new();
    let loaded = LoadedCatalog {
        catalog: Catalog {
            format_version: ultar_store::catalog::FORMAT_VERSION,
            generated: None,
            apps: Vec::new(),
        },
        verified: true,
        signer: Some("k".into()),
        dir: Some("/usb".into()),
    };
    let cmds = s.event(StoreEvent::CatalogLoaded(Ok(loaded)));
    assert_eq!(s.surface, Surface::Ready);
    assert!(s.catalog.verified);
    assert_eq!(s.catalog.dir.as_deref(), Some("/usb"));
    assert_eq!(cmds, vec![Command::ReloadInstalled]);
    let cmds = s.event(StoreEvent::Installed(vec![InstalledApp {
        id: "a".into(),
        version: "1".into(),
        size: None,
        last_used_ms: None,
    }]));
    assert!(cmds.is_empty());
    assert!(s.is_installed("a"));
}

#[test]
fn progress_and_done_update_the_operation() {
    let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
    s.act(StoreAction::Install("a".into()));
    s.event(StoreEvent::Progress {
        app: "a".into(),
        percent: Some(0.5),
    });
    assert_eq!(
        s.operation.as_ref().unwrap().state("a"),
        Some(&OpState::Running(Some(0.5)))
    );
    let cmds = s.event(StoreEvent::OperationDone {
        app: "a".into(),
        result: Ok(()),
    });
    assert!(cmds.contains(&Command::ReloadInstalled));
    assert!(s.operation.is_none());
}

#[test]
fn failed_operation_keeps_reason() {
    let mut s = store_with(&[("a", "1.0", &[], Some("a.uap"))]);
    s.act(StoreAction::Install("a".into()));
    s.event(StoreEvent::OperationDone {
        app: "a".into(),
        result: Err(StoreError::from(BackendError::PackageRefused)),
    });
    assert_eq!(
        s.operation.as_ref().unwrap().state("a"),
        Some(&OpState::Failed)
    );
    assert_eq!(
        s.last_error.as_ref().unwrap().message_key(),
        "store.error.package-refused"
    );
}

#[test]
fn search_lives_and_clears() {
    let mut s = store_with(&[("office", "1.0", &[], None)]);
    s.act(StoreAction::Search("no-such-thing".into()));
    assert_eq!(s.area, Area::Search);
    assert!(visible_results(&s).is_empty());
    s.act(StoreAction::SearchClear);
    assert_eq!(s.area, Area::Discover);
    assert!(s.query.is_empty());
    assert!(visible_results(&s).is_empty());
}
