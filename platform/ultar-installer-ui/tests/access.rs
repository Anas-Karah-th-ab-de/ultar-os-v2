//! Keyboard-only installation flow, the screen-reader tree and the frame budget.

mod common;

use common::*;
use std::time::Instant;
use ultar_i18n::Lang;
use ultar_installer_ui::wizard::Step;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key, Modifiers};
use ultar_ui::tokens::ThemeKind;
use ultar_ui::Size;

const BUDGET_MS: u128 = 8;

fn key(app: &mut ultar_installer_ui::InstallerApp, k: Key) {
    app.handle(Event::Key {
        key: k,
        modifiers: Modifiers::NONE,
    })
    .unwrap();
}

#[test]
fn every_clickable_is_named_and_reachable_on_every_step() {
    let dir = scratch("access");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    for step in Step::ALL.iter().copied().filter(|s| *s <= Step::Summary) {
        for lang in [Lang::Ar, Lang::En] {
            let world = world();
            let mut app = app_with(
                &world,
                wizard_at(&src, vec![disk_info(&tgt, "Disk")], step),
                lang,
                ThemeKind::Dark,
                Size::new(1280.0, 800.0),
            );
            if step == Step::Drivers {
                app.load_devices();
                app.rebuild().unwrap();
            }
            let order = app.ui.focus_order();
            let tree = app.ui.a11y_tree();
            for (node, act) in app.view.registry.entries().to_vec() {
                if app.ui.is_disabled(node) {
                    continue;
                }
                let listed = matches!(act, ultar_installer_ui::action::Act::Step(_));
                assert!(
                    listed || order.contains(&node),
                    "{step:?} {lang:?}: {act:?} not reachable by Tab"
                );
                let n = tree
                    .iter()
                    .find(|n| n.id == node && n.part.is_none())
                    .unwrap_or_else(|| panic!("{step:?}: {act:?} no a11y node"));
                let name = n
                    .name
                    .as_ref()
                    .map(|l| l.resolve(app.ui.translator()).into_owned())
                    .unwrap_or_default();
                assert!(
                    !name.trim().is_empty() && !name.starts_with("installer."),
                    "{step:?} {lang:?}: {act:?} name {name:?}"
                );
            }
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// The password and its repetition, typed into their fields by keyboard only.
fn type_password(app: &mut ultar_installer_ui::InstallerApp) {
    for n in 0..2 {
        let field = app
            .view
            .registry
            .node(ultar_installer_ui::action::Act::Password(n))
            .unwrap();
        let mut guard = 0;
        while app.ui.focused() != Some(field) && guard < 300 {
            key(app, Key::Tab);
            guard += 1;
        }
        for c in "km-secret-1".chars() {
            key(app, Key::Char(c));
        }
    }
}

#[test]
fn keyboard_only_from_language_to_summary_and_install() {
    let dir = scratch("kbd");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    let world = world();
    let mut w = wizard_at(&src, vec![disk_info(&tgt, "Disk")], Step::Language);
    w.reached = Step::Language;
    let mut app = app_with(
        &world,
        w,
        Lang::Ar,
        ThemeKind::Dark,
        Size::new(1280.0, 800.0),
    );
    for _ in 0..2000 {
        if app.model.wizard.step == Step::Summary {
            break;
        }
        if app.model.wizard.step == Step::Account && app.ui.is_disabled(app.view.next.unwrap()) {
            type_password(&mut app);
        }
        let next = app.view.next.unwrap();
        let mut guard = 0;
        while app.ui.focused() != Some(next) && guard < 300 {
            key(&mut app, Key::Tab);
            guard += 1;
        }
        assert_eq!(
            app.ui.focused(),
            Some(next),
            "Tab reaches Next on {:?}",
            app.model.wizard.step
        );
        key(&mut app, Key::Enter);
    }
    assert_eq!(app.model.wizard.step, Step::Summary);
    let confirm = app
        .view
        .registry
        .node(ultar_installer_ui::action::Act::Confirm)
        .unwrap();
    while app.ui.focused() != Some(confirm) {
        key(&mut app, Key::Tab);
    }
    key(&mut app, Key::Space);
    assert!(
        app.model.wizard.erase_confirmed,
        "Space ticks the erase line"
    );
    let next = app.view.next.unwrap();
    while app.ui.focused() != Some(next) {
        key(&mut app, Key::Tab);
    }
    key(&mut app, Key::Enter);
    assert!(app.take_install().is_some(), "Install by keyboard");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn full_frames_fit_the_budget() {
    let dir = scratch("frame");
    let src = medium(&dir);
    let tgt = target(&dir, 300 * MIB);
    for step in [Step::Components, Step::Disk, Step::Drivers, Step::Summary] {
        let world = world();
        let mut app = app_with(
            &world,
            wizard_at(&src, vec![disk_info(&tgt, "Disk")], step),
            Lang::Ar,
            ThemeKind::Dark,
            Size::new(1280.0, 800.0),
        );
        app.load_devices();
        app.rebuild().unwrap();
        let mut canvas = Canvas::new(1280, 800, PixelFormat::Bgrx8).unwrap();
        app.ui.paint_all(&mut canvas);
        let mut worst = 0;
        for _ in 0..5 {
            app.ui.invalidate_layout();
            let t = Instant::now();
            app.ui.paint_all(&mut canvas);
            worst = worst.max(t.elapsed().as_millis());
        }
        eprintln!("frame {step:?}: {worst} ms");
        if !cfg!(debug_assertions) {
            assert!(worst <= BUDGET_MS, "{step:?}: {worst} ms");
        }
    }
    std::fs::remove_dir_all(dir).unwrap();
}
