//! Headless renders of every surface in dark/light and Arabic/English, wide and phone size.

mod common;

use common::*;
use ultar_bus::messages::call::Phase;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};
use ultar_ui::Lang;

fn dump(app: &mut App, name: &str) {
    if let Ok(dir) = std::env::var("ULTAR_PHONE_OUT") {
        let canvas = render(app.ui()).unwrap();
        let png = Image::from_canvas(&canvas).encode_png().unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(std::path::Path::new(&dir).join(format!("{name}.png")), png).unwrap();
    }
}

/// Every text is translated, the page ground is the theme's token, Arabic is mirrored.
fn check(a: &mut App, lang: Lang, theme: ThemeKind, name: &str) {
    for text in names(a) {
        assert!(!text.contains("phone."), "{name}: untranslated {text}");
    }
    let canvas = render(a.ui()).unwrap();
    let ground = match theme {
        ThemeKind::Dark => DARK.get(ColorRole::Ground),
        ThemeKind::Light => LIGHT.get(ColorRole::Ground),
    };
    let size = a.ui().size();
    let px = canvas.pixel(2, size.h as i32 - 2).unwrap();
    assert_eq!(
        (px.r, px.g, px.b),
        (ground.r, ground.g, ground.b),
        "{name}: page ground"
    );
    let copy = node(a, "account.copy");
    let rect = a.ui().rect(copy).unwrap();
    let at_right = rect.center().x > size.w / 2.0;
    // Wide: own number at the start (board); phone: title at the start, number at the end.
    let start = size.w >= 600.0;
    assert_eq!(
        at_right,
        (lang == Lang::Ar) == start,
        "{name}: own number side"
    );
}

#[test]
fn render_all_surfaces() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (size, sname) in [(WIDE, "wide"), (PHONE, "phone")] {
                for (state, s) in [
                    (ready(), "dial"),
                    (with_calls(vec![call(1, Phase::Ringing, false)]), "incoming"),
                    (with_calls(vec![call(1, Phase::Active, true)]), "incall"),
                ] {
                    let mut a = app_with(lang, theme, size, state);
                    let name = format!("{s}-{sname}-{lang:?}-{theme:?}");
                    dump(&mut a, &name);
                    check(&mut a, lang, theme, &name);
                }
            }
        }
    }
}
