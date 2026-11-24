//! Every `browser.*` click of docs/ui/CLICKS.md (board "DesktopBottom") driven through the
//! real window with synthetic pointer and keyboard input over the test engine.

mod common;

use common::*;
use ultar_ui::event::PointerButton;
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Lang;
use ultar_web::app::Overlay;
use ultar_web::chrome::Act;
use ultar_web::engine::test_engine::Recorded;
use ultar_web::engine::{PermissionKind, PointerInput};

#[test]
fn newtab_tab_and_tabclose() {
    let mut b = browser(Lang::En, ThemeKind::Light);
    assert_eq!(b.state().tabs.len(), 1);
    // A new tab starts on the start page with the caret in its search field.
    let start = b.chrome().start_search.unwrap();
    assert_eq!(b.ui().focused(), Some(start));
    {
        let n = b.chrome().new_tab;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.len(), 2);
    assert_eq!(b.state().tabs.active_index(), 1);
    // browser.tab shows that tab.
    {
        let n = b.chrome().tabs[0].0;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.active_index(), 0);
    assert_eq!(
        b.engine().is_visible(b.state().tabs.list()[1].engine),
        Some(false)
    );
    // browser.tabclose closes this tab; the last one closes the window.
    {
        let n = b.chrome().tabs[1].1;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.len(), 1);
    assert!(!b.is_closed());
    {
        let n = b.chrome().tabs[0].1;
        click(&mut b, n);
    }
    assert!(b.is_closed());
    assert_eq!(b.engine().tab_count(), 0);
}

#[test]
fn address_opens_addresses_searches_and_refuses() {
    let mut b = browser(Lang::En, ThemeKind::Light);
    open(&mut b, "code-craft.tech");
    let tab = b.state().tabs.active().unwrap().clone();
    assert_eq!(tab.url, "https://code-craft.tech/");
    assert_eq!(tab.title, "Page code-craft.tech");
    assert_eq!(
        b.ui().text(b.chrome().address).unwrap(),
        "https://code-craft.tech/"
    );
    assert!(b.chrome().page.is_some() && b.chrome().start_search.is_none());
    open(&mut b, "rust language");
    assert!(b
        .state()
        .tabs
        .active()
        .unwrap()
        .url
        .starts_with("https://duckduckgo.com/?q=rust+language"));
    open(&mut b, "javascript:alert(1)");
    assert!(b.ui().error(b.chrome().address).is_some());
    assert!(b
        .state()
        .tabs
        .active()
        .unwrap()
        .url
        .starts_with("https://duckduckgo.com/"));
    // The start page's field (the second browser.address of the board).
    {
        let n = b.chrome().new_tab;
        click(&mut b, n);
    }
    type_enter(&mut b, "example.org");
    assert_eq!(b.state().tabs.active().unwrap().url, "https://example.org/");
}

#[test]
fn back_forward_reload_and_stop() {
    let mut b = browser(Lang::En, ThemeKind::Light);
    assert!(b.ui().is_disabled(b.chrome().back));
    assert!(b.ui().is_disabled(b.chrome().forward));
    open(&mut b, "a.example");
    open(&mut b, "b.example");
    assert!(!b.ui().is_disabled(b.chrome().back));
    {
        let n = b.chrome().back;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.active().unwrap().url, "https://a.example/");
    assert!(!b.ui().is_disabled(b.chrome().forward));
    {
        let n = b.chrome().forward;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.active().unwrap().url, "https://b.example/");
    let before = b.engine().recorded().len();
    {
        let n = b.chrome().reload;
        click(&mut b, n);
    }
    assert_eq!(b.state().tabs.active().unwrap().url, "https://b.example/");
    assert_eq!(b.engine().recorded().len(), before);
    // While loading the same place stops the load.
    open(&mut b, "c.example/slow");
    assert!(b.chrome().is_loading());
    assert_eq!(b.chrome().act(b.chrome().reload), Some(Act::Reload));
    {
        let n = b.chrome().reload;
        click(&mut b, n);
    }
    assert!(!b.state().tabs.active().unwrap().loading);
    assert!(!b.chrome().is_loading());
}

#[test]
fn shield_opens_the_page_panel_without_switching_anything() {
    let mut b = browser(Lang::En, ThemeKind::Light);
    open(&mut b, "site.example/trackers/5");
    assert_eq!(b.state().tabs.active().unwrap().blocked, Some(5));
    {
        let n = b.chrome().shield;
        click(&mut b, n);
    }
    let Overlay::Shield(panel) = b.overlay().clone() else {
        panic!("page panel not open");
    };
    assert!(b.ui().is_on(panel.site).unwrap());
    assert_eq!(panel.rights.len(), 4);
    assert!(b.state().settings.protects("site.example"));
    // Switching "Protection for this site" off lists the site and reloads it unprotected.
    click(&mut b, panel.site);
    assert!(!b.state().settings.protects("site.example"));
    assert_eq!(b.state().tabs.active().unwrap().blocked, Some(0));
    assert!(matches!(b.overlay(), Overlay::None));
    // A right chosen in the panel is remembered.
    {
        let n = b.chrome().shield;
        click(&mut b, n);
    }
    let Overlay::Shield(panel) = b.overlay().clone() else {
        panic!()
    };
    let (camera, kind) = panel.rights[0];
    assert_eq!(kind, PermissionKind::Camera);
    b.ui_mut().focus(camera).unwrap();
    b.ui_mut().set_selected(camera, 2).unwrap();
    let actions = vec![ultar_ui::event::Action {
        node: camera,
        kind: ultar_ui::event::ActionKind::Selected(2),
    }];
    b.handle_actions(&actions).unwrap();
    assert_eq!(
        b.state()
            .permissions
            .get("site.example", PermissionKind::Camera),
        ultar_web::model::permissions::Decision::Block
    );
    click(&mut b, panel.close);
    assert!(matches!(b.overlay(), Overlay::None));
}

#[test]
fn downloads_list_open_show_clear_cancel() {
    let files = Files::default();
    let shown = files.shown.clone();
    let (mut b, paths) = browser_with_profile("downloads", files);
    {
        let n = b.chrome().downloads;
        click(&mut b, n);
    }
    assert!(matches!(b.overlay(), Overlay::Downloads(p) if p.open.is_empty() && p.clear.is_none()));
    b.close_overlay().unwrap();
    open(&mut b, "files.example/download/report.pdf");
    assert_eq!(b.state().downloads.list().len(), 1);
    {
        let n = b.chrome().downloads;
        click(&mut b, n);
    }
    let Overlay::Downloads(panel) = b.overlay().clone() else {
        panic!()
    };
    click(&mut b, panel.show[0].0);
    assert_eq!(
        shown.borrow().as_slice(),
        [paths.downloads.join("report.pdf")]
    );
    click(&mut b, panel.open[0].0);
    let url = b.state().tabs.active().unwrap().url.clone();
    assert!(
        url.starts_with("file://") && url.ends_with("/Downloads/report.pdf"),
        "{url}"
    );
    {
        let n = b.chrome().downloads;
        click(&mut b, n);
    }
    let Overlay::Downloads(panel) = b.overlay().clone() else {
        panic!()
    };
    click(&mut b, panel.clear.unwrap());
    assert!(b.state().downloads.list().is_empty());
    // Cancel of a running download.
    b.engine_event(ultar_web::engine::EngineEvent::DownloadStarted {
        download: ultar_web::engine::DownloadId(77),
        url: "https://x/".into(),
        name: "../big.iso".into(),
    })
    .unwrap();
    assert_eq!(b.state().downloads.list()[0].name, "big.iso");
    b.open_downloads().unwrap();
    let Overlay::Downloads(panel) = b.overlay().clone() else {
        panic!()
    };
    click(&mut b, panel.cancel[0].0);
    assert!(b
        .engine()
        .recorded()
        .contains(&Recorded::Cancel(ultar_web::engine::DownloadId(77))));
}

#[test]
fn show_in_folder_without_the_files_app_says_so() {
    let (mut b, _) = browser_with_profile(
        "nofiles",
        Files {
            missing: true,
            ..Files::default()
        },
    );
    open(&mut b, "f.example/download/a.txt");
    b.open_downloads().unwrap();
    let Overlay::Downloads(panel) = b.overlay().clone() else {
        panic!()
    };
    assert!(b.ui().toasts().is_empty());
    click(&mut b, panel.show[0].0);
    assert_eq!(b.ui().toasts().len(), 1);
}

#[test]
fn permission_prompt_allow_block_and_remember() {
    let (mut b, paths) = browser_with_profile("perm", Files::default());
    open(&mut b, "cam.example/ask/camera");
    let Overlay::Prompt(prompt, pending) = b.overlay().clone() else {
        panic!("no prompt")
    };
    assert_eq!(pending.kind, PermissionKind::Camera);
    assert_eq!(b.ui().focused(), Some(prompt.block));
    click(&mut b, prompt.remember);
    click(&mut b, prompt.allow);
    assert!(b
        .engine()
        .recorded()
        .contains(&Recorded::Answer(pending.request, true)));
    assert_eq!(
        b.state()
            .permissions
            .get("cam.example", PermissionKind::Camera),
        ultar_web::model::permissions::Decision::Allow
    );
    // Remembered: no second prompt, answered at once; written to the profile.
    open(&mut b, "cam.example/ask/camera");
    assert!(matches!(b.overlay(), Overlay::None));
    let saved = ultar_web::model::permissions::SitePermissions::load(
        &paths.profile.join("permissions.tsv"),
    )
    .unwrap();
    assert_eq!(saved.len(), 1);
    // Escape blocks without remembering.
    open(&mut b, "mic.example/ask/microphone");
    let Overlay::Prompt(_, pending) = b.overlay().clone() else {
        panic!()
    };
    b.escape().unwrap();
    assert!(b
        .engine()
        .recorded()
        .contains(&Recorded::Answer(pending.request, false)));
    assert_eq!(
        b.state()
            .permissions
            .get("mic.example", PermissionKind::Microphone),
        ultar_web::model::permissions::Decision::Ask
    );
}

#[test]
fn private_tabs_keep_nothing() {
    let (mut b, _) = browser_with_profile("private", Files::default());
    shortcut(&mut b, 49, 1 | 2); // Ctrl+Shift+N
    assert!(b.state().tabs.active().unwrap().private);
    assert_eq!(
        b.engine()
            .is_private(b.state().tabs.active().unwrap().engine),
        Some(true)
    );
    open(&mut b, "secret.example/ask/location");
    assert!(b.state().history.is_empty());
    let Overlay::Prompt(prompt, _) = b.overlay().clone() else {
        panic!()
    };
    assert!(b.ui().is_disabled(prompt.remember));
    click(&mut b, prompt.allow);
    assert!(b.state().permissions.is_empty());
    assert_eq!(b.state().private_permissions.len(), 1);
    let index = b.state().tabs.active_index();
    b.close_tab(index).unwrap();
    assert!(b.state().private_permissions.is_empty());
}

#[test]
fn menu_settings_and_shortcuts() {
    let (mut b, paths) = browser_with_profile("menu", Files::default());
    // Secondary click on the toolbar opens the menu; choosing "Settings" opens the dialog.
    let toolbar = b.chrome().toolbar;
    let mut pos = center(&mut b, toolbar);
    let back = b.chrome().back;
    pos.x = b.ui().rect(back).unwrap().x - 4.0;
    click_at(&mut b, pos, PointerButton::Secondary);
    assert!(b.ui().menu_owner().is_some());
    b.handle_actions(&[ultar_ui::event::Action {
        node: b.chrome().downloads,
        kind: ultar_ui::event::ActionKind::MenuChosen(5),
    }])
    .unwrap();
    let Overlay::Settings(d) = b.overlay().clone() else {
        panic!()
    };
    click(&mut b, d.protection);
    assert!(!b.state().settings.protection);
    click(&mut b, d.close);
    let saved =
        ultar_web::model::settings::Settings::load(&paths.profile.join("settings.tsv")).unwrap();
    assert!(!saved.protection);
    // Shortcuts: Ctrl+T, Ctrl+Tab, Ctrl+W, F10.
    shortcut(&mut b, 20, 2);
    assert_eq!(b.state().tabs.len(), 2);
    shortcut(&mut b, 15, 2);
    assert_eq!(b.state().tabs.active_index(), 0);
    shortcut(&mut b, 17, 2);
    assert_eq!(b.state().tabs.len(), 1);
    shortcut(&mut b, 68, 0);
    assert!(b.ui().menu_owner().is_some());
}

#[test]
fn the_page_gets_pointer_and_keys() {
    let mut b = browser(Lang::En, ThemeKind::Light);
    open(&mut b, "p.example");
    let page = b.chrome().page.unwrap();
    let rect = b.ui().rect(page).unwrap();
    let pos = ultar_ui::Point::new(rect.x + 10.0, rect.y + 20.0);
    click_at(&mut b, pos, PointerButton::Primary);
    let tab = b.state().tabs.active().unwrap().engine;
    assert!(b.engine().recorded().contains(&Recorded::Pointer(
        tab,
        PointerInput::Down {
            x: 10.0,
            y: 20.0,
            button: 1
        }
    )));
    assert_eq!(b.ui().focused(), Some(page));
    b.key(
        ultar_web::engine::KeyInput {
            code: 30,
            character: 'a' as u32,
            modifiers: 0,
            pressed: true,
        },
        None,
    )
    .unwrap();
    assert!(b
        .engine()
        .recorded()
        .iter()
        .any(|r| matches!(r, Recorded::Key(_, k) if k.character == 'a' as u32)));
    // The engine was told the page size in device pixels.
    assert!(b.engine().recorded().iter().any(
        |r| matches!(r, Recorded::Resize(_, w, h, _) if *w == rect.w as u32 && *h == rect.h as u32)
    ));
}

#[test]
fn new_tab_requests_crashes_and_suggestions() {
    let mut b = browser(Lang::Ar, ThemeKind::Dark);
    open(&mut b, "opener.example/popup");
    assert_eq!(b.state().tabs.len(), 2);
    assert_eq!(
        b.state().tabs.active().unwrap().url,
        "https://popup.example/"
    );
    open(&mut b, "x.example/crash");
    assert!(
        b.state().tabs.active().unwrap().crashed,
        "{:?}",
        b.state().tabs.active()
    );
    assert_eq!(b.ui().toasts().len(), 1);
    // Down in the address field offers pages from the history.
    let field = b.chrome().address;
    b.ui_mut().focus(field).unwrap();
    b.ui_mut().set_text(field, "popup.example").unwrap();
    shortcut(&mut b, 108, 0);
    assert!(b.ui().menu_owner().is_some());
    b.handle_actions(&[ultar_ui::event::Action {
        node: field,
        kind: ultar_ui::event::ActionKind::MenuChosen(100),
    }])
    .unwrap();
    b.pump(None).unwrap();
    assert_eq!(
        b.state().tabs.active().unwrap().url,
        "https://popup.example/"
    );
    assert!(!b.state().tabs.active().unwrap().crashed);
}
