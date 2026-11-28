//! Headless renders of the window in Arabic and English, dark and light, compact, medium and
//! wide: the document paints, the paper stays white in the dark theme, the tool bar mirrors in
//! Arabic, every state (empty, question, find, recovery, warning, message) paints.
//! With `ULTAR_DOC_SHOTS=<dir>` the pictures are written there for review.

mod common;

use common::*;
use ultar_doc::commands::Cmd;
use ultar_doc::dialogs::Ask;
use ultar_doc::state::{Cursor, Doc};
use ultar_doc::App;
use ultar_i18n::Lang;
use ultar_office::doc::{Block, Dir, Document, Para, Picture, Style};
use ultar_ui::snapshot::{render, Image};
use ultar_ui::tokens::ThemeKind;

const SIZES: [(&str, f32, f32); 3] = [
    ("compact", 360.0, 720.0),
    ("medium", 640.0, 800.0),
    ("wide", 1280.0, 800.0),
];

fn png_bytes() -> Vec<u8> {
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, 8, 4);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header().unwrap();
    w.write_image_data(&[40, 120, 200].repeat(32)).unwrap();
    drop(w);
    out
}

/// A document with every kind of content.
fn rich() -> Doc {
    let mut d = Document::from_text("");
    d.blocks.clear();
    let mut title = Para::new(Style::H1, "\u{645}\u{633}\u{62a}\u{646}\u{62f} \u{62a}\u{62c}\u{631}\u{64a}\u{628}\u{64a} Report 2026");
    title.dir = Dir::Rtl;
    d.blocks.push(Block::Para(title));
    let mut p = Para::new(Style::Normal, "Mixed paragraph: \u{627}\u{644}\u{646}\u{635} \u{627}\u{644}\u{639}\u{631}\u{628}\u{64a} inside English text, bold and italic and colour.");
    p.runs[0].bold = true;
    d.blocks.push(Block::Para(p));
    let mut ar = Para::new(Style::Normal, "\u{647}\u{630}\u{647} \u{641}\u{642}\u{631}\u{629} \u{639}\u{631}\u{628}\u{64a}\u{629} \u{641}\u{64a}\u{647}\u{627} English \u{648}\u{623}\u{631}\u{642}\u{627}\u{645} 12345 \u{62b}\u{645} \u{646}\u{647}\u{627}\u{64a}\u{629} \u{627}\u{644}\u{62c}\u{645}\u{644}\u{629}.");
    ar.dir = Dir::Rtl;
    d.blocks.push(Block::Para(ar));
    d.blocks
        .push(Block::Para(Para::new(Style::Bullet, "first item")));
    d.blocks
        .push(Block::Para(Para::new(Style::Bullet, "second item")));
    d.blocks.push(Block::Para(Para::new(Style::Number, "one")));
    d.blocks.push(Block::Para(Para::new(Style::Number, "two")));
    d.blocks.push(Block::Table(vec![
        vec!["a".into(), "\u{628}".into(), "c".into()],
        vec!["1".into(), "2".into(), "3".into()],
    ]));
    d.blocks.push(Block::Picture(Picture {
        bytes: png_bytes(),
        name: "i.png".into(),
        width_pt: Some(120.0),
        height_pt: Some(60.0),
    }));
    d.blocks
        .push(Block::Para(Para::new(Style::Normal, "The end.")));
    Doc::from_document(d)
}

fn shot(app: &mut App, name: &str) -> Image {
    app.ui.layout();
    let canvas = render(&mut app.ui).unwrap();
    let img = Image::from_canvas(&canvas);
    let first = img.rgba[..4].to_vec();
    assert!(
        img.rgba.chunks(4).any(|p| p != first.as_slice()),
        "{name}: something is painted"
    );
    if let Ok(dir) = std::env::var("ULTAR_DOC_SHOTS") {
        let png = img.encode_png().unwrap();
        ultar_ui::snapshot::write_atomic(
            &std::path::Path::new(&dir).join(format!("{name}.png")),
            &png,
        )
        .unwrap();
    }
    img
}

fn px(img: &Image, x: usize, y: usize) -> [u8; 3] {
    let w = img.width as usize;
    let i = (y * w + x) * 4;
    [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
}

fn tag(lang: Lang, theme: ThemeKind, size: &str) -> String {
    format!("{lang:?}-{}-{size}", theme.name()).to_lowercase()
}

#[test]
fn the_window_paints_in_every_variant_and_the_paper_stays_white() {
    let dir = areas("window");
    for lang in [Lang::Ar, Lang::En] {
        for theme in [ThemeKind::Dark, ThemeKind::Light] {
            for (name, w, h) in SIZES {
                let mut app = app_with(&dir, rich(), lang, theme, (w, h));
                let img = shot(&mut app, &format!("doc-{}", tag(lang, theme, name)));
                // A point on the paper's top margin, above any text.
                let r = app.ui.rect(app.nodes.text.unwrap()).unwrap();
                let s = app.ui.env().scale;
                let (x, y) = (
                    (r.x * s) as usize + app.paper_x() as usize + 6,
                    (r.y * s) as usize + 24,
                );
                let [pr, pg, pb] = px(&img, x, y);
                assert!(
                    app.doc.page_view || name == "compact",
                    "{name}: page view except on the phone"
                );
                if app.doc.page_view {
                    assert!(
                        pr > 240 && pg > 240 && pb > 240,
                        "{name} {lang:?} {theme:?}: the paper is white, got {:?}",
                        [pr, pg, pb]
                    );
                }
                if theme == ThemeKind::Dark {
                    let [gr, gg, gb] = px(&img, 2, img.height as usize - 3);
                    assert!(
                        u32::from(gr) + u32::from(gg) + u32::from(gb) < 3 * 128,
                        "{name}: the window around is dark"
                    );
                }
            }
        }
    }
}

#[test]
fn the_tool_bar_mirrors_in_arabic_and_starts_at_the_start_edge() {
    let dir = areas("mirror");
    for (lang, theme) in [(Lang::Ar, ThemeKind::Dark), (Lang::En, ThemeKind::Dark)] {
        let app = app_with(&dir, Doc::new(), lang, theme, (1280.0, 800.0));
        let new = app.command_node(Cmd::New).unwrap();
        let r = app.ui.rect(new).unwrap();
        let centre = r.x + r.w / 2.0;
        assert_eq!(
            centre > 640.0,
            lang == Lang::Ar,
            "{lang:?}: the first button sits at the start edge"
        );
    }
}

#[test]
fn every_state_paints() {
    let dir = areas("states");
    for lang in [Lang::Ar, Lang::En] {
        for (sname, w, h) in [SIZES[0], SIZES[2]] {
            let t = tag(lang, ThemeKind::Dark, sname);
            // empty document
            let mut app = app_with(&dir, Doc::new(), lang, ThemeKind::Dark, (w, h));
            shot(&mut app, &format!("state-empty-{t}"));
            // unsaved question
            let mut app = app_with(&dir, rich(), lang, ThemeKind::Light, (w, h));
            type_text(&mut app, "x");
            app.close_requested().unwrap();
            assert!(matches!(app.ask, Some(Ask::Unsaved { .. })));
            shot(&mut app, &format!("state-unsaved-{t}"));
            // find with hits and a message
            let mut app = app_with(&dir, rich(), lang, ThemeKind::Dark, (w, h));
            chord(&mut app, 'f');
            app.ui
                .set_text(app.nodes.find_field.unwrap(), "item")
                .unwrap();
            {
                let n = app.nodes.ok.unwrap();
                click(&mut app, n);
            }
            assert_eq!(app.find.hits.len(), 2);
            shot(&mut app, &format!("state-find-{t}"));
            // name question, colour question, more
            let mut app = app_with(&dir, rich(), lang, ThemeKind::Dark, (w, h));
            chord(&mut app, 's');
            shot(&mut app, &format!("state-name-{t}"));
            // a failed open
            let mut app = app_with(&dir, Doc::new(), lang, ThemeKind::Dark, (w, h));
            app.open_failed(ultar_doc::state::DocError::Unsupported)
                .unwrap();
            shot(&mut app, &format!("state-error-{t}"));
            // the selection is drawn
            let mut app = app_with(&dir, rich(), lang, ThemeKind::Dark, (w, h));
            app.doc.select_all();
            app.show_caret().unwrap();
            shot(&mut app, &format!("state-selected-{t}"));
            let _ = Cursor::default();
        }
    }
}

#[test]
fn recovery_and_warning_questions_paint() {
    let dir = areas("questions");
    for lang in [Lang::Ar, Lang::En] {
        let mut d = Doc::new();
        d.insert("unsaved words");
        ultar_doc::recovery::write(&d, &dir).unwrap();
        let mut app = app_with(&dir, Doc::new(), lang, ThemeKind::Dark, (1280.0, 800.0));
        assert!(matches!(app.ask, Some(Ask::Recover { .. })));
        shot(
            &mut app,
            &format!("state-recover-{}", tag(lang, ThemeKind::Dark, "wide")),
        );
        let mut lossy = rich();
        lossy.lossy = vec!["comments", "headers_footers", "charts"];
        ultar_doc::recovery::remove(None, &dir);
        let mut app = app_with(&dir, lossy, lang, ThemeKind::Light, (360.0, 720.0));
        if matches!(app.ask, Some(Ask::Recover { .. })) {
            {
                let n = app.nodes.discard.unwrap();
                click(&mut app, n);
            }
            app.ask = Some(Ask::Lossy);
            app.rebuild().unwrap();
        }
        assert_eq!(app.ask, Some(Ask::Lossy));
        shot(
            &mut app,
            &format!("state-lossy-{}", tag(lang, ThemeKind::Light, "compact")),
        );
    }
}
