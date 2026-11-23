//! Tests of the typography added in wave 3: caret inside ligatures, the shared baseline of
//! mixed runs, and Arabic ink inside its line box.

use super::*;
use crate::geom::Rect;

fn engine() -> TextEngine {
    TextEngine::new().unwrap()
}

fn style(lang: Lang) -> TextStyle {
    TextStyle::new(16.0, lang)
}

fn ar() -> TextStyle {
    style(Lang::Ar)
}

fn en() -> TextStyle {
    style(Lang::En)
}

/// Number of distinct clusters of a layout.
fn clusters(layout: &TextLayout) -> usize {
    let mut all = layout.visual_clusters();
    all.sort_unstable();
    all.dedup();
    all.len()
}

#[test]
fn caret_inside_a_ligature_divides_its_advance() {
    let mut e = engine();
    // Naskh draws the vocalised word Allah as one ligature glyph: four letters (alef, lam,
    // lam, heh) and two marks (shadda, superscript alef) in one cluster.
    let word = "اللّٰه";
    let layout = e.layout(word, &ar(), None);
    assert_eq!(clusters(&layout), 1);
    let w = layout.width;
    let at = |byte: usize| layout.caret_x(byte);
    assert_eq!((at(0), at(12)), (w, 0.0));
    assert!((at(2) - 0.75 * w).abs() < 0.01 && (at(4) - 0.5 * w).abs() < 0.01);
    assert!((at(6) - 0.25 * w).abs() < 0.01, "{}", at(6));
    // Bytes: alef 0, lam 2, lam 4, shadda 6, superscript alef 8, heh 10.
    assert_eq!((at(8), at(10)), (at(6), at(6)), "marks are no caret stops");
    for x in 0..=(w.ceil() as usize) {
        let index = layout.index_at_x(x as f32);
        assert!(index != 6 && index != 8, "x={x}: {index}");
    }
    assert_eq!(layout.index_at_x(0.45 * w), 4);
    // Latin ligature-free text keeps one position per character.
    let latin = e.layout("fi", &en(), None);
    assert!(latin.caret_x(0) < latin.caret_x(1) && latin.caret_x(1) < latin.caret_x(2));
}

#[test]
fn one_baseline_for_arabic_latin_and_mixed_text() {
    let mut e = engine();
    for role in [FontRole::Text, FontRole::Title, FontRole::Mono] {
        for weight in [Weight::Regular, Weight::Medium, Weight::Bold] {
            let style = ar().with_role(role).with_weight(weight);
            let baselines: Vec<f32> = ["سلام", "Hello", "سلام Hello 123", "", "١٢٣ (ص)"]
                .iter()
                .map(|text| e.layout(text, &style, None).lines[0].baseline)
                .collect();
            assert!(
                baselines.windows(2).all(|p| p[0] == p[1]),
                "{role:?} {weight:?}: {baselines:?}"
            );
        }
    }
    // The Arabic line metrics lift the baseline to make room for the deeper descenders;
    // in a Latin style the Latin face alone decides (unchanged from before the Arabic faces).
    let arabic = e.layout("x", &ar(), None).lines[0].baseline;
    let mut latin_metrics = ar();
    latin_metrics.arabic_metrics = false;
    let latin = e.layout("x", &latin_metrics, None).lines[0].baseline;
    assert!(arabic < latin, "{arabic} < {latin}");
    // Mixed runs of one line share the pen line: no glyph of a base letter is shifted.
    let mixed = e.layout("سلام Hello 123", &ar(), None);
    let fonts: std::collections::BTreeSet<_> =
        mixed.glyphs.iter().map(|g| g.font as usize).collect();
    assert_eq!(fonts.len(), 2, "Arabic and Latin face");
    assert!(mixed
        .glyphs
        .iter()
        .filter(|g| g.advance > 0.0)
        .all(|g| g.dy == 0.0));
}

#[test]
fn arabic_ink_stays_inside_its_line_box() {
    use crate::canvas::{Canvas, PixelFormat};
    use crate::tokens::{ColorRole, DARK};
    let mut e = engine();
    // Deep descenders (ي ج ع), tall letters (أ ل ك) and stacked marks.
    let text = "يجعل أكلٌ بِّ";
    for role in [FontRole::Text, FontRole::Title, FontRole::Mono] {
        for size in [12.0f32, 14.0, 16.0, 20.0, 24.0, 32.0] {
            let style = TextStyle::new(size, Lang::Ar)
                .with_role(role)
                .with_weight(Weight::Bold);
            let layout = e.layout(text, &style, None);
            let pad = 40;
            let (w, h) = (
                layout.width.ceil() as i32 + 2 * pad,
                layout.height as i32 + 2 * pad,
            );
            let mut canvas = Canvas::new(w as u32, h as u32, PixelFormat::Rgba8).unwrap();
            let area = Rect::new(pad as f32, pad as f32, layout.width.ceil(), layout.height);
            e.draw(
                &mut canvas,
                &layout,
                area,
                TextAlign::Start,
                DARK.get(ColorRole::Text),
            );
            let rows: Vec<i32> = (0..h)
                .filter(|y| (0..w).any(|x| canvas.pixel(x, *y).is_some_and(|c| c.a > 0)))
                .collect();
            let (top, bottom) = (rows[0] - pad, rows[rows.len() - 1] + 1 - pad);
            // One pixel of anti-aliasing overhang is tolerated.
            assert!(
                top >= -1 && bottom <= layout.height as i32 + 1,
                "{role:?} {size}: ink rows {top}..{bottom} of a {} high line",
                layout.height
            );
        }
    }
}
