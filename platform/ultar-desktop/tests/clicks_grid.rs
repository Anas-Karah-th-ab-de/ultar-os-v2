//! Clicks of the boards AppGrid and Toasts (docs/ui/CLICKS.md) that the shell owns: every
//! one has its effect or is disabled with a reason; never a dead click.

mod common;

use common::*;
use std::collections::BTreeSet;
use ultar_desktop::model::Open;
use ultar_desktop::shell::{Effect, Opening};
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::tokens::ThemeKind;

/// Actions of the two boards that belong to other programs.
const ELSEWHERE: [&str; 5] = ["files.", "win.", "desk.icon", "toast.", "shell.drive"];
/// The notification contract has no actions yet (see the conformance matrix).
const NOT_YET: [&str; 1] = ["note.action"];
/// The app whose menu pins and unpins.
const BROWSER: &str = "org.ultar.browser";

fn actions() -> BTreeSet<String> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/ui/CLICKS.md");
    let text = std::fs::read_to_string(path).unwrap();
    let mut board = String::new();
    let mut out = BTreeSet::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("## ") {
            board = rest.split(' ').next().unwrap_or("").to_owned();
        } else if line.starts_with("| ") && (board == "AppGrid" || board == "Toasts") {
            if let Some(a) = line.split('|').nth(3).map(|c| c.trim().trim_matches('`')) {
                if a.contains('.') && !a.contains(' ') {
                    out.insert(a.to_owned());
                }
            }
        }
    }
    out
}

fn grid() -> Harness {
    let mut h = Harness::new(Lang::Ar, ThemeKind::Dark);
    h.click_bar(Target::AllApps);
    assert!(matches!(h.shell.d.open, Open::Apps(None)));
    h
}

/// Chooses item `id` of an app's context menu after opening it with the secondary button.
fn menu(h: &mut Harness, app: &str, id: u32) {
    h.right_popup(Target::AppItem(app.into()));
    let p = h.popup.as_mut().unwrap();
    assert!(p.view.ui.menu_owner().is_some(), "the context menu opened");
    let node = p.view.node(&Target::AppItem(app.into())).unwrap();
    h.dispatch_popup(vec![ultar_ui::event::Action {
        node,
        kind: ActionKind::MenuChosen(id),
    }]);
}

fn launched(h: &mut Harness) -> Vec<String> {
    h.take()
        .into_iter()
        .filter_map(|e| {
            if let Effect::Launch { app, .. } = e {
                Some(app)
            } else {
                None
            }
        })
        .collect()
}

fn check(action: &str) -> bool {
    use ultar_desktop::view::apps::item;
    match action {
        "apps.app" => {
            let mut h = grid();
            h.click_popup(Target::AppItem("org.ultar.phone".into()));
            launched(&mut h) == ["org.ultar.phone"] && h.shell.d.open == Open::None
        }
        "menu.item" => {
            let mut h = grid();
            menu(&mut h, "org.ultar.phone", item::OPEN);
            let open = launched(&mut h) == ["org.ultar.phone"];
            let mut h = grid();
            menu(&mut h, "org.ultar.phone", item::NEW_WINDOW);
            open && launched(&mut h) == ["org.ultar.phone"]
        }
        "apps.pin" => {
            let mut h = grid();
            menu(&mut h, BROWSER, item::PIN);
            let pinned = |h: &Harness| h.shell.d.places.pinned.contains(&BROWSER.to_owned());
            let gone = !pinned(&h);
            menu(&mut h, BROWSER, item::PIN);
            gone && pinned(&h)
        }
        // The Store opens the app's page or its removal dialog (launcher.start version 2);
        // an app of the base system cannot be removed.
        "apps.details" | "apps.uninstall" => {
            let remove = action == "apps.uninstall";
            let id = if remove {
                item::UNINSTALL
            } else {
                item::DETAILS
            };
            let mut h = grid();
            menu(&mut h, "org.mozilla.firefox", id);
            let want = Effect::Launch {
                app: "org.ultar.store".into(),
                open: Opening::store_page("org.mozilla.firefox", remove),
            };
            let opened = h.take() == [want];
            let d = board_desktop(Lang::Ar, ThemeKind::Dark);
            let base = d
                .catalog
                .iter()
                .find(|a| a.id == "org.ultar.files")
                .unwrap();
            let m = ultar_desktop::view::apps::menu(&d, base);
            let base_item = m.items.iter().find(|i| i.id == id).unwrap();
            opened && base_item.disabled == remove
        }
        _ => check_more(action),
    }
}

/// Search, categories and the banners.
fn check_more(action: &str) -> bool {
    match action {
        "apps.search" => {
            let mut h = grid();
            h.type_popup("هاتف");
            let one = h.popup_target(&Target::AppItem("org.ultar.phone".into()))
                && !h.popup_target(&Target::AppItem("org.ultar.files".into()));
            h.key_popup(Key::Enter);
            one && launched(&mut h) == ["org.ultar.phone"]
        }
        "apps.category" => {
            let mut h = grid();
            let p = h.popup.as_ref().unwrap();
            let tabs = p.view.node(&Target::Categories).unwrap();
            // Tab 2 = "Communication" (All, Internet, Communication, …).
            h.dispatch_popup(vec![ultar_ui::event::Action {
                node: tabs,
                kind: ActionKind::Selected(2),
            }]);
            h.popup_target(&Target::AppItem("org.ultar.phone".into()))
                && !h.popup_target(&Target::AppItem("org.ultar.files".into()))
        }
        "note.dismiss" => {
            let mut d = board_desktop(Lang::Ar, ThemeKind::Dark);
            board_notes(&mut d, Lang::Ar);
            let before = d.notes.banners(BOARD_MS).len();
            let mut h = Harness::with(d, Lang::Ar, ThemeKind::Dark);
            let out = h
                .shell
                .on_popup(&Target::Dismiss(101), &ActionKind::Activated);
            h.apply(out);
            before == 3
                && h.shell.d.notes.banners(BOARD_MS).len() == 2
                && h.shell.d.notes.list().len() == 2
        }
        a if a.starts_with("shell.") => true, // bar clicks: tests/clicks.rs
        _ => false,
    }
}

#[test]
fn every_click_of_the_app_grid_and_the_banners_has_its_effect() {
    let mut missing = Vec::new();
    for a in actions() {
        if ELSEWHERE.iter().any(|p| a.starts_with(p)) || NOT_YET.contains(&a.as_str()) {
            continue;
        }
        if !check(&a) {
            missing.push(a);
        }
    }
    assert!(missing.is_empty(), "clicks without effect: {missing:?}");
}

#[test]
fn the_grid_sorts_arabic_names_without_the_article_and_hides_empty_categories() {
    let h = grid();
    let d = &h.shell.d;
    let names: Vec<String> = ultar_desktop::view::apps::listed(d, None, "", &h.shell.tr)
        .iter()
        .map(|a| a.display_name(&h.shell.tr))
        .collect();
    assert_eq!(
        &names[..4],
        ["الإعدادات", "الطرفية", "متجر Ultar", "المتصفح"]
    );
    assert_eq!(ultar_desktop::view::apps::categories(d).len(), 4);
}

#[test]
fn the_power_button_of_the_grid_opens_the_power_menu() {
    for lang in [Lang::Ar, Lang::En] {
        let mut h = Harness::new(lang, ThemeKind::Dark);
        h.click_bar(Target::AllApps);
        assert!(
            h.popup_target(&Target::Power),
            "the grid shows the power button"
        );
        h.click_popup(Target::Power);
        assert_eq!(h.shell.d.open, Open::PowerMenu);
    }
}
