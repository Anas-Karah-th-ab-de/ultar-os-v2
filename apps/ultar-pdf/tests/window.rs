//! The reader window rendered headless: Arabic and English, dark and light, the three layout
//! classes, and every designed state. Checks what must hold on every screen: the page stays
//! white, only the interface mirrors in Arabic, and nothing sticks out of the window.

mod common;

use common::*;
use std::time::Duration;
use ultar_i18n::Lang;
use ultar_pdf::app::App;
use ultar_pdf::doc::DocState;
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::{SizeClass, ThemeKind};

const LIMIT: Duration = Duration::from_secs(30);
const SCREENS: [((f32, f32), SizeClass); 3] = [
    (COMPACT, SizeClass::Compact),
    (MEDIUM, SizeClass::Medium),
    (WIDE, SizeClass::Wide),
];

fn shot(app: &mut App) -> Image {
    let canvas = render(&mut app.ui).expect("render");
    Image::from_canvas(&canvas)
}

/// Share of the pixels in `area` (logical rect) that are (nearly) white.
fn white_share(img: &Image, area: ultar_ui::geom::Rect) -> f32 {
    let (mut white, mut total) = (0usize, 0usize);
    let x1 = ((area.x + area.w) as u32).min(img.width);
    let y1 = ((area.y + area.h) as u32).min(img.height);
    for y in (area.y.max(0.0) as u32..y1).step_by(2) {
        for x in (area.x.max(0.0) as u32..x1).step_by(2) {
            let i = ((y * img.width + x) * 4) as usize;
            let px = &img.rgba[i..i + 3];
            total += 1;
            if px.iter().all(|c| *c >= 245) {
                white += 1;
            }
        }
    }
    white as f32 / total.max(1) as f32
}

fn dark_share(img: &Image) -> f32 {
    let n = img
        .rgba
        .chunks(4)
        .filter(|p| p[..3].iter().all(|c| *c < 80))
        .count();
    n as f32 / (img.rgba.len() / 4) as f32
}

#[test]
fn the_page_is_white_in_every_language_theme_and_class() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (size, class) in SCREENS {
                let a = areas("shots", &["mixed.pdf"]);
                let mut app = reader(Some(DOC), &a, lang, theme, size);
                open_and_show(&mut app);
                assert_eq!(app.size_class(), class);
                let page = app.page_area().expect("page area");
                let img = shot(&mut app);
                let share = white_share(&img, page);
                assert!(
                    share > 0.5,
                    "{lang:?} {theme:?} {class:?}: only {share} of the page area is white"
                );
                let right = page.x + page.w;
                let bottom = page.y + page.h;
                assert!(
                    right <= size.0 + 0.5 && bottom <= size.1 + 0.5,
                    "page area sticks out: {page:?}"
                );
                if theme == ThemeKind::Dark {
                    assert!(
                        dark_share(&img) > 0.01,
                        "{lang:?} {class:?}: the interface is not dark"
                    );
                }
            }
        }
    }
}

#[test]
fn only_the_interface_mirrors_in_arabic_the_panel_sits_on_the_start_side() {
    let a = areas("mirror", &["mixed.pdf"]);
    let mut ar = reader(Some(DOC), &a, Lang::Ar, ThemeKind::Dark, WIDE);
    open_and_show(&mut ar);
    let (panel, page) = (
        ar.panel_area().expect("panel"),
        ar.page_area().expect("page"),
    );
    assert!(
        panel.x > page.x,
        "Arabic: the panel is on the right, {panel:?} vs {page:?}"
    );
    let mut en = reader(Some(DOC), &a, Lang::En, ThemeKind::Dark, WIDE);
    open_and_show(&mut en);
    let (panel, page) = (
        en.panel_area().expect("panel"),
        en.page_area().expect("page"),
    );
    assert!(
        panel.x < page.x,
        "English: the panel is on the left, {panel:?} vs {page:?}"
    );
    // the page itself is not mirrored: the Latin first line starts on the left of the page
    assert!(pump_until(&mut en, LIMIT, |x| x.doc.text(0).is_some()));
    let first = en
        .doc
        .text(0)
        .and_then(|t| t.words.first().map(|w| w.x0))
        .expect("word");
    assert!(
        first < 100.0,
        "the first word of the page is at its left edge, {first}"
    );
}

#[test]
fn the_panel_exists_only_on_wide_windows() {
    for (size, class) in SCREENS {
        let a = areas("panel", &["mixed.pdf"]);
        let mut app = reader(Some(DOC), &a, Lang::En, ThemeKind::Light, size);
        open_and_show(&mut app);
        assert_eq!(
            app.panel_area().is_some(),
            class == SizeClass::Wide,
            "{class:?}"
        );
    }
}

#[test]
fn the_search_bar_fits_every_class() {
    for lang in [Lang::Ar, Lang::En] {
        for (size, _) in SCREENS {
            let a = areas("bar", &["mixed.pdf"]);
            let mut app = reader(Some(DOC), &a, lang, ThemeKind::Dark, size);
            open_and_show(&mut app);
            ctrl(&mut app, 'f');
            type_text(&mut app, "fox");
            assert!(pump_until(&mut app, LIMIT, |x| x.search_done()));
            let field = app.search_area().expect("field");
            assert!(
                field.x >= -0.5 && field.x + field.w <= size.0 + 0.5,
                "{lang:?} {size:?}: {field:?}"
            );
            shot(&mut app);
        }
    }
}

#[test]
fn every_designed_state_renders_on_every_screen() {
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (size, _) in SCREENS {
                let a = areas("states", &["mixed.pdf", "locked.pdf", "broken.pdf"]);
                // no file
                let mut app = reader(None, &a, lang, theme, size);
                assert_eq!(app.doc.state, DocState::NoFile);
                shot(&mut app);
                // missing file
                let mut app = reader(Some("files://documents/gone.pdf"), &a, lang, theme, size);
                assert!(pump_until(&mut app, LIMIT, |x| x.doc.state != DocState::Opening));
                shot(&mut app);
                // loading is the first frame of a real file
                let mut app = reader(Some(DOC), &a, lang, theme, size);
                shot(&mut app);
                // damaged
                let mut app = reader(Some("files://documents/broken.pdf"), &a, lang, theme, size);
                assert!(pump_until(&mut app, LIMIT, |x| x.doc.state == DocState::Damaged));
                shot(&mut app);
                // password
                let mut app = reader(Some("files://documents/locked.pdf"), &a, lang, theme, size);
                assert!(pump_until(&mut app, LIMIT, |x| x.doc.state == DocState::NeedsPassword));
                shot(&mut app);
                // the open dialog
                let mut app = reader(None, &a, lang, theme, size);
                ctrl(&mut app, 'o');
                assert!(app.asking());
                shot(&mut app);
                // empty search
                let mut app = reader(Some(DOC), &a, lang, theme, size);
                open_and_show(&mut app);
                ctrl(&mut app, 'f');
                type_text(&mut app, "zzzqqq");
                assert!(pump_until(&mut app, LIMIT, |x| x.search_done()));
                shot(&mut app);
            }
        }
    }
}

#[test]
fn the_states_differ_from_each_other_on_screen() {
    let a = areas("differ", &["mixed.pdf", "broken.pdf"]);
    let mut none = reader(None, &a, Lang::En, ThemeKind::Dark, MEDIUM);
    let mut bad = reader(
        Some("files://documents/broken.pdf"),
        &a,
        Lang::En,
        ThemeKind::Dark,
        MEDIUM,
    );
    assert!(pump_until(&mut bad, LIMIT, |x| x.doc.state == DocState::Damaged));
    let (i1, i2) = (shot(&mut none), shot(&mut bad));
    assert_ne!(i1.rgba, i2.rgba, "no-file and damaged look the same");
}
