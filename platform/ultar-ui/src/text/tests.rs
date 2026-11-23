use super::*;
use crate::canvas::PixelFormat;
use crate::tokens::{ColorRole, TypeSize, DARK};

const SIZE: f32 = TypeSize::S16.points();

fn engine() -> TextEngine {
    TextEngine::new().unwrap()
}

fn ar() -> TextStyle {
    TextStyle::new(SIZE, Lang::Ar)
}

fn en() -> TextStyle {
    TextStyle::new(SIZE, Lang::En)
}

/// Number of distinct clusters of a layout.
fn clusters(layout: &TextLayout) -> usize {
    let mut all = layout.visual_clusters();
    all.sort_unstable();
    all.dedup();
    all.len()
}

/// The first character of every glyph's cluster, in visual order from the left.
fn visual(layout: &TextLayout) -> String {
    layout
        .visual_clusters()
        .iter()
        .filter_map(|c| layout.text[*c as usize..].chars().next())
        .collect()
}

#[test]
fn arabic_word_is_shaped_not_isolated_forms() {
    let mut e = engine();
    let word = "بيت";
    let layout = e.layout(word, &ar(), None);
    // Noto Naskh Arabic composes dotted letters from a body and separate dot glyphs, so the
    // glyph count is a property of the font; the clusters are what the toolkit relies on.
    assert_eq!(clusters(&layout), 3);
    let nominal: Vec<u16> = word
        .chars()
        .map(|c| e.nominal_glyph(FontId::Arabic, c).unwrap())
        .collect();
    let shaped: Vec<u16> = layout.glyphs.iter().map(|g| g.id).collect();
    for id in &shaped {
        assert!(
            !nominal.contains(id),
            "glyph {id} is an isolated form: {shaped:?} vs {nominal:?}"
        );
    }
    assert!(layout
        .glyphs
        .iter()
        .all(|g| g.rtl && g.font == FontId::Arabic));
    // Visual order is reversed logical order: the first letter is the rightmost glyph.
    let mut order = layout.visual_clusters();
    order.dedup();
    assert_eq!(order, vec![4, 2, 0]);
}

#[test]
fn joined_word_is_narrower_than_its_isolated_letters() {
    let mut e = engine();
    let joined = e.layout("سلم", &ar(), None).width;
    let apart = e.layout("س\u{200C}ل\u{200C}م", &ar(), None).width;
    assert!(joined < apart, "{joined} < {apart}");
}

#[test]
fn lam_alef_is_joined_in_every_arabic_face() {
    let mut e = engine();
    // Every embedded Arabic face draws lam-alef with a pair of special forms (two clusters,
    // so the caret can stand between them).
    for role in [FontRole::Text, FontRole::Title, FontRole::Mono] {
        let style = ar().with_role(role);
        let layout = e.layout("سلام", &style, None);
        assert_eq!(clusters(&layout), 4, "{role:?}");
        let font = layout.glyphs[0].font;
        let lam = e.nominal_glyph(font, 'ل').unwrap();
        let alef = e.nominal_glyph(font, 'ا').unwrap();
        assert!(layout.glyphs.iter().all(|g| g.id != lam && g.id != alef));
        let plain_final_alef = e.layout("با", &style, None).glyphs[0].id;
        let after_lam = e.layout("لا", &style, None).glyphs[0].id;
        assert_ne!(
            plain_final_alef, after_lam,
            "{role:?}: alef after lam has its own form"
        );
    }
    // Noto Sans Arabic (fixed-width text) draws the pair the same way since 2.013.
    let mono = ar().with_role(FontRole::Mono);
    let layout = e.layout("سلام", &mono, None);
    assert_eq!(clusters(&layout), 4);
}

#[test]
fn tashkeel_is_positioned_on_its_base() {
    let mut e = engine();
    let layout = e.layout("بَ", &ar(), None);
    assert_eq!(
        clusters(&layout),
        1,
        "the mark belongs to its letter's cluster"
    );
    let fatha = e.nominal_glyph(FontId::Arabic, '\u{064E}').unwrap();
    let mark = layout
        .glyphs
        .iter()
        .find(|g| g.id == fatha)
        .expect("the fatha glyph");
    assert_eq!(mark.advance, 0.0);
    assert!(
        mark.dx != 0.0 || mark.dy != 0.0,
        "GPOS moved the mark: {mark:?}"
    );
    let base = e.layout("ب", &ar(), None);
    assert!(
        (layout.width - base.width).abs() < 0.01,
        "marks add no width"
    );
    // A mark below and a mark above the same base get different vertical offsets.
    let below = e.layout("بِ", &ar(), None);
    let below_mark = below.glyphs.iter().find(|g| g.advance == 0.0).unwrap();
    assert!(below_mark.id != mark.id);
}

#[test]
fn isolated_numbers_keep_ltr_order_inside_arabic() {
    let mut e = engine();
    for data in ["3840 × 2160", "60 Hz", "200%"] {
        let text = format!("الدقة {} جيدة", isolate(data));
        let shown = visual(&e.layout(&text, &ar(), None));
        assert!(
            shown.contains(data),
            "{data:?} not left-to-right in {shown:?}"
        );
        // The Arabic word that comes first logically ends up to the right of the data.
        assert!(shown.ends_with('ا') && shown.starts_with('ة'), "{shown:?}");
        assert!(
            shown.find('ج').unwrap() < shown.find(data).unwrap(),
            "{shown:?}"
        );
    }
}

#[test]
fn without_isolation_the_bidi_algorithm_reorders_number_groups() {
    // This is why data values are isolated: plain UAX #9 puts "2160" before "3840".
    let mut e = engine();
    let shown = visual(&e.layout("الدقة 3840 × 2160", &ar(), None));
    assert!(shown.contains("2160 × 3840"), "{shown:?}");
    let shown = visual(&e.layout(&isolate("3840 × 2160"), &ar(), None));
    assert_eq!(shown, "3840 × 2160");
}

#[test]
fn mixed_arabic_and_latin_runs() {
    let mut e = engine();
    let shown = visual(&e.layout("شبكة Wi-Fi متصلة", &ar(), None));
    assert!(shown.contains("Wi-Fi"), "{shown:?}");
    assert!(
        shown.starts_with('ة'),
        "last Arabic word is leftmost: {shown:?}"
    );
    assert!(
        shown.ends_with('ش'),
        "first Arabic letter is rightmost: {shown:?}"
    );
    // The same text in a left-to-right paragraph keeps the Latin word in the middle but
    // the Arabic words swap sides.
    let ltr = visual(&e.layout("شبكة Wi-Fi متصلة", &en(), None));
    assert!(
        ltr.contains("Wi-Fi") && ltr.starts_with('ة') && !ltr.ends_with('ش'),
        "{ltr:?}"
    );
    // Digits inside Arabic read left to right.
    let digits = visual(&e.layout("عدد 12345", &ar(), None));
    assert!(digits.starts_with("12345"), "{digits:?}");
    // Latin fonts are used for Latin and digits, the Arabic font for Arabic.
    let layout = e.layout("عدد 12 ab", &ar(), None);
    for g in &layout.glyphs {
        let ch = layout.text[g.cluster as usize..].chars().next().unwrap();
        if ch.is_ascii_alphanumeric() {
            assert_eq!(g.font, FontId::Latin);
        } else if !ch.is_whitespace() {
            assert_eq!(g.font, FontId::Arabic);
        }
    }
}

#[test]
fn measurement_and_line_metrics() {
    let mut e = engine();
    let one = e.layout("Settings", &en(), None);
    let two = e.layout("Settings Settings", &en(), None);
    assert!(one.width > 0.0 && two.width > 2.0 * one.width);
    assert_eq!(one.lines.len(), 1);
    assert_eq!(one.height, (SIZE * typography::LINE_HEIGHT_LATIN).round());
    let arabic = e.layout("الإعدادات", &ar(), None);
    assert_eq!(
        arabic.height,
        (SIZE * typography::LINE_HEIGHT_ARABIC).round()
    );
    assert!(arabic.lines[0].baseline > 0.0 && arabic.lines[0].baseline < arabic.height);
    let bold = e.layout("Settings", &en().with_weight(Weight::Bold), None);
    assert!(bold.width > one.width);
    assert!(bold.glyphs.iter().all(|g| g.font == FontId::LatinBold));
    let mono = e.layout("il", &en().with_role(FontRole::Mono), None);
    assert_eq!(mono.glyphs[0].advance, mono.glyphs[1].advance);
    let empty = e.layout("", &en(), None);
    assert_eq!((empty.lines.len(), empty.width), (1, 0.0));
    assert!(empty.height > 0.0);
    let lines = e.layout("a\nb\n", &en(), None);
    assert_eq!(lines.lines.len(), 2);
}

#[test]
fn wraps_at_words_and_keeps_every_line_inside() {
    let mut e = engine();
    let text = "اسحب الشاشات لتطابق أماكنها على مكتبك. اختر شاشة لتغيير إعداداتها.";
    let style = ar().with_overflow(Overflow::Wrap);
    let free = e.layout(text, &style, None);
    assert_eq!(free.lines.len(), 1);
    let max = free.width / 3.0;
    let wrapped = e.layout(text, &style, Some(max));
    assert!(wrapped.lines.len() >= 3, "{}", wrapped.lines.len());
    for line in &wrapped.lines {
        assert!(line.width <= max + 0.01, "{} > {max}", line.width);
        let content = &text[line.bytes.clone()];
        assert!(
            !content.starts_with(' ') && !content.ends_with(' '),
            "{content:?}"
        );
    }
    // No character is lost: lines are consecutive words.
    let joined: Vec<&str> = wrapped
        .lines
        .iter()
        .map(|l| &text[l.bytes.clone()])
        .collect();
    assert_eq!(joined.join(" "), text);
    assert_eq!(wrapped.height, wrapped.lines.len() as f32 * free.height);
    // Right-to-left lines align at the right edge of their box (start), left for End.
    let short = wrapped.lines.iter().find(|l| l.width < max - 1.0).unwrap();
    assert_eq!(
        wrapped.line_offset(short, max, TextAlign::Start),
        max - short.width
    );
    assert_eq!(wrapped.line_offset(short, max, TextAlign::End), 0.0);
    assert_eq!(
        wrapped.line_offset(short, max, TextAlign::Center),
        (max - short.width) / 2.0
    );
}

#[test]
fn a_word_wider_than_the_line_breaks_inside() {
    let mut e = engine();
    let style = en().with_overflow(Overflow::Wrap);
    let word = "Wwwwwwwwwwwwwwwwwwwwwwwwwwwwwwww";
    let max = e.layout("Wwwww", &style, None).width;
    let wrapped = e.layout(word, &style, Some(max));
    assert!(wrapped.lines.len() > 4);
    assert!(wrapped.lines.iter().all(|l| l.width <= max + 0.01));
    // A width smaller than one letter still terminates with one letter per line.
    let tiny = e.layout("abc", &style, Some(1.0));
    assert_eq!(tiny.lines.len(), 3);
}

#[test]
fn ellipsis_truncates_to_the_width() {
    let mut e = engine();
    for (text, base) in [
        ("تعود النوافذ إلى شاشتها عند إعادة توصيلها", ar()),
        ("Windows return to their screen", en()),
    ] {
        let style = base.with_overflow(Overflow::Ellipsis);
        let full = e.layout(text, &style, None);
        assert!(!full.truncated);
        let fits = e.layout(text, &style, Some(full.width + 1.0));
        assert!(!fits.truncated && fits.text == text);
        let max = full.width * 0.6;
        let cut = e.layout(text, &style, Some(max));
        assert!(cut.truncated);
        assert!(cut.width <= max, "{} <= {max}", cut.width);
        assert!(
            cut.width > max * 0.7,
            "not cut more than needed: {} of {max}",
            cut.width
        );
        assert!(cut.text.ends_with(ELLIPSIS));
        assert!(text.starts_with(cut.text.trim_end_matches(ELLIPSIS)));
        assert_eq!(cut.lines.len(), 1);
        // In right-to-left text the ellipsis is the leftmost glyph (the end side).
        let shown = visual(&cut);
        if style.direction.is_rtl() {
            assert!(shown.starts_with(ELLIPSIS), "{shown:?}");
        } else {
            assert!(shown.ends_with(ELLIPSIS), "{shown:?}");
        }
        let nothing = e.layout(text, &style, Some(0.5));
        assert_eq!(nothing.text, ELLIPSIS.to_string());
    }
}

#[test]
fn caret_positions_follow_the_visual_order() {
    let mut e = engine();
    let latin = e.layout("abc", &en(), None);
    assert_eq!(latin.caret_x(0), 0.0);
    assert!((latin.caret_x(3) - latin.width).abs() < 0.01);
    assert!(latin.caret_x(1) > 0.0 && latin.caret_x(1) < latin.caret_x(2));
    assert_eq!(latin.index_at_x(-5.0), 0);
    assert_eq!(latin.index_at_x(latin.width + 5.0), 3);
    assert_eq!(latin.index_at_x(latin.caret_x(2) + 0.2), 2);
    let arabic = e.layout("بيت", &ar(), None);
    assert!(
        (arabic.caret_x(0) - arabic.width).abs() < 0.01,
        "caret before the first letter is at the right"
    );
    assert_eq!(arabic.caret_x(6), 0.0);
    assert!(arabic.caret_x(2) < arabic.caret_x(0) && arabic.caret_x(2) > arabic.caret_x(4));
    assert_eq!(arabic.index_at_x(0.0), 6);
    assert_eq!(arabic.index_at_x(arabic.width), 0);
    let empty = e.layout("", &ar(), None);
    assert_eq!((empty.caret_x(0), empty.index_at_x(3.0)), (0.0, 0));
    // Trailing spaces move the caret (text fields).
    let spaced = e.layout("ab ", &en(), None);
    assert!(spaced.caret_x(3) > spaced.caret_x(2));
}

#[test]
fn layouts_are_cached_and_caches_stay_bounded() {
    let mut e = TextEngine::with_limits(4096, 4096).unwrap();
    let a = e.layout("مرحبا", &ar(), None);
    let b = e.layout("مرحبا", &ar(), None);
    assert!(Rc::ptr_eq(&a, &b));
    assert_eq!(e.layout_cache_stats().hits, 1);
    let other = e.layout("مرحبا", &ar().with_weight(Weight::Bold), Some(50.0));
    assert!(!Rc::ptr_eq(&a, &other));
    let mut canvas = Canvas::new(400, 40, PixelFormat::Rgba8).unwrap();
    let color = DARK.get(ColorRole::Text);
    let area = Rect::new(0.0, 0.0, 400.0, 40.0);
    for i in 0..400u32 {
        let text = format!("نص رقم {i} abc");
        let size = SIZE + (i % 7) as f32;
        let layout = e.layout(&text, &TextStyle::new(size, Lang::Ar), None);
        e.draw(&mut canvas, &layout, area, TextAlign::Start, color);
        assert!(
            e.glyph_cache_stats().cost <= 2 * 4096 + 4096,
            "{:?}",
            e.glyph_cache_stats()
        );
        assert!(
            e.layout_cache_stats().cost <= 2 * 4096 + 4096,
            "{:?}",
            e.layout_cache_stats()
        );
    }
    assert!(e.glyph_cache_stats().hits > 0);
    e.clear_caches();
    assert_eq!(
        e.glyph_cache_stats().entries + e.layout_cache_stats().entries,
        0
    );
}

#[test]
fn drawing_puts_ink_where_the_text_is() {
    let mut e = engine();
    let mut canvas = Canvas::new(200, 40, PixelFormat::Rgba8).unwrap();
    let color = DARK.get(ColorRole::Text);
    let layout = e.layout("سلام", &ar(), None);
    e.draw(
        &mut canvas,
        &layout,
        Rect::new(0.0, 0.0, 200.0, 40.0),
        TextAlign::Start,
        color,
    );
    let damage = canvas.damage().bounds();
    assert!(
        damage.x0 >= 200 - layout.width.ceil() as i32 - 2,
        "right-aligned: {damage:?}"
    );
    assert!(damage.x1 <= 200 && damage.x1 > 190);
    let ink = |c: &Canvas| c.data().chunks(4).filter(|p| p[3] > 0).count();
    let start_ink = ink(&canvas);
    assert!(start_ink > 50);
    let mut left = Canvas::new(200, 40, PixelFormat::Rgba8).unwrap();
    e.draw(
        &mut left,
        &layout,
        Rect::new(0.0, 0.0, 200.0, 40.0),
        TextAlign::End,
        color,
    );
    assert!(left.damage().bounds().x1 < 100);
    assert!(ink(&left) > 50);
    assert!(is_bidi_control('\u{2068}') && !is_bidi_control('a'));
    assert!(missing_families().len() < 4);
}
