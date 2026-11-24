//! Flow tests (APP-RULES §11): complete runs through the real event loop —
//! every `Command` is executed by a backend and its `StoreEvent` answers go
//! back into the model, exactly like the shell does. Nothing is asserted on
//! a half-step: a success is only visible when the backend said success.

mod common;

use common::Scene;
use ultar_store::action::{Command, StoreAction, StoreEvent};
use ultar_store::backend::{Backend, ScriptedBackend};
use ultar_store::error::{BackendError, SourceError, StoreError};
use ultar_store::state::{Area, OpState, Surface};
use ultar_ui::Lang;

/// One loop step: run every command through the backend and feed all answer
/// events back; returns all events seen (for assertions).
fn pump(scene: &mut Scene, backend: &ScriptedBackend, cmds: Vec<Command>) -> Vec<StoreEvent> {
    let mut events = Vec::new();
    for cmd in cmds {
        backend.run(cmd, &mut |ev| events.push(ev));
    }
    for ev in events.clone() {
        let more = scene.store.event(ev);
        if !more.is_empty() {
            events.extend(pump(scene, backend, more));
        }
    }
    scene.rebuild();
    events
}

#[test]
fn boot_to_discover_over_a_working_launcher() {
    let be = ScriptedBackend::with_catalog(Ok(common::loaded_from_asset()));
    be.set_installed(
        ["km-office", "coder"]
            .iter()
            .map(|id| ultar_store::state::InstalledApp {
                id: (*id).to_string(),
                version: "1.0.0".into(),
                size: None,
                last_used_ms: None,
            })
            .collect(),
    );
    let mut s = Scene::new(
        ultar_store::Store::new(),
        Lang::Ar,
        common::dark(),
        common::WIDE,
    );
    // Boot: the shell asks for the catalogue.
    let mut events = Vec::new();
    be.run(Command::ReloadCatalog, &mut |ev| events.push(ev));
    for ev in events {
        let cmds = s.store.event(ev);
        assert!(
            !cmds.is_empty(),
            "catalog answer must lead to the installed query"
        );
        let mut more = Vec::new();
        for c in cmds {
            be.run(c, &mut |e| more.push(e));
        }
        for e in more {
            assert!(s.store.event(e).is_empty());
        }
    }
    s.rebuild();
    assert_eq!(s.store.surface, Surface::Ready);
    assert!(s.store.is_installed("km-office"));
    // The offline list shows both installed names.
    assert!(s.a11y_texts().iter().any(|t| t.contains("الأدوات المكتبية")));
}

#[test]
fn boot_without_any_catalogue_shows_the_offline_surface() {
    let be = ScriptedBackend::empty();
    let mut s = Scene::new(
        ultar_store::Store::new(),
        Lang::Ar,
        common::dark(),
        common::WIDE,
    );
    let mut events = Vec::new();
    be.run(Command::ReloadCatalog, &mut |ev| events.push(ev));
    for ev in events {
        s.store.event(ev);
    }
    s.rebuild();
    assert_eq!(s.store.surface, Surface::Offline);
    // The offline surface still offers installed apps and offline sources.
    assert!(s
        .a11y_texts()
        .iter()
        .any(|t| t.contains("غير متصل بالإنترنت")));
}

#[test]
fn full_install_flow_updates_the_page_with_no_faked_success() {
    let be = ScriptedBackend::with_catalog(Ok(common::loaded_from_asset()));
    // The launcher's list after the success: media joined the installed ones.
    be.set_installed(
        ["km-office", "coder", "media"]
            .iter()
            .map(|id| ultar_store::state::InstalledApp {
                id: (*id).to_string(),
                version: "1.0.0".into(),
                size: None,
                last_used_ms: None,
            })
            .collect(),
    );
    let mut s = Scene::new(
        common::ready_store(),
        Lang::Ar,
        common::dark(),
        common::COMPACT,
    );
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    let cmds = s.click(&StoreAction::Install("media".into()));
    let seen = pump(&mut s, &be, cmds);
    assert!(matches!(
        seen.iter()
            .find(|e| matches!(e, StoreEvent::OperationDone { .. })),
        Some(StoreEvent::OperationDone { result: Ok(()), .. })
    ));
    // Success only after the scripted launcher answered Ok; now Open+Remove.
    assert!(s.store.operation.is_none());
    assert!(s
        .hits
        .node_for(&StoreAction::Launch("media".into()))
        .is_some());
    assert!(s
        .hits
        .node_for(&StoreAction::Remove("media".into()))
        .is_some());
}

#[test]
fn refused_package_stays_not_installed_and_shows_the_reason() {
    let be = ScriptedBackend::empty();
    be.installs(
        "media",
        Err(StoreError::Backend(BackendError::PackageRefused)),
    );
    let mut s = Scene::new(
        common::ready_store(),
        Lang::Ar,
        common::dark(),
        common::COMPACT,
    );
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    let cmds = s.click(&StoreAction::Install("media".into()));
    pump(&mut s, &be, cmds);
    assert!(!s.store.is_installed("media"));
    assert_eq!(
        s.store.operation.as_ref().and_then(|o| o.state("media")),
        Some(&OpState::Failed)
    );
    // The reason is on screen (never silent).
    assert!(s.a11y_texts().iter().any(|t| t.contains("رفضت الحزمة")));
}

#[test]
fn no_space_then_free_then_continue_end_to_end() {
    let be = ScriptedBackend::with_catalog(Ok(common::loaded_from_asset()));
    be.installs(
        "media",
        Err(StoreError::Backend(BackendError::NoSpace {
            required: None,
            available: None,
        })),
    );
    be.set_free(
        vec![
            ultar_store::state::FreeEntry {
                id: "trash".into(),
                size: Some(30_000_000),
                selected: true,
            },
            ultar_store::state::FreeEntry {
                id: "temp".into(),
                size: Some(10_000_000),
                selected: true,
            },
        ],
        None,
        None,
    );
    be.set_freed(40_000_000, true);
    let mut s = Scene::new(
        common::ready_store(),
        Lang::Ar,
        common::dark(),
        common::COMPACT,
    );
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    let cmds = s.click(&StoreAction::Install("media".into()));
    pump(&mut s, &be, cmds);
    // The board's panel: with the launcher's numbers the selected total shows.
    assert_eq!(s.store.free.entries.len(), 2);
    assert!(s.a11y_texts().iter().any(|t| t.contains("ما يمكن تحريره")));
    let cmds = s.click(&StoreAction::FreeGo);
    pump(&mut s, &be, cmds);
    // Freed + resumed: the install command ran again (with the default Ok).
    assert!(be
        .seen()
        .iter()
        .any(|c| matches!(c, Command::Install { app, .. } if app == "media")));
}

#[test]
fn remove_keeps_or_deletes_data_as_chosen() {
    let be = ScriptedBackend::empty();
    let mut s = Scene::new(
        common::ready_store(),
        Lang::Ar,
        common::dark(),
        common::WIDE,
    );
    s.store.act(StoreAction::Open("km-office".into()));
    s.rebuild();
    let cmds = s.click(&StoreAction::Remove("km-office".into()));
    assert_eq!(cmds, vec![Command::AskRemove("km-office".into())]);
    // User confirms with "delete data": the uninstall carries keep_data=false.
    let cmds = s.store.act(StoreAction::RemoveConfirmed {
        app: "km-office".into(),
        keep_data: false,
    });
    let seen = pump(&mut s, &be, cmds);
    assert!(seen.contains(&StoreEvent::Removed("km-office".into())));
    assert!(!s.store.is_installed("km-office"));
}

#[test]
fn search_flow_from_typing_to_app_page_and_back() {
    let be = ScriptedBackend::empty();
    let _ = be;
    let mut s = Scene::ready(Lang::Ar, common::dark(), true);
    s.type_search("مبرمج"); // the Arabic name of Coder, without diacritics
    let hits = ultar_store::update::visible_results(&s.store);
    assert!(hits.iter().any(|h| h.app.id == "coder"), "{hits:?}");
    let cmds = s.click(&StoreAction::Open("coder".into()));
    assert!(cmds.is_empty());
    assert_eq!(s.store.area, Area::AppPage);
    s.rebuild();
    s.click(&StoreAction::Back);
    assert_eq!(s.store.area, Area::Search);
    assert!(!s.store.query.is_empty());
}

#[test]
fn log_flow_saves_and_reports_the_path() {
    let mut s = Scene::new(
        {
            let mut st = ultar_store::Store::new();
            st.event(StoreEvent::CatalogLoaded(Err(StoreError::Source(
                SourceError::BadSignature,
            ))));
            st
        },
        Lang::Ar,
        common::dark(),
        common::WIDE,
    );
    s.render();
    let cmds = s.click(&StoreAction::Log);
    assert_eq!(cmds.len(), 1);
    let be = ScriptedBackend::empty();
    let seen = pump(&mut s, &be, cmds);
    assert!(matches!(
        seen.iter().find(|e| matches!(e, StoreEvent::LogSaved { .. })),
        Some(StoreEvent::LogSaved { path }) if path.contains("store-error-report")
    ));
    assert!(s.store.log_saved.is_some());
}
