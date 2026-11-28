//! Every screen in Arabic and English, dark and light: it builds, it fits the phone, no key
//! stays untranslated, Arabic runs from the right, and the ground is the theme's.

mod common;

use common::*;
use ultar_mobile::click::Click;
use ultar_mobile::model::Screen;
use ultar_ui::snapshot::render;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};
use ultar_ui::Lang;

const SCREENS: [Screen; 6] = [
    Screen::Lock,
    Screen::Home,
    Screen::Quick,
    Screen::Recents,
    Screen::Search,
    Screen::Mesh,
];

fn check(a: &mut ultar_mobile::MobileApp, lang: Lang, theme: ThemeKind, name: &str) {
    let mut problems = Vec::new();
    check_into(a, lang, theme, name, &mut problems);
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

fn check_into(
    a: &mut ultar_mobile::MobileApp,
    lang: Lang,
    theme: ThemeKind,
    name: &str,
    problems: &mut Vec<String>,
) {
    let tree = a.ui().a11y_tree();
    for node in &tree {
        let text = node.describe(a.ui().translator());
        if text.contains("mobile.") {
            problems.push(format!("{name}: untranslated {text}"));
        }
    }
    let size = a.ui().size();
    for node in &tree {
        let r = node.rect;
        if !(r.x >= -0.5 && r.y >= -0.5 && r.right() <= size.w + 0.5) {
            let text = node.describe(a.ui().translator());
            problems.push(format!(
                "{name}: {:?} {text:?} outside the screen: {r:?}",
                node.role
            ));
        }
    }
    let canvas = render(a.ui()).unwrap();
    let ground = match theme {
        ThemeKind::Dark => DARK.get(ColorRole::Ground),
        ThemeKind::Light => LIGHT.get(ColorRole::Ground),
    };
    // The choice of a device dims the page behind its sheet (board CompactMeshSend): there
    // the ground shows beside the back arrow, above the dimmed part.
    let row = if name.starts_with("mesh") {
        44
    } else {
        size.h as i32 / 2
    };
    let px = canvas.pixel(1, row).unwrap();
    if (px.r, px.g, px.b) != (ground.r, ground.g, ground.b) {
        problems.push(format!("{name}: ground"));
    }
    // The navigation bar's "recent apps" sits at the start side: right in Arabic.
    if let Some(id) = a.built().node(&Click::Recents) {
        let x = a.ui().rect(id).unwrap().center().x;
        if (x > size.w / 2.0) != (lang == Lang::Ar) {
            problems.push(format!("{name}: navigation side"));
        }
    }
}

#[test]
fn every_screen_in_both_languages_and_themes() {
    let mut problems = Vec::new();
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for screen in SCREENS {
                let mut m = sample(screen == Screen::Lock);
                m.go(screen);
                if screen == Screen::Search {
                    m.query = "set".into();
                }
                if screen == Screen::Mesh {
                    m.mesh.sent(
                        "org.ultar.files",
                        ultar_bus::messages::mesh::SendOutcome::Choose(targets()),
                        0,
                    );
                    m.go(Screen::Mesh);
                }
                let mut a = app(lang, theme, m);
                let name = format!("{screen:?}-{lang:?}-{theme:?}").to_lowercase();
                dump(&mut a, &name);
                check_into(&mut a, lang, theme, &name, &mut problems);
            }
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn german_builds_too() {
    for screen in SCREENS {
        let mut m = sample(screen == Screen::Lock);
        if screen == Screen::Mesh {
            m.mesh.sent(
                "org.ultar.files",
                ultar_bus::messages::mesh::SendOutcome::Choose(targets()),
                0,
            );
        }
        m.go(screen);
        let mut a = app(Lang::De, ThemeKind::Dark, m);
        let name = format!("{screen:?}-de-dark").to_lowercase();
        dump(&mut a, &name);
        check(&mut a, Lang::De, ThemeKind::Dark, &name);
    }
}

/// The phone lock program's screens: cover without camera and emergency, sign-in plain, with
/// a wrong password, while waiting and with PIN – in both languages.
#[test]
fn the_lock_program_screens() {
    use ultar_bus::messages::auth::Method;
    use ultar_lock_core::{LockState, SignInError, Stage};
    use ultar_mobile::view::{build_lock, lock::Cover};
    let user = anas();
    let apps = ultar_mobile::model::apps::system_apps();
    let mut problems = Vec::new();
    for lang in [Lang::Ar, Lang::En] {
        let states = [
            ("cover", Stage::Cover, None, 0, Method::Password),
            ("signin", Stage::SignIn, None, 0, Method::Password),
            (
                "wrong",
                Stage::SignIn,
                Some(SignInError::Wrong),
                0,
                Method::Password,
            ),
            (
                "wait",
                Stage::SignIn,
                Some(SignInError::Wrong),
                5_000,
                Method::Password,
            ),
            ("pin", Stage::SignIn, None, 0, Method::Pin),
        ];
        for (name, stage, error, retry, method) in states {
            let state = LockState {
                stage,
                error,
                retry_until_ms: retry,
                method,
                user: Some(user.clone()),
                ..LockState::default()
            };
            let cover = Cover {
                now: ultar_mobile::model::time::LocalTime::at(SAMPLE_MS, 0),
                counts: vec![("org.ultar.phone".into(), 2)],
                torch: None,
                apps: &apps,
                camera: false,
                emergency: false,
            };
            let m = sample(true);
            let mut a = app(lang, ThemeKind::Dark, m.clone());
            let built = build_lock(a.ui(), &state, &cover, &m, 0).unwrap();
            let shown = built.password.is_some();
            assert_eq!(shown, stage == Stage::SignIn, "{name}");
            let label = format!("lockprog-{name}-{lang:?}").to_lowercase();
            dump(&mut a, &label);
            check_into(&mut a, lang, ThemeKind::Dark, &label, &mut problems);
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

/// The account of the lock screen tests (with a PIN).
fn anas() -> ultar_bus::messages::auth::UserInfo {
    ultar_bus::messages::auth::UserInfo {
        name: "anas".into(),
        display_name: "أنس".into(),
        has_pin: true,
        admin: true,
    }
}
