//! Snapshot tests of every surface of the shell, the lock screen and the dialogs in both
//! themes and both languages (DESKTOP-SPEC §2.2c): each screen is composed like the
//! compositor does (ground, bar at the bottom, the popup at its place with its alpha) at
//! the boards' size and compared with its reference in `tests/snapshots`.
//! `ULTAR_UPDATE_SNAPSHOTS=1` writes the references; `ULTAR_SHOTS_DIR=dir` also writes
//! every picture there (side-by-side with the boards: tools/boards-side-by-side.py).

mod common;

use common::shots::*;
use common::*;
use ultar_bus::messages::auth::UserInfo;
use ultar_bus::messages::settings::SettingValue;
use ultar_bus::messages::sysdialog::{AskPermission, Authenticate, Permission};
use ultar_desktop::dialog::{self, Dialogs, Question};
use ultar_desktop::lock::{self, LockState, SignInError};
use ultar_desktop::model::time::LocalTime;
use ultar_desktop::model::{Open, QuickPage};
use ultar_desktop::shell::Outcome;
use ultar_desktop::view::Look;
use ultar_desktop::view::Target;
use ultar_i18n::Lang;
use ultar_ui::geom::Size;
use ultar_ui::snapshot::Image;
use ultar_ui::tokens::ThemeKind;

fn blank(theme: ThemeKind) -> Image {
    let g = ground(theme);
    Image {
        width: SCREEN.w as u32,
        height: SCREEN.h as u32,
        rgba: g.repeat((SCREEN.w * SCREEN.h) as usize),
    }
}

fn desktop(h: &mut Harness) -> Image {
    let mut img = blank(h.look.theme);
    // the desktop background (company logo in the middle of the work area) under everything
    let mut background = ultar_desktop::view::wallpaper::build(
        &h.shell.d,
        h.look,
        SCREEN,
        ultar_desktop::model::Shape::default(),
        &h.shell.tr,
    )
    .unwrap();
    over(&mut img, &mut background, SCREEN, (0, 0));
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

fn open(h: &mut Harness, open: Open) {
    h.shell.d.open = open;
    h.apply(Outcome::surfaces());
}

fn shell_screens(lang: Lang, theme: ThemeKind, problems: &mut Vec<String>) {
    let cases: [(&str, Open); 9] = [
        ("desktop", Open::None),
        ("quick", Open::Quick(QuickPage::Tiles)),
        ("quick-modes", Open::Quick(QuickPage::PowerModes)),
        ("calendar", Open::Calendar),
        ("overview", Open::Overview),
        ("apps", Open::Apps(None)),
        ("projection", Open::Projection(0)),
        ("menu-keyboard", Open::KeyboardMenu),
        ("menu-tile", Open::TileMenu(0)),
    ];
    for (name, o) in cases {
        let mut h = Harness::new(lang, theme);
        if name == "calendar" {
            // Board CalendarSheet lists notifications (their banners are not drawn here).
            board_notes(&mut h.shell.d, lang);
        }
        // The bar's menus as a click opens them: at their button.
        match o {
            Open::KeyboardMenu => h.click_bar(Target::Keyboard),
            Open::TileMenu(i) => h.right_bar(Target::App(i)),
            _ => open(&mut h, o),
        }
        let img = desktop(&mut h);
        check(name, lang, theme, &img, problems);
    }
    // Toasts board: three banners over the desktop.
    let mut h = Harness::new(lang, theme);
    board_notes(&mut h.shell.d, lang);
    let mut img = desktop(&mut h);
    let tr = ultar_desktop::i18n::translator(lang).unwrap();
    let (mut v, place) =
        ultar_desktop::view::banners::build(&h.shell.d, (BOARD_MS, 0), h.look, SCREEN, &tr)
            .unwrap()
            .unwrap();
    let bar_top = (SCREEN.h - 56.0) as i32;
    over(
        &mut img,
        &mut v,
        place.surface,
        (place.offset.0, bar_top + place.offset.1),
    );
    check("banners", lang, theme, &img, problems);
    // AppGrid board: the context menu of the browser open.
    let mut h = Harness::new(lang, theme);
    open(&mut h, Open::Apps(None));
    h.right_popup(Target::AppItem("org.ultar.browser".into()));
    let img = desktop(&mut h);
    check("apps-menu", lang, theme, &img, problems);
}

fn user() -> UserInfo {
    UserInfo {
        name: "anas".into(),
        display_name: "أنس".into(),
        has_pin: true,
        admin: true,
    }
}

fn lock_screens(lang: Lang, theme: ThemeKind, problems: &mut Vec<String>) {
    let tr = ultar_desktop::i18n::translator(lang).unwrap();
    let look = Look::new(lang, theme);
    let t = LocalTime::at(BOARD_MS, 0);
    let mut s = LockState {
        user: Some(user()),
        notes: vec![("org.ultar.phone".into(), 1), ("org.ultar.store".into(), 1)],
        ..LockState::default()
    };
    s.locked();
    let mut stages = vec![("lock", s.clone())];
    s.show_sign_in();
    stages.push(("lock-signin", s.clone()));
    // Board Login: two accounts, the session's one chosen.
    let mut login = s.clone();
    login.users = vec![
        user(),
        UserInfo {
            name: "layla".into(),
            display_name: "ليلى".into(),
            has_pin: false,
            admin: false,
        },
    ];
    login.layouts = (vec!["ar".into(), "en".into()], 0);
    stages.push(("login", login));
    s.error = Some(SignInError::Wrong);
    s.retry_until_ms = BOARD_MS + 4000;
    s.caps_lock = true;
    s.layouts = (vec!["ar".into(), "en".into()], 0);
    stages.push(("lock-wait", s.clone()));
    for (name, state) in stages {
        let mut v = lock::view::build(&state, (BOARD_MS, t), look, SCREEN, &tr)
            .unwrap()
            .unwrap();
        let mut img = blank(theme);
        over(&mut img, &mut v, SCREEN, (0, 0));
        check(name, lang, theme, &img, problems);
    }
}

fn dialog_screens(lang: Lang, theme: ThemeKind, problems: &mut Vec<String>) {
    let tr = ultar_desktop::i18n::translator(lang).unwrap();
    let look = Look::new(lang, theme);
    let questions = [
        (
            "dialog-permission",
            Question::Permission(AskPermission {
                app_id: "org.ultar.camera".into(),
                app_name: "Camera".into(),
                permission: Permission::Camera,
                reason: None,
            }),
        ),
        (
            "dialog-auth",
            Question::Authenticate(Authenticate {
                app_id: "org.ultar.settings".into(),
                app_name: "Settings".into(),
                action: ultar_bus::Text::key("desk.kbd.settings"),
            }),
        ),
        (
            "dialog-handoff",
            Question::Handoff(ultar_bus::messages::mesh_dialog::AskHandoff {
                from_name: "KM One".into(),
                from_kind: ultar_bus::messages::mesh::DeviceKind::Phone,
                title: "notes.txt".into(),
                app: "org.ultar.editor".into(),
                size: Some(1_258_291),
                stage: ultar_bus::messages::mesh::Stage::State,
            }),
        ),
        (
            "dialog-keep",
            Question::Keep {
                key: "display.mode".into(),
                value: SettingValue::Text("extend".into()),
            },
        ),
    ];
    for (name, q) in questions {
        let mut d = Dialogs {
            admin: Some(user()),
            ..Dialogs::default()
        };
        d.ask(1, q, 0);
        let mut v = dialog::view::build(&d, 0, look, &tr).unwrap().unwrap();
        let size = v.ui.size();
        let mut img = blank(theme);
        let x = ((SCREEN.w - size.w) / 2.0) as i32;
        over(&mut img, &mut v, size, (x, 300));
        check(name, lang, theme, &img, problems);
    }
}

#[test]
fn every_surface_in_both_themes_and_both_languages_matches_its_reference() {
    let mut problems = Vec::new();
    for (lang, theme) in VARIANTS {
        shell_screens(lang, theme, &mut problems);
        lock_screens(lang, theme, &mut problems);
        dialog_screens(lang, theme, &mut problems);
    }
    assert!(problems.is_empty(), "{problems:#?}");
}
