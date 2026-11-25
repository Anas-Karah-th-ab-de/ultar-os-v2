//! Every screen in Arabic and English, dark and light: it builds, it fits the phone, no key
//! stays untranslated, Arabic runs from the right, and the ground is the theme's.

mod common;

use common::*;
use ultar_mobile::click::Click;
use ultar_mobile::model::Screen;
use ultar_ui::snapshot::render;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};
use ultar_ui::Lang;

const SCREENS: [Screen; 5] = [
    Screen::Lock,
    Screen::Home,
    Screen::Quick,
    Screen::Recents,
    Screen::Search,
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
    let px = canvas.pixel(1, size.h as i32 / 2).unwrap();
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
        m.go(screen);
        let mut a = app(Lang::De, ThemeKind::Dark, m);
        let name = format!("{screen:?}-de-dark").to_lowercase();
        dump(&mut a, &name);
        check(&mut a, Lang::De, ThemeKind::Dark, &name);
    }
}
