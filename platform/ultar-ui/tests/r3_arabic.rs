//! Review round 3, "Prüfer Bild": Arabic rendering measured on the rasterised glyphs –
//! weight, joining, digits, mixed direction, marks inside the line box, 12-point legibility.
//! Target path: platform/ultar-ui/tests/r3_arabic.rs

use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::text::{isolate, FontId, FontRole, TextAlign, TextEngine, TextStyle, Weight};
use ultar_ui::tokens::{ColorRole, TypeSize, LIGHT};
use ultar_ui::{Lang, Rect};

const W: i32 = 900;
const H: i32 = 240;
const AREA_X: f32 = 150.0;
const AREA_Y: f32 = 90.0;

/// Ink of a text drawn white on a black canvas: bounding box (left, top, right, bottom) relative to
/// the layout box and the sum of the coverage.
struct Ink {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    sum: u64,
    box_w: f32,
    box_h: f32,
}

fn ink(engine: &mut TextEngine, text: &str, style: &TextStyle) -> Ink {
    let mut canvas = Canvas::new(W as u32, H as u32, PixelFormat::Rgba8).unwrap();
    // A new canvas is black; the surface colour of the light theme is pure white.
    let white = LIGHT.get(ColorRole::Surface);
    let layout = engine.layout(text, style, None);
    let area = Rect::new(AREA_X, AREA_Y, layout.width, layout.height);
    engine.draw(&mut canvas, &layout, area, TextAlign::Start, white);
    let (mut l, mut t, mut r, mut b, mut sum) = (W, H, -1, -1, 0u64);
    for y in 0..H {
        for x in 0..W {
            let v = canvas.pixel(x, y).map_or(0, |c| c.r);
            if v > 24 {
                (l, t, r, b) = (l.min(x), t.min(y), r.max(x), b.max(y));
            }
            sum += u64::from(v);
        }
    }
    Ink {
        left: l as f32 - AREA_X,
        top: t as f32 - AREA_Y,
        right: (r + 1) as f32 - AREA_X,
        bottom: (b + 1) as f32 - AREA_Y,
        sum,
        box_w: layout.width,
        box_h: layout.height,
    }
}

fn style(size: TypeSize, lang: Lang, role: FontRole, weight: Weight) -> TextStyle {
    TextStyle::new(size.points(), lang)
        .with_role(role)
        .with_weight(weight)
}

const ROLES: [FontRole; 3] = [FontRole::Text, FontRole::Title, FontRole::Mono];

/// Bold is really bold: at least a fifth more ink than regular in every Arabic face, at the
/// body size and at 12.
#[test]
fn a01_bold_arabic_has_clearly_more_ink_than_regular() {
    let mut engine = TextEngine::new().unwrap();
    let text = "إعدادات الشاشة والشبكة";
    for size in [TypeSize::S12, TypeSize::S14, TypeSize::S24] {
        for role in ROLES {
            let regular = ink(
                &mut engine,
                text,
                &style(size, Lang::Ar, role, Weight::Regular),
            );
            let bold = ink(
                &mut engine,
                text,
                &style(size, Lang::Ar, role, Weight::Bold),
            );
            let ratio = bold.sum as f64 / regular.sum as f64;
            println!("{size:?} {role:?}: bold/regular ink = {ratio:.2}");
            assert!(ratio >= 1.2, "{size:?} {role:?}: {ratio:.2}");
        }
    }
}

/// The four weights of the tokens (400, 500, 600, 700) are four visible steps in Arabic
/// running text, as the boards draw them (labels 500, buttons 600, titles 700).
#[test]
fn a02_the_four_token_weights_are_four_steps() {
    let mut engine = TextEngine::new().unwrap();
    let text = "إعدادات الشاشة والشبكة";
    let mut same = Vec::new();
    for (lang, sample) in [(Lang::Ar, text), (Lang::En, "Display and network")] {
        let mut sums = Vec::new();
        for weight in [
            Weight::Regular,
            Weight::Medium,
            Weight::Semibold,
            Weight::Bold,
        ] {
            let style = style(TypeSize::S14, lang, FontRole::Text, weight);
            sums.push(ink(&mut engine, sample, &style).sum);
        }
        if !sums.windows(2).all(|w| w[1] > w[0]) {
            same.push(format!(
                "{lang:?}: ink by weight 400/500/600/700 = {sums:?}"
            ));
        }
    }
    assert!(
        same.is_empty(),
        "token weights that draw the same:\n{}",
        same.join("\n")
    );
}

/// Stacked marks and deep descenders stay inside the line box, so that a clipping parent
/// (scroll container, text field, list row) never cuts them – also for Arabic text shown
/// in the English interface (file names, network names).
#[test]
fn a03_marks_and_descenders_stay_inside_the_line_box() {
    let mut engine = TextEngine::new().unwrap();
    let text = "ٱللّٰهُ إِنَّ جٍّ يٍّ لِأُمٍّ عِجٍّ";
    let mut outside = Vec::new();
    for lang in [Lang::Ar, Lang::En] {
        for size in TypeSize::ALL {
            for role in ROLES {
                for weight in [Weight::Regular, Weight::Bold] {
                    let i = ink(&mut engine, text, &style(size, lang, role, weight));
                    if i.top < -0.5 || i.bottom > i.box_h + 0.5 {
                        let case = format!("{lang:?} {size:?} {role:?} {weight:?}");
                        outside.push(format!("{case}: {}..{} of {}", i.top, i.bottom, i.box_h));
                    }
                }
            }
        }
    }
    assert!(
        outside.is_empty(),
        "ink outside the line box:\n{}",
        outside.join("\n")
    );
}

/// No letter reaches outside the measured width by more than a pixel (a field or a button
/// clips at that width).
#[test]
fn a04_ink_stays_inside_the_measured_width() {
    let mut engine = TextEngine::new().unwrap();
    for text in ["في", "إلى", "حج", "كتاب", "ملف", "Wj", "fy"] {
        for role in ROLES {
            for weight in [Weight::Regular, Weight::Bold] {
                let i = ink(
                    &mut engine,
                    text,
                    &style(TypeSize::S16, Lang::Ar, role, weight),
                );
                assert!(
                    i.left >= -1.5 && i.right <= i.box_w + 1.5,
                    "{text} {role:?} {weight:?}: ink {}..{} in a width of {}",
                    i.left,
                    i.right,
                    i.box_w
                );
            }
        }
    }
}

/// Secondary text (12 points) in Arabic is as legible as in Latin: the body of the letters
/// (height of a sad/hah loop without dots) is not much smaller than the Latin x-height.
#[test]
fn a05_twelve_point_arabic_is_not_smaller_than_latin() {
    let mut engine = TextEngine::new().unwrap();
    let small = |lang| style(TypeSize::S12, lang, FontRole::Text, Weight::Regular);
    let arabic = ink(&mut engine, "صصص", &small(Lang::Ar));
    let latin = ink(&mut engine, "xxx", &small(Lang::En));
    let (body, x_height) = (arabic.bottom - arabic.top, latin.bottom - latin.top);
    let density = |i: &Ink| i.sum as f64 / f64::from(i.box_w.max(1.0));
    println!(
        "12 pt: Arabic body {body} px, Latin x-height {x_height} px; ink per px of width: \
         Arabic {:.0}, Latin {:.0}",
        density(&arabic),
        density(&latin)
    );
    assert!(
        body >= x_height - 1.0,
        "Arabic letter bodies are {body} px high, the Latin x-height is {x_height} px"
    );
}

/// Left position of the glyph that draws the character at byte `at`.
fn x_of(engine: &mut TextEngine, text: &str, at: usize) -> f32 {
    let style = style(TypeSize::S14, Lang::Ar, FontRole::Text, Weight::Regular);
    let layout = engine.layout(text, &style, None);
    assert!(layout.rtl);
    let glyph = layout.glyphs.iter().find(|g| g.cluster as usize == at);
    glyph
        .unwrap_or_else(|| panic!("no glyph at byte {at} of {text}"))
        .x
}

/// Digits are 0-9 from the Latin face in reading order, a number with its unit is one
/// left-to-right island when isolated, and Arabic-Indic digits typed by the user keep
/// their own order.
#[test]
fn a06_digits_and_units_keep_their_order_in_arabic_lines() {
    let mut engine = TextEngine::new().unwrap();
    let text = format!("معدّل التحديث {}", isolate("60 Hz"));
    let at = |c: char| text.find(c).unwrap();
    let xs: Vec<f32> = "60Hz"
        .chars()
        .map(|c| x_of(&mut engine, &text, at(c)))
        .collect();
    assert!(
        xs.windows(2).all(|w| w[0] < w[1]),
        "60 Hz is not one island: {xs:?}"
    );
    assert!(
        xs[3] < x_of(&mut engine, &text, 0),
        "the island sits left of the words"
    );
    let plain = "الشبكة رقم 12345";
    let xs: Vec<f32> = "12345"
        .chars()
        .map(|c| x_of(&mut engine, plain, plain.find(c).unwrap()))
        .collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
    let style = style(TypeSize::S14, Lang::Ar, FontRole::Text, Weight::Regular);
    let layout = engine.layout(plain, &style, None);
    let digit = layout
        .glyphs
        .iter()
        .find(|g| g.cluster as usize == plain.find('1').unwrap());
    assert_eq!(
        digit.unwrap().font,
        FontId::Latin,
        "0-9 come from the Latin face"
    );
    let indic = "الرقم ١٢٣";
    let xs: Vec<f32> = "١٢٣"
        .chars()
        .map(|c| x_of(&mut engine, indic, indic.find(c).unwrap()))
        .collect();
    assert!(xs.windows(2).all(|w| w[0] < w[1]), "{xs:?}");
}

/// Letters join: the three forms of beh are three different glyphs and none of them is the
/// isolated one, in every Arabic face.
#[test]
fn a07_letters_join_in_every_arabic_face() {
    let mut engine = TextEngine::new().unwrap();
    for (role, font) in [
        (FontRole::Text, FontId::Arabic),
        (FontRole::Title, FontId::ArabicTitle),
        (FontRole::Mono, FontId::ArabicSans),
    ] {
        let style = style(TypeSize::S16, Lang::Ar, role, Weight::Regular);
        let layout = engine.layout("سسس", &style, None);
        let isolated = engine.nominal_glyph(font, 'س').unwrap();
        let mut ids: Vec<u16> = layout.glyphs.iter().map(|g| g.id).collect();
        assert!(layout.glyphs.iter().all(|g| g.font == font), "{role:?}");
        assert!(
            !ids.contains(&isolated),
            "{role:?}: an isolated form inside a word"
        );
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 3, "{role:?}: initial, medial, final");
    }
}

/// Probe (prints only): which kind of text leaves the line box.
#[test]
fn a08_probe_which_marks_leave_the_line_box() {
    let mut engine = TextEngine::new().unwrap();
    for text in ["إِنَّ الشَّبَكَةَ مُتَّصِلَةٌ", "اللّٰه", "جٍّ يٍّ", "أُمٍّ", "لأ إلى آخر", "يجحع"]
    {
        for (lang, role) in [
            (Lang::Ar, FontRole::Text),
            (Lang::Ar, FontRole::Title),
            (Lang::En, FontRole::Text),
        ] {
            let i = ink(
                &mut engine,
                text,
                &style(TypeSize::S14, lang, role, Weight::Regular),
            );
            println!(
                "probe {text} {lang:?} {role:?}: {}..{} of {}",
                i.top, i.bottom, i.box_h
            );
        }
    }
}
