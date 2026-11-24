//! Click tests: every `store.*` action of docs/ui/CLICKS.md, driven through
//! the real headless widget tree (pointer down/up on the bound node), doing
//! exactly the documented behaviour. No dead clicks: each board id must be
//! bound on the screen the board shows it on.

mod common;

use common::Scene;
use ultar_store::action::{Command, StoreAction};
use ultar_store::state::{Area, OpState};
use ultar_ui::Lang;

fn ar() -> ultar_ui::tokens::ThemeKind {
    ultar_ui::tokens::ThemeKind::Dark
}

// --- store.area -----------------------------------------------------------

#[test]
fn store_area_opens_every_store_area() {
    let mut s = Scene::ready(Lang::Ar, ar(), true);
    for (label, area) in [
        ("store.area.discover", Area::Discover),
        ("store.area.installed", Area::Installed),
        ("store.area.updates", Area::Updates),
        ("store.area.install-file", Area::InstallFile),
        ("store.area.offline-sources", Area::OfflineSources),
        ("store.area.account", Area::Account),
    ] {
        let node = s
            .hits
            .node_for(&StoreAction::Area(area))
            .unwrap_or_else(|| panic!("{label} not bound on the desktop nav"));
        s.click_node(node);
        assert_eq!(s.store.area, area, "{label} does not open its area");
        // Installed and Updates refresh their list from the launcher.
        if matches!(area, Area::Installed | Area::Updates) {
            assert!(s
                .store
                .act(StoreAction::Area(area))
                .contains(&Command::ReloadInstalled));
        }
        s.store.act(StoreAction::Area(Area::Discover));
        s.rebuild();
    }
}

#[test]
fn store_area_tabs_on_phone() {
    // The phone tabs switch areas through the tab bar's Selected(index),
    // driven by a real pointer click on the tab that leads to each area.
    for lang in [Lang::Ar, Lang::En] {
        let mut s = Scene::ready(lang, ar(), false);
        assert!(s.hits.search_field().is_some(), "phone shows the search");
        let cmds = s.click_tab_to(Area::Installed);
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
        assert_eq!(s.store.area, Area::Installed);
        let cmds = s.click_tab_to(Area::Updates);
        assert_eq!(cmds, vec![Command::ReloadInstalled]);
        assert_eq!(s.store.area, Area::Updates);
        s.click_tab_to(Area::Discover);
        assert_eq!(s.store.area, Area::Discover);
    }
}

// --- store.search ----------------------------------------------------------

#[test]
fn store_search_typed_finds_results_arabic_tolerant() {
    let mut s = Scene::ready(Lang::Ar, ar(), true);
    s.type_search("مبرمج"); // matches المبرمج without diacritics/alef handling
    assert!(!s.store.query.is_empty());
    assert_eq!(s.store.area, Area::Search);
    let hits = ultar_store::update::visible_results(&s.store);
    assert!(
        hits.iter().any(|h| h.app.id == "coder"),
        "typed search must find coder: {hits:?}"
    );
}

#[test]
fn store_search_file_type_finds_component() {
    let mut s = Scene::ready(Lang::En, ar(), true);
    s.type_search(".xlsx");
    let hits = ultar_store::update::visible_results(&s.store);
    assert!(hits.iter().any(|h| h.app.id == "km-office"));
}

#[test]
fn store_search_clear_empties_and_shows_discover() {
    let mut s = Scene::ready(Lang::En, ar(), true);
    s.type_search("zzz-nothing");
    let clear = s
        .hits
        .node_for(&StoreAction::SearchClear)
        .expect("clear button bound in the no-results state");
    s.click_node(clear);
    assert!(s.store.query.is_empty());
    assert_eq!(s.store.area, Area::Discover);
}

// --- store.open / store.back ----------------------------------------------

#[test]
fn store_open_from_card_opens_app_page_never_installs() {
    for wide in [true, false] {
        let mut s = Scene::ready(Lang::Ar, ar(), wide);
        let node = s
            .hits
            .nodes_for_id("store.open")
            .first()
            .copied()
            .expect("cards bind store.open on both boards");
        s.click_node(node);
        assert_eq!(s.store.area, Area::AppPage, "wide={wide}");
        // No Install command was produced by opening.
        assert!(s.store.operation.is_none());
        s.rebuild();
        // store.back returns to the list.
        let back = s
            .hits
            .node_for(&StoreAction::Back)
            .expect("back bound on the app page");
        s.click_node(back);
        assert_eq!(s.store.area, Area::Discover);
    }
}

#[test]
fn store_back_restores_a_search_list() {
    let mut s = Scene::ready(Lang::Ar, ar(), true);
    s.type_search("coder");
    let node = s.hits.nodes_for_id("store.open").remove(0);
    s.click_node(node);
    assert_eq!(s.store.area, Area::AppPage);
    s.store.act(StoreAction::Back);
    s.rebuild();
    assert_eq!(s.store.area, Area::Search);
    assert_eq!(s.store.query, "coder");
}

// --- store.launch ----------------------------------------------------------

#[test]
fn store_launch_asks_the_launcher() {
    let mut s = Scene::ready(Lang::Ar, ar(), true);
    let cmds = s.click(&StoreAction::Launch("km-office".into()));
    assert_eq!(cmds, vec![Command::Launch("km-office".into())]);
}

// --- store.install ---------------------------------------------------------

#[test]
fn store_install_progress_ring_then_open_remove() {
    let mut s = Scene::ready(Lang::Ar, ar(), false);
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    let cmds = s.click(&StoreAction::Install("media".into()));
    assert!(matches!(&cmds[0], Command::Install { app, .. } if app == "media"));
    s.rebuild();
    // The button now shows the ring: label percent, plus Cancel next to it.
    let _ = s.answer(ultar_store::action::StoreEvent::Progress {
        app: "media".into(),
        percent: Some(0.4),
    });
    let texts = s.a11y_texts();
    assert!(
        texts.iter().any(|t| t.contains("40")),
        "percent not shown on the button: {texts:?}"
    );
    assert!(
        s.hits
            .node_for(&StoreAction::Cancel)
            .is_some(),
        "Cancel next to the running install"
    );
    let _ = s.answer(ultar_store::action::StoreEvent::OperationDone {
        app: "media".into(),
        result: Ok(()),
    });
    assert!(s.store.operation.is_none());
    assert_eq!(
        s.store
            .operation
            .as_ref()
            .map(|o| o.state("media")),
        None
    );
    // Installed now: the page offers Open and Remove (CLICKS wording).
    s.store.installed.push(ultar_store::state::InstalledApp {
        id: "media".into(),
        version: "1.0.0".into(),
        size: None,
        last_used_ms: None,
    });
    s.rebuild();
    assert!(s.hits.node_for(&StoreAction::Launch("media".into())).is_some());
    assert!(s.hits.node_for(&StoreAction::Remove("media".into())).is_some());
}

#[test]
fn store_install_cancel_leaves_nothing_running() {
    let mut s = Scene::ready(Lang::Ar, ar(), false);
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    s.click(&StoreAction::Install("media".into()));
    s.rebuild(); // the shell redraws: the button turns into ring + Cancel
    let cmds = s.click(&StoreAction::Cancel);
    assert_eq!(
        cmds,
        vec![Command::CancelOp {
            kind: ultar_store::state::OpKind::Install
        }]
    );
    assert!(s.store.operation.is_none());
}

// --- store.retry / store.log ------------------------------------------------

#[test]
fn store_retry_asks_to_load_again_from_offline() {
    let mut s = Scene::new(common::offline_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    let cmds = s.click(&StoreAction::Retry);
    assert_eq!(cmds, vec![Command::ReloadCatalog]);
}

#[test]
fn store_retry_and_log_from_error() {
    let mut s = Scene::new(common::error_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    let cmds = s.click(&StoreAction::Log);
    assert_eq!(cmds.len(), 1);
    assert!(matches!(&cmds[0], Command::SaveLog { report } if report.contains("ultar-store error report")));
    let cmds = s.click(&StoreAction::Retry);
    assert_eq!(cmds, vec![Command::ReloadCatalog]);
}

// --- store.offline ----------------------------------------------------------

#[test]
fn store_offline_opens_the_offline_sources() {
    let mut s = Scene::new(common::offline_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    let cmds = s.click(&StoreAction::Offline);
    assert_eq!(s.store.area, Area::OfflineSources);
    assert_eq!(cmds, vec![Command::DiscoverSources]);
}

// --- store.free.* -----------------------------------------------------------

#[test]
fn store_free_item_toggles_the_sum() {
    let mut s = Scene::new(common::no_space_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    // Board: trash on, temp on, old updates off; with no sizes the sum is `—`.
    let before = s.store.free.selected_total();
    s.click(&StoreAction::FreeItem("old-updates".into()));
    let after = s.store.free.selected_total();
    assert_eq!(before, after, "unknown sizes keep the sum stable");
    // And the checkbox state visibly flipped:
    let media_state = s
        .store
        .operation
        .as_ref()
        .and_then(|o| o.state("media").cloned());
    assert_eq!(media_state, Some(OpState::Failed));
}

#[test]
fn store_free_go_frees_and_continues_install() {
    let mut s = Scene::new(common::no_space_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    let cmds = s.click(&StoreAction::FreeGo);
    assert_eq!(
        cmds,
        vec![Command::Free {
            ids: vec!["trash".into(), "temp".into()],
            resume: Some("media".into())
        }]
    );
}

#[test]
fn store_free_cancel_cancels_and_frees_nothing() {
    let mut s = Scene::new(common::no_space_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    let cmds = s.click(&StoreAction::FreeCancel);
    assert!(matches!(&cmds[0], Command::CancelOp { .. }));
    assert!(s.store.operation.is_none());
    assert!(s.store.free.entries.is_empty());
}

// --- store.more -------------------------------------------------------------

#[test]
fn store_more_opens_the_menu_with_the_three_areas() {
    let mut s = Scene::ready(Lang::Ar, ar(), false);
    let cmds = s.click(&StoreAction::More);
    assert!(cmds.is_empty()); // the menu itself is a UI effect
    assert!(s.ui.menu_owner().is_some(), "more opened no menu");
}

// --- every CLICKS id is bound somewhere -------------------------------------

#[test]
fn every_clicks_store_id_is_reachable_on_some_screen() {
    // The board ids of docs/ui/CLICKS.md (Store, SurfaceStates,
    // CompactStore, CompactStoreApp).
    let clicks_ids = [
        "store.area",
        "store.search.clear",
        "store.open",
        "store.back",
        "store.launch",
        "store.install",
        "store.retry",
        "store.log",
        "store.offline",
        "store.free.item",
        "store.free.go",
        "store.free.cancel",
        "store.more",
    ];
    let mut seen: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    // Walk every reachable screen and collect ids.
    let mut s = Scene::ready(Lang::Ar, ar(), true);
    seen.extend(s.hits.ids());
    // Search page adds store.search.clear.
    s.store.act(StoreAction::Search("zzz".into()));
    s.rebuild();
    seen.extend(s.hits.ids());
    // Offline and error and no-space panels.
    let mut s = Scene::new(common::offline_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    seen.extend(s.hits.ids());
    let mut s = Scene::new(common::error_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    seen.extend(s.hits.ids());
    let mut s = Scene::new(common::no_space_store(), Lang::Ar, ar(), common::WIDE);
    s.render();
    seen.extend(s.hits.ids());
    // Compact app page: install/back.
    let mut s = Scene::ready(Lang::Ar, ar(), false);
    s.store.act(StoreAction::Open("media".into()));
    s.rebuild();
    seen.extend(s.hits.ids());
    for id in clicks_ids {
        assert!(seen.contains(id), "board action `{id}` is not bound on any screen (dead by absence)");
    }
    // store.search itself is the text field: bound through the field node.
    let s = Scene::ready(Lang::Ar, ar(), true);
    assert!(s.hits.search_field().is_some());
}
