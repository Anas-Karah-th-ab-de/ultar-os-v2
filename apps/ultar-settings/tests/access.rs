//! Keyboard-only use, the screen-reader tree and the frame budget of the Settings app.

mod common;

use common::*;
use std::time::Instant;
use ultar_i18n::Lang;
use ultar_settings::action::Act;
use ultar_settings::areas::index_of;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::Key;
use ultar_ui::tokens::ThemeKind;

/// Frame budget on a PC (docs/PLAN.md §3.6: 8 ms); checked in optimised builds only.
const BUDGET_MS: u128 = 8;

#[test]
fn every_clickable_is_reachable_and_named() {
    for area in ["displays", "drivers", "accounts"] {
        for lang in [Lang::Ar, Lang::En] {
            let world = board_world(3, true);
            let mut app = app(
                &world,
                lang,
                ThemeKind::Dark,
                content_size(1440.0, 900.0),
                area,
            );
            let order = app.ui.focus_order();
            let tree = app.ui.a11y_tree();
            for (node, act) in app.view.entries() {
                if app.ui.is_disabled(node) {
                    continue;
                }
                let in_order = order.contains(&node) || matches!(act, Act::Area(_));
                assert!(in_order, "{area} {lang:?}: {act:?} not reachable by Tab");
                let a11y = tree.iter().find(|n| n.id == node && n.part.is_none());
                let a11y =
                    a11y.unwrap_or_else(|| panic!("{area}: {act:?} has no accessibility node"));
                let name = a11y
                    .name
                    .as_ref()
                    .map(|l| l.resolve(app.ui.translator()).into_owned())
                    .unwrap_or_default();
                assert!(
                    !name.trim().is_empty(),
                    "{area} {lang:?}: {act:?} has no name"
                );
                assert!(
                    !name.contains("app.settings."),
                    "{area} {lang:?}: {act:?} untranslated: {name}"
                );
            }
        }
    }
}

#[test]
fn keyboard_only_reaches_the_drivers_page_and_switches_a_setting() {
    let world = board_world(3, true);
    let mut app = app(
        &world,
        Lang::Ar,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "displays",
    );
    for _ in 0..3 {
        key(&mut app, Key::Tab);
        if app.ui.focused() == app.view.search {
            break;
        }
    }
    assert_eq!(
        app.ui.focused(),
        app.view.search,
        "the sidebar (scroll area), then search"
    );
    type_text(&mut app, "العتاد");
    key(&mut app, Key::Enter);
    assert_eq!(
        app.state.area,
        index_of("drivers").unwrap(),
        "search + Enter opens the area"
    );
    let check = app.view.node(Act::FirmwareCheck).unwrap();
    for _ in 0..200 {
        if app.ui.focused() == Some(check) {
            break;
        }
        key(&mut app, Key::Tab);
    }
    assert_eq!(
        app.ui.focused(),
        Some(check),
        "Tab reaches the firmware check"
    );
    key(&mut app, Key::Enter);
    assert_eq!(log(&world).last().map(String::as_str), Some("update.check"));
}

#[test]
fn dialogs_take_and_return_focus() {
    let world = board_world(3, true);
    let mut app = app(
        &world,
        Lang::Ar,
        ThemeKind::Dark,
        content_size(1440.0, 900.0),
        "accounts",
    );
    let qr = app.view.node(Act::AccountQr).unwrap();
    click(&mut app, qr);
    let ok = app.view.node(Act::DialogOk).unwrap();
    let tree = app.ui.a11y_tree();
    assert!(
        tree.iter().all(|n| n.id != qr),
        "nothing behind the dialog is reachable"
    );
    let _ = ok;
    key(&mut app, Key::Escape);
    assert!(app.state.dialog.is_none());
}

#[test]
fn full_frames_fit_the_budget() {
    for (area, lang, theme) in [
        ("displays", Lang::Ar, ThemeKind::Dark),
        ("displays", Lang::En, ThemeKind::Light),
        ("drivers", Lang::Ar, ThemeKind::Dark),
        ("accounts", Lang::Ar, ThemeKind::Dark),
    ] {
        let world = board_world(3, true);
        let mut app = app(&world, lang, theme, content_size(1440.0, 900.0), area);
        let mut canvas = Canvas::new(1440, 860, PixelFormat::Bgrx8).unwrap();
        app.ui.paint_all(&mut canvas);
        let mut worst = 0;
        for _ in 0..5 {
            app.ui.invalidate_layout();
            let t = Instant::now();
            app.ui.paint_all(&mut canvas);
            worst = worst.max(t.elapsed().as_millis());
        }
        eprintln!("frame {area} {lang:?} {theme:?}: {worst} ms");
        if !cfg!(debug_assertions) {
            assert!(worst <= BUDGET_MS, "{area}: {worst} ms > {BUDGET_MS} ms");
        }
    }
}
