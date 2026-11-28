//! The desktop background (DESKTOP-SPEC §5.1 "Arbeitsfläche"): its right-click menu and the
//! app shortcuts on it – added from the menu ("New shortcut" → app grid) and from an app's
//! menu in the app grid, selected, opened, renamed, pinned, removed, sorted, sized, hidden,
//! placed from their corner – and the pictures of the background with shortcuts, both menus
//! and the rename dialog in both themes and both languages.

mod common;

use common::shots::*;
use common::*;
use ultar_bus::messages::settings::SettingValue;
use ultar_desktop::model::prefs::{Corner, IconSize};
use ultar_desktop::model::Open;
use ultar_desktop::shell::{desk_item, shortcut_item, Effect, Opening, Outcome};
use ultar_desktop::view::apps::item;
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_surface::proto::WindowAction;
use ultar_ui::event::{ActionKind, Key};
use ultar_ui::geom::Size;
use ultar_ui::snapshot::Image;
use ultar_ui::tokens::ThemeKind;

const FILES: &str = "org.ultar.files";
const BROWSER: &str = "org.ultar.browser";
const TERMINAL: &str = "org.ultar.terminal";

fn h() -> Harness {
    Harness::new(Lang::En, ThemeKind::Dark)
}

/// The board's desktop with shortcuts of Files, the browser, the terminal and Firefox.
fn with_shortcuts(lang: Lang, theme: ThemeKind) -> Harness {
    let mut h = Harness::new(lang, theme);
    for app in [FILES, BROWSER, TERMINAL, "org.mozilla.firefox"] {
        h.shell.d.places.add(app);
    }
    h.shell.d.places.selected = None;
    h
}

/// An item of the open menu chosen.
fn choose(h: &mut Harness, id: u32) {
    let out = h.shell.on_menu(id);
    h.apply(out);
}

fn shortcuts(h: &Harness) -> Vec<String> {
    h.shell
        .d
        .places
        .shortcuts
        .iter()
        .map(|s| s.app.clone())
        .collect()
}

#[test]
fn a_right_click_on_the_background_opens_its_menu_at_the_pointer() {
    let mut h = h();
    h.right_background(None, (300, 200));
    assert_eq!(h.shell.d.open, Open::DeskMenu(300, 200));
    assert!(h.popup_target(&Target::Menu), "the menu is a popup");
    let p = h.popup.as_ref().unwrap();
    // The card's corner is at the pointer (the surface holds its shadow before it).
    let bar_top = SCREEN.h as i32 - 56;
    let (x, y) = (p.offset.0, bar_top + p.offset.1);
    assert!((300 - 32..=300).contains(&x), "x {x}");
    assert!((200 - 32..=200).contains(&y), "y {y}");
    // Near the end edge and the bar it turns to the other side and above the pointer.
    h.shell.close();
    h.right_background(None, (SCREEN.w as i32 - 10, SCREEN.h as i32 - 70));
    let p = h.popup.as_ref().unwrap();
    assert!(p.offset.0 as f32 + p.size.w <= SCREEN.w + 1.0);
    assert!(bar_top + p.offset.1 + (p.size.h as i32) <= SCREEN.h as i32);
    // Arabic: it opens towards the end side, the left.
    let mut ar = Harness::new(Lang::Ar, ThemeKind::Dark);
    ar.right_background(None, (700, 200));
    let p = ar.popup.as_ref().unwrap();
    assert!(
        p.offset.0 as f32 + p.size.w <= 700.0 + 32.0,
        "{:?}",
        p.offset
    );
    // The top bar: the menu stays where the pointer is on the screen.
    let mut top = Harness::top(Lang::En, ThemeKind::Dark);
    top.right_background(None, (300, 200));
    let p = top.popup.as_ref().unwrap();
    assert!((200 - 32..=200).contains(&p.offset.1), "{:?}", p.offset);
}

#[test]
fn new_shortcut_chooses_the_app_in_the_app_grid() {
    let mut h = h();
    h.right_background(None, (300, 200));
    choose(&mut h, desk_item::NEW_SHORTCUT);
    assert_eq!(h.shell.d.open, Open::Apps(None));
    assert!(h.shell.d.places.picking);
    h.click_popup(Target::AppItem(BROWSER.into()));
    assert_eq!(shortcuts(&h), vec![BROWSER.to_owned()]);
    assert_eq!(h.shell.d.open, Open::None, "chosen: the grid closes");
    assert!(!h.shell.d.places.picking);
    assert!(
        h.take().contains(&Effect::Shortcuts),
        "the list is written down"
    );
    // A grid opened from the bar starts apps again.
    h.click_bar(Target::AllApps);
    assert!(!h.shell.d.places.picking);
    h.click_popup(Target::AppItem(TERMINAL.into()));
    assert_eq!(shortcuts(&h), vec![BROWSER.to_owned()]);
    // Esc while choosing ends the choice.
    h.right_background(None, (300, 200));
    choose(&mut h, desk_item::NEW_SHORTCUT);
    h.key_popup(Key::Escape);
    assert!(!h.shell.d.places.picking && h.shell.d.open == Open::None);
}

#[test]
fn the_app_grid_puts_an_app_on_the_desktop_and_takes_it_off() {
    let mut h = h();
    h.click_bar(Target::AllApps);
    h.act(
        Target::AppItem(FILES.into()),
        ActionKind::MenuChosen(item::DESKTOP),
    );
    assert_eq!(shortcuts(&h), vec![FILES.to_owned()]);
    assert!(h.take().contains(&Effect::Shortcuts));
    let bg = h.background();
    assert!(
        bg.node(&Target::Shortcut(0)).is_some(),
        "it is on the background"
    );
    h.click_bar(Target::AllApps);
    h.act(
        Target::AppItem(FILES.into()),
        ActionKind::MenuChosen(item::DESKTOP),
    );
    assert!(shortcuts(&h).is_empty(), "the second time takes it off");
}

#[test]
fn a_click_selects_and_a_double_click_opens() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    let mut bg = h.background();
    h.click_background(&mut bg, Target::Shortcut(1), 1);
    assert_eq!(h.shell.d.places.selected, Some(1));
    assert!(h.take().is_empty(), "a single click only selects");
    let mut bg = h.background();
    h.click_background(&mut bg, Target::Shortcut(1), 2);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: BROWSER.into(),
            open: Opening::start()
        }]
    );
    // An app with a window: the window comes forward.
    let mut bg = h.background();
    h.click_background(&mut bg, Target::Shortcut(0), 2);
    assert_eq!(h.take(), vec![Effect::Window(2, WindowAction::Activate)]);
}

#[test]
fn the_shortcut_menu_renames() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    h.right_background(Some(Target::Shortcut(2)), (100, 300));
    assert_eq!(h.shell.d.open, Open::ShortcutMenu(2, 100, 300));
    assert_eq!(
        h.shell.d.places.selected,
        Some(2),
        "the clicked one is selected"
    );
    // Rename: the dialog with the shown name, typed, Enter.
    choose(&mut h, shortcut_item::RENAME);
    assert_eq!(h.shell.d.open, Open::Rename(2));
    assert!(h.popup_target(&Target::RenameField));
    let p = h.popup.as_ref().unwrap();
    let field = p.view.node(&Target::RenameField).unwrap();
    assert_eq!(p.view.ui.text(field).unwrap(), "Terminal");
    assert_eq!(
        p.view.ui.focused(),
        Some(field),
        "the caret is in the field"
    );
    h.key_popup(Key::Backspace);
    h.type_popup("l – Arbeit");
    assert_eq!(h.shell.d.places.draft, "Terminal – Arbeit");
    h.key_popup(Key::Enter);
    assert_eq!(h.shell.d.open, Open::None);
    assert_eq!(
        h.shell.d.places.shortcuts[2].name.as_deref(),
        Some("Terminal – Arbeit")
    );
    assert!(h.take().contains(&Effect::Shortcuts));
    // Cancel keeps the name.
    h.right_background(Some(Target::Shortcut(2)), (100, 300));
    choose(&mut h, shortcut_item::RENAME);
    h.type_popup("x");
    h.click_popup(Target::RenameCancel);
    assert_eq!(
        h.shell.d.places.shortcuts[2].name.as_deref(),
        Some("Terminal – Arbeit")
    );
}

#[test]
fn the_shortcut_menu_pins_shows_properties_and_removes() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    // Pin to the bar (Firefox is not pinned yet).
    h.right_background(Some(Target::Shortcut(3)), (100, 300));
    choose(&mut h, shortcut_item::PIN);
    let firefox = "org.mozilla.firefox".to_owned();
    assert!(h.shell.d.places.pinned.contains(&firefox));
    // Properties: the app's page in Settings.
    h.right_background(Some(Target::Shortcut(3)), (100, 300));
    choose(&mut h, shortcut_item::PROPERTIES);
    assert_eq!(
        h.take(),
        vec![Effect::Launch {
            app: "org.ultar.settings".into(),
            open: Opening::settings_page("apps/org.mozilla.firefox"),
        }]
    );
    // Remove from the desktop: the app stays installed.
    h.right_background(Some(Target::Shortcut(0)), (100, 300));
    choose(&mut h, shortcut_item::REMOVE);
    assert_eq!(shortcuts(&h)[0], BROWSER);
    assert!(h.shell.d.catalog.iter().any(|a| a.id == FILES));
    assert!(h.take().contains(&Effect::Shortcuts));
}

#[test]
fn the_menu_sets_size_visibility_and_order() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    let menu = |h: &mut Harness, id: u32| {
        h.right_background(None, (300, 200));
        choose(h, id);
        h.take()
    };
    let large = menu(&mut h, desk_item::SIZE);
    assert_eq!(h.shell.d.prefs.desk.size, IconSize::Large);
    assert!(large.contains(&Effect::Setting(
        "desktop.icons.size".into(),
        SettingValue::Text("large".into())
    )));
    let large_bg = h.background();
    menu(&mut h, desk_item::SIZE + 2);
    assert_eq!(h.shell.d.prefs.desk.size, IconSize::Small);
    let cell =
        |v: &ultar_desktop::view::View| v.ui.rect(v.node(&Target::Shortcut(0)).unwrap()).unwrap();
    assert!(
        cell(&h.background()).w < cell(&large_bg).w,
        "small icons are smaller"
    );
    // Hidden: nothing on the background; shown again.
    let hide = menu(&mut h, desk_item::SHOW);
    assert!(hide.contains(&Effect::Setting(
        "desktop.icons.enabled".into(),
        SettingValue::Flag(false)
    )));
    assert!(h.background().targets.is_empty());
    menu(&mut h, desk_item::SHOW);
    assert!(h.shell.d.prefs.desk.shown);
    // Sorted by the shown names (Browser, Files, Firefox, Terminal).
    assert!(menu(&mut h, desk_item::SORT).contains(&Effect::Shortcuts));
    assert_eq!(
        shortcuts(&h),
        vec![BROWSER, FILES, "org.mozilla.firefox", TERMINAL]
    );
}

#[test]
fn the_menu_leads_to_settings_and_the_terminal() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    let menu = |h: &mut Harness, id: u32| {
        h.right_background(None, (300, 200));
        choose(h, id);
        h.take()
    };
    for (id, page) in [
        (desk_item::WALLPAPER, "appearance"),
        (desk_item::DISPLAY, "display"),
        (desk_item::SETTINGS, "desktop_bar"),
    ] {
        assert_eq!(
            menu(&mut h, id),
            vec![Effect::Launch {
                app: "org.ultar.settings".into(),
                open: Opening::settings_page(page),
            }]
        );
    }
    assert_eq!(
        menu(&mut h, desk_item::TERMINAL),
        vec![Effect::Launch {
            app: TERMINAL.into(),
            open: Opening::start(),
        }]
    );
    // What needs the files service is not offered as working.
    for id in [desk_item::NEW_FOLDER, desk_item::PASTE] {
        assert!(menu(&mut h, id).is_empty());
    }
}

#[test]
fn the_shortcuts_follow_their_settings_and_their_corner() {
    let mut h = with_shortcuts(Lang::En, ThemeKind::Dark);
    let first = |h: &Harness| {
        let v = h.background();
        v.ui.rect(v.node(&Target::Shortcut(0)).unwrap()).unwrap()
    };
    let r = first(&h);
    assert_eq!((r.x, r.y), (24.0, 24.0), "top start, 24 from the edges");
    h.shell.d.prefs.desk.corner = Corner::BottomEnd;
    let r = first(&h);
    assert!((SCREEN.w - r.right() - 24.0).abs() < 0.5, "{r:?}");
    assert!((SCREEN.h - 56.0 - r.bottom() - 24.0).abs() < 0.5, "{r:?}");
    // Arabic: the start is on the right.
    let mut ar = with_shortcuts(Lang::Ar, ThemeKind::Dark);
    ar.shell.d.prefs.desk.corner = Corner::TopStart;
    let r = first(&ar);
    assert!((SCREEN.w - r.right() - 24.0).abs() < 0.5, "{r:?}");
    // A full column continues in the next one.
    let mut many = Harness::new(Lang::En, ThemeKind::Dark);
    for i in 0..30 {
        let id = format!("org.example.app{i}");
        many.shell
            .d
            .catalog
            .push(ultar_desktop::model::apps::installed(&id));
        many.shell.d.places.add(&id);
    }
    let v = many.background();
    let x = |i: usize| v.ui.rect(v.node(&Target::Shortcut(i)).unwrap()).unwrap().x;
    assert!(x(29) > x(0), "several columns");
    // Uninstalled apps are not shown, a menu over nothing changes nothing.
    many.shell.d.catalog.retain(|a| a.id != "org.example.app0");
    assert!(many.background().node(&Target::Shortcut(0)).is_none());
    many.right_background(Some(Target::Shortcut(99)), (1, 1));
    assert_eq!(many.shell.d.open, Open::DeskMenu(1, 1));
}

/// The background, the bar and the popup composed like the compositor does.
fn screen(h: &mut Harness) -> Image {
    let mut img = blank_of(h.look.theme, SCREEN);
    let mut bg = h.background();
    over(&mut img, &mut bg, SCREEN, (0, 0));
    let bar_top = (SCREEN.h - 56.0) as i32;
    over(
        &mut img,
        &mut h.bar,
        Size::new(SCREEN.w, 56.0),
        (0, bar_top),
    );
    if let Some(p) = &mut h.popup {
        let size = p.size;
        let off = (p.offset.0, bar_top + p.offset.1);
        over(&mut img, &mut p.view, size, off);
    }
    img
}

#[test]
fn the_background_its_menus_and_the_rename_dialog_match_their_references() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        let mut h = with_shortcuts(lang, theme);
        h.shell.d.places.selected = Some(1);
        h.apply(Outcome::surfaces());
        let img = screen(&mut h);
        check("desk-shortcuts", lang, theme, &img, &mut problems);
        let at = if lang == Lang::Ar {
            (900, 260)
        } else {
            (540, 260)
        };
        h.right_background(None, at);
        let img = screen(&mut h);
        check("desk-menu", lang, theme, &img, &mut problems);
        h.shell.close();
        h.right_background(Some(Target::Shortcut(2)), (130, 300));
        let img = screen(&mut h);
        check("desk-shortcut-menu", lang, theme, &img, &mut problems);
        choose(&mut h, shortcut_item::RENAME);
        let img = screen(&mut h);
        check("desk-rename", lang, theme, &img, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
