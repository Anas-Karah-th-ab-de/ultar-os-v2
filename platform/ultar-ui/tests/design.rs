//! Design fidelity on the toolkit side (review round 1, section 3 B): state tokens, the
//! shadow of the token, the raised card, real bold Arabic, title and mono families, and the
//! fixed text direction of a field.

mod common;

use common::{center, key};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key};
use ultar_ui::layout::{CrossAlign, Flex, Padding, Sizing};
use ultar_ui::paint::TextSpec;
use ultar_ui::text::{FontId, FontRole, TextEngine, TextStyle, Weight};
use ultar_ui::tokens::{shadow, state, ColorRole, Space, ThemeKind, TypeSize, DARK};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, ButtonVariant, Card, Label, Text, TextDirection, TextField};
use ultar_ui::{Lang, PixelRect, Rect, Size};

const SURFACE: Size = Size::new(400.0, 300.0);

fn paint(ui: &mut Ui) -> Canvas {
    let mut canvas = Canvas::new(400, 300, PixelFormat::Rgba8).unwrap();
    ui.paint_all(&mut canvas);
    canvas
}

fn card_ui(card: Card) -> (Ui, NodeId) {
    themed_card_ui(card, ThemeKind::Dark)
}

fn themed_card_ui(card: Card, theme: ThemeKind) -> (Ui, NodeId) {
    let mut ui = common::ui(Lang::En, theme, SURFACE);
    let pad = Padding::all(Space::S48);
    ui.set_root_flex(Flex::column().padding(pad).cross(CrossAlign::Center));
    let root = ui.root();
    let sizing = Sizing::width(160.0).with_height(80.0);
    let id = ui.add_sized(root, card, sizing).unwrap();
    ui.layout();
    (ui, id)
}

/// Bounding box of the pixels that differ between two frames.
fn changed(a: &Canvas, b: &Canvas) -> Rect {
    let mut bounds = PixelRect::default();
    for y in 0..300 {
        for x in 0..400 {
            if a.pixel(x, y) != b.pixel(x, y) {
                bounds = bounds.union(&PixelRect::new(x, y, x + 1, y + 1));
            }
        }
    }
    bounds.to_rect()
}

#[test]
fn the_shadow_reaches_as_far_as_the_css_shadow_of_the_token() {
    // Measured on the light ground, where the faint end of the fade is still a colour step
    // (on the nearly black dark ground the last third of it rounds to nothing).
    let (mut plain, id) = themed_card_ui(Card::new(), ThemeKind::Light);
    let (mut floating, _) = themed_card_ui(Card::new().floating(), ThemeKind::Light);
    let rect = plain.rect(id).unwrap();
    let ink = changed(&paint(&mut plain), &paint(&mut floating));
    // CSS `0 12px 32px`: the shadow fades out 32 beside and 12 + 32 below the box. The last
    // steps of the fade are below one colour step, so a few points less are visible.
    let beside = rect.x - ink.x;
    let below = ink.bottom() - rect.bottom();
    assert!(
        beside > 0.7 * shadow::BLUR && beside <= shadow::BLUR,
        "beside: {beside}"
    );
    let full = shadow::OFFSET_Y + shadow::BLUR;
    assert!(
        below > 0.7 * full && below <= full,
        "below: {below} of {full}"
    );
    assert!(
        (ink.right() - rect.right() - beside).abs() <= 1.0,
        "symmetric"
    );
    // Above the box the offset takes the shadow away again.
    assert!(rect.y - ink.y <= shadow::BLUR - shadow::OFFSET_Y);
}

#[test]
fn the_raised_card_has_the_raised_background_and_an_accent_border() {
    let (mut ui, id) = card_ui(Card::padded().raised());
    let rect = ui.rect(id).unwrap();
    let canvas = paint(&mut ui);
    let (cx, cy) = (rect.center().x as i32, rect.center().y as i32);
    assert_eq!(canvas.pixel(cx, cy), Some(DARK.get(ColorRole::Raised)));
    assert_eq!(
        canvas.pixel(cx, rect.y as i32),
        Some(DARK.get(ColorRole::Accent)),
        "top border"
    );
    assert_eq!(
        canvas.pixel(rect.x as i32, cy),
        Some(DARK.get(ColorRole::Accent)),
        "start border"
    );
    // The plain floating card keeps surface and line.
    let (mut ui, id) = card_ui(Card::new().floating());
    let rect = ui.rect(id).unwrap();
    let canvas = paint(&mut ui);
    assert_eq!(canvas.pixel(cx, cy), Some(DARK.get(ColorRole::Surface)));
    assert_eq!(
        canvas.pixel(cx, rect.y as i32),
        Some(DARK.get(ColorRole::Line))
    );
}

#[test]
fn hover_pressed_and_focus_use_the_state_tokens() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, SURFACE);
    ui.set_root_flex(Flex::column().padding(Padding::all(Space::S48)));
    let root = ui.root();
    let primary = Button::new(Label::data("")).variant(ButtonVariant::Primary);
    let button = ui.add_sized(root, primary, Sizing::width(120.0)).unwrap();
    let pos = center(&mut ui, button);
    let (accent, on) = (DARK.get(ColorRole::Accent), DARK.get(ColorRole::OnAccent));
    let at = |ui: &mut Ui| paint(ui).pixel(pos.x as i32, pos.y as i32);
    assert_eq!(at(&mut ui), Some(accent));
    ui.handle(Event::PointerMove { pos });
    assert_eq!(at(&mut ui), Some(accent.mix(on, state::HOVER_OVERLAY)));
    common::press(&mut ui, pos);
    assert_eq!(at(&mut ui), Some(accent.mix(on, state::PRESSED_OVERLAY)));
    common::release(&mut ui, pos);
    ui.handle(Event::PointerLeave);
    key(&mut ui, Key::Tab);
    let rect = ui.rect(button).unwrap();
    let canvas = paint(&mut ui);
    // The ring sits outside the button, one ring width away from it.
    let ring_y = (rect.y - 3.0) as i32;
    assert_eq!(
        canvas.pixel(pos.x as i32, ring_y),
        Some(DARK.get(state::FOCUS_RING))
    );
    assert_eq!(state::FOCUS_RING, ColorRole::AccentText);
}

#[test]
fn arabic_is_drawn_bold_and_titles_and_code_have_their_own_families() {
    let mut engine = TextEngine::new().unwrap();
    let word = "الشاشات";
    let style = |role, weight| {
        TextStyle::new(24.0, Lang::Ar)
            .with_role(role)
            .with_weight(weight)
    };
    let mut widths = Vec::new();
    for (role, weight, font) in [
        (FontRole::Text, Weight::Regular, FontId::Arabic),
        (FontRole::Text, Weight::Medium, FontId::ArabicMedium),
        (FontRole::Text, Weight::Bold, FontId::ArabicBold),
        (FontRole::Title, Weight::Regular, FontId::ArabicTitle),
        (FontRole::Title, Weight::Bold, FontId::ArabicTitleBold),
        (FontRole::Mono, Weight::Regular, FontId::ArabicSans),
        (FontRole::Mono, Weight::Bold, FontId::ArabicSansBold),
    ] {
        let layout = engine.layout(word, &style(role, weight), None);
        assert!(layout.glyphs.iter().all(|g| g.font == font), "{font:?}");
        widths.push(layout.width);
    }
    for (i, a) in widths.iter().enumerate() {
        for b in &widths[i + 1..] {
            assert!((a - b).abs() > 0.05, "two faces draw the same: {widths:?}");
        }
    }
    assert!(widths[2] > widths[0], "bold Naskh is wider than regular");
    // 600 uses the bold file as well: semibold labels are visibly heavier than regular.
    let semibold = engine.layout(word, &style(FontRole::Text, Weight::Semibold), None);
    assert_eq!(semibold.width, widths[2]);
    assert_eq!(ultar_ui::text::missing_families(), Vec::<&str>::new());
    // A title spec selects the title family.
    let spec = TextSpec::title(TypeSize::S24);
    assert_eq!((spec.role, spec.weight), (FontRole::Title, Weight::Bold));
}

#[test]
fn a_title_and_a_text_look_different_in_arabic_but_share_the_latin_face() {
    let picture = |lang: Lang, spec: TextSpec| {
        let mut ui = common::ui(lang, ThemeKind::Dark, SURFACE);
        let root = ui.root();
        ui.add(root, Text::new(Label::key("demo.display.title")).spec(spec))
            .unwrap();
        paint(&mut ui).to_rgba()
    };
    let bold = TextSpec::new(TypeSize::S24).weight(Weight::Bold);
    let title = TextSpec::title(TypeSize::S24);
    assert_ne!(
        picture(Lang::Ar, bold),
        picture(Lang::Ar, title),
        "Kufi vs Naskh"
    );
    assert_eq!(
        picture(Lang::En, bold),
        picture(Lang::En, title),
        "one Latin family"
    );
}

#[test]
fn a_left_to_right_field_keeps_a_phone_number_readable_in_arabic() {
    let phone = "+963 11 123 4567";
    let mut engine = TextEngine::new().unwrap();
    let order = |style: &TextStyle, engine: &mut TextEngine| -> String {
        let layout = engine.layout(phone, style, None);
        let mut clusters = layout.visual_clusters();
        clusters.dedup();
        clusters
            .iter()
            .map(|c| phone[*c as usize..].chars().next().unwrap())
            .collect()
    };
    let mut style = TextStyle::new(14.0, Lang::Ar);
    assert_ne!(
        order(&style, &mut engine),
        phone,
        "UAX #9 in an Arabic paragraph"
    );
    style.direction = TextDirection::Ltr.resolve(Lang::Ar.direction());
    assert_eq!(order(&style, &mut engine), phone);
    assert_eq!(
        TextDirection::Auto.resolve(Lang::Ar.direction()),
        Lang::Ar.direction()
    );
    assert!(TextDirection::Rtl.resolve(Lang::En.direction()).is_rtl());
    // In the widget: the text starts at the left edge, and Right moves the caret forward.
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let root = ui.root();
    let field = TextField::new(Label::key("demo.phone")).direction(TextDirection::Ltr);
    let id = ui.add(root, field.text(phone)).unwrap();
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Home);
    key(&mut ui, Key::ArrowRight);
    key(&mut ui, Key::Char('0'));
    assert_eq!(ui.text(id).unwrap(), "+0963 11 123 4567");
    let rect = ui.rect(id).unwrap();
    let canvas = paint(&mut ui);
    let text = DARK.get(ColorRole::Text);
    let row = rect.center().y as i32;
    let first_ink = (rect.x as i32..rect.right() as i32)
        .find(|x| (row - 6..row + 6).any(|y| canvas.pixel(*x, y) == Some(text)));
    assert!(
        first_ink.unwrap() < rect.center().x as i32,
        "content starts at the left"
    );
}
