//! State-transition tests over the pure model (`ultar_store::update`):
//! every event, every consequence, no screen, no I/O.

use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::catalog::{Catalog, CatalogApp, Loc, Source};
use ultar_store::state::{Area, FreeEntry, InstalledApp, OpKind, Surface};
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
        package: package.map(|s| s.to_string()),
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
fn remove_flow_confirms_then_removes() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    let cmds = s.act(StoreAction::Remove("a".into()));
    assert_eq!(cmds, vec![Command::AskRemove("a".into())]);
    let cmds = s.act(StoreAction::RemoveConfirmed {
        app: "a".into(),
        keep_data: true,
    });
    assert_eq!(
        cmds,
        vec![Command::Uninstall {
            app: "a".into(),
            keep_data: true
        }]
    );
    let cmds = s.event(StoreEvent::Removed("a".into()));
    assert!(cmds.is_empty());
    assert!(!s.is_installed("a"));
    let _ = s.act(StoreAction::RemoveCancelled);
}

#[test]
fn free_items_event_keeps_selection_and_sets_numbers() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    s.free.entries = vec![FreeEntry {
        id: "temp".into(),
        size: None,
        selected: false,
    }];
    let cmds = s.event(StoreEvent::FreeItems(
        vec![
            FreeEntry {
                id: "trash".into(),
                size: Some(5),
                selected: true,
            },
            FreeEntry {
                id: "temp".into(),
                size: Some(7),
                selected: true,
            },
        ],
        Some(1),
        Some(2),
    ));
    assert!(cmds.is_empty());
    assert_eq!(s.free.required, Some(1));
    assert_eq!(s.free.available, Some(2));
    let temp = s.free.entries.iter().find(|f| f.id == "temp").unwrap();
    assert!(!temp.selected, "user selection must survive the refresh");
    assert_eq!(temp.size, Some(7));
    let trash = s.free.entries.iter().find(|f| f.id == "trash").unwrap();
    assert!(trash.selected);
    s.act(StoreAction::FreeItem("temp".into()));
    assert_eq!(s.free.selected_total(), 12);
}

#[test]
fn category_page_from_chip() {
    let mut s = store_with(&[("a", "1.0", &[], None)]);
    s.category = None;
    let cmds = s.act(StoreAction::Category("office".into()));
    assert!(cmds.is_empty());
    assert_eq!(s.area, Area::Categories);
    assert_eq!(s.category.as_deref(), Some("office"));
}

#[test]
fn install_file_takes_the_path_directly() {
    let mut s = Store::new();
    let cmds = s.act(StoreAction::InstallFile("/usb/tool.uap".into()));
    assert_eq!(
        cmds,
        vec![Command::Install {
            app: "/usb/tool.uap".into(),
            path: "/usb/tool.uap".into()
        }]
    );
    assert!(s.operation.is_some());
}

#[test]
fn update_all_chains_the_updates() {
    let mut s = store_with(&[
        ("a", "2.0", &[], Some("a.uap")),
        ("b", "2.0", &[], Some("b.uap")),
    ]);
    s.installed = vec![
        InstalledApp {
            id: "a".into(),
            version: "1.0".into(),
            size: None,
            last_used_ms: None,
        },
        InstalledApp {
            id: "b".into(),
            version: "1.0".into(),
            size: None,
            last_used_ms: None,
        },
    ];
    let cmds = s.act(StoreAction::UpdateAll);
    assert_eq!(cmds.len(), 1);
    assert_eq!(s.operation.as_ref().unwrap().kind, OpKind::Update);
    // First done → the chain continues with the second.
    let cmds = s.event(StoreEvent::OperationDone {
        app: "a".into(),
        result: Ok(()),
    });
    assert!(matches!(cmds[0], Command::Install { .. }));
    let _ = s.event(StoreEvent::OperationDone {
        app: "b".into(),
        result: Ok(()),
    });
    assert!(s.update_queue.is_empty());
}
