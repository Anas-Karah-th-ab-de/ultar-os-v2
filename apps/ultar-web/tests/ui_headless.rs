//! The window headless in Arabic and English, dark and light: mirrored layout, theme colours,
//! the page's pixels in the window, accessible names and roles, every text translated.
//! With `ULTAR_CONFORMANCE_OUT` set the four pictures are written there.

mod common;

use common::*;
use ultar_ui::color::Color;
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK, LIGHT};
use ultar_ui::ui::Role;
use ultar_ui::Lang;
use ultar_web::engine::test_engine::page_colour;

const CASES: [(Lang, ThemeKind); 4] = [
    (Lang::Ar, ThemeKind::Dark),
    (Lang::Ar, ThemeKind::Light),
    (Lang::En, ThemeKind::Dark),
    (Lang::En, ThemeKind::Light),
];

fn name(lang: Lang, theme: ThemeKind) -> String {
    format!(
        "browser-{}-{}",
        if lang == Lang::Ar { "ar" } else { "en" },
        if theme == ThemeKind::Dark {
            "dark"
        } else {
            "light"
        }
    )
}

#[test]
fn window_in_both_languages_and_themes() {
    for (lang, theme) in CASES {
        let mut b = browser(lang, theme);
        let palette = if theme == ThemeKind::Dark {
            &DARK
        } else {
            &LIGHT
        };
        let canvas = ultar_ui::snapshot::render(b.ui_mut()).unwrap();
        if let Ok(dir) = std::env::var("ULTAR_CONFORMANCE_OUT") {
            let png = ultar_ui::snapshot::Image::from_canvas(&canvas)
                .encode_png()
                .unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                std::path::Path::new(&dir).join(format!("{}.png", name(lang, theme))),
                png,
            )
            .unwrap();
        }
        assert_eq!(
            canvas.pixel(400, 3),
            Some(palette.get(ColorRole::Raised)),
            "{lang:?} {theme:?} strip"
        );
        assert_eq!(
            canvas.pixel(60, 500),
            Some(palette.get(ColorRole::Surface)),
            "{lang:?} {theme:?} start page"
        );
        let back = b.ui().rect(b.chrome().back).unwrap();
        let downloads = b.ui().rect(b.chrome().downloads).unwrap();
        // Arabic mirrors the window: back at the right (start) edge, downloads at the left.
        if lang == Lang::Ar {
            assert!(back.x > CLIENT.w / 2.0 && downloads.x < CLIENT.w / 2.0);
            assert_eq!((CLIENT.w - back.right()).round(), 12.0);
        } else {
            assert_eq!(back.x.round(), 12.0);
        }
    }
}

#[test]
fn the_page_shows_the_engines_pixels() {
    for (lang, theme) in CASES {
        let mut b = browser(lang, theme);
        open(&mut b, "pixels.example");
        let page = b.ui().rect(b.chrome().page.unwrap()).unwrap();
        let canvas = ultar_ui::snapshot::render(b.ui_mut()).unwrap();
        let [blue, green, red, _] = page_colour("https://pixels.example/");
        let want = Color::rgb(red, green, blue);
        let (x, y) = (
            (page.x + page.w / 2.0) as i32,
            (page.y + page.h / 2.0) as i32,
        );
        assert_eq!(canvas.pixel(x, y), Some(want), "{}", name(lang, theme));
        assert_eq!(
            canvas.pixel(page.x as i32 + 1, page.bottom() as i32 - 1),
            Some(want)
        );
    }
}

#[test]
fn every_element_is_named_and_translated() {
    for lang in [Lang::Ar, Lang::En] {
        let mut b = browser(lang, ThemeKind::Dark);
        let n = b.chrome().new_tab;
        click(&mut b, n);
        open(&mut b, "named.example/title/Hello");
        let tree = b.ui_mut().a11y_tree();
        let tabs: Vec<_> = tree.iter().filter(|n| n.role == Role::Tab).collect();
        assert_eq!(tabs.len(), 2);
        assert_eq!(
            tabs.iter()
                .filter(|t| t.state.selected == Some(true))
                .count(),
            1
        );
        let tr = b.ui().translator();
        for node in &tree {
            if let Some(label) = &node.name {
                let text = label.resolve(tr);
                assert!(
                    !text.is_empty() && !text.starts_with("web."),
                    "untranslated {text:?}"
                );
            }
        }
        let close = b.ui().a11y_node(b.chrome().tabs[1].1).unwrap();
        let spoken = close.name.unwrap().resolve(tr).into_owned();
        assert!(spoken.contains("Hello"), "{spoken}");
        if lang == Lang::Ar {
            assert!(spoken.starts_with("إغلاق"), "{spoken}");
        }
    }
}
