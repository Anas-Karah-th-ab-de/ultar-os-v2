//! Review (agent "ui"): failing tests for the major findings. Each test states the correct
//! behaviour; all four fail on the reviewed code and pass with fixes/1..4.patch.
mod common;

use common::{click, key};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::color::Color;
use ultar_ui::event::Key;
use ultar_ui::layout::{Flex, Padding};
use ultar_ui::text::{Overflow, TextEngine, TextStyle};
use ultar_ui::tokens::{ColorRole, Space, ThemeKind, DARK};
use ultar_ui::ui::{NodeId, Ui};
use ultar_ui::widgets::{Button, ButtonVariant, Label, TextField, Widget};
use ultar_ui::{Lang, Size};

fn over(fg: Color, alpha: f32, bg: Color) -> [i32; 3] {
    let m = |f: u8, b: u8| (f32::from(f) * alpha + f32::from(b) * (1.0 - alpha)).round() as i32;
    [m(fg.r, bg.r), m(fg.g, bg.g), m(fg.b, bg.b)]
}

/// Finding 1: a disabled widget is painted with opacity 0.4 (style.rs DISABLED_OPACITY), not
/// with 0.4 * 0.4 = 0.16.
#[test]
fn f1_disabled_widget_is_painted_at_forty_percent() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    ui.set_root_flex(Flex::column().padding(Padding::all(Space::S16)));
    let root = ui.root();
    let button = Button::new(Label::key("demo.apply")).variant(ButtonVariant::Primary);
    let b = ui.add(root, button).unwrap();
    ui.set_disabled(b, true).unwrap();
    let mut canvas = Canvas::new(400, 300, PixelFormat::Rgba8).unwrap();
    ui.paint(&mut canvas);
    let r = ui.rect(b).unwrap();
    // Background of the button, inside its start padding (no label pixels there).
    let px = canvas
        .pixel(r.x as i32 + 4, (r.y + r.h / 2.0) as i32)
        .unwrap();
    let want = over(
        DARK.get(ColorRole::Accent),
        0.4,
        DARK.get(ColorRole::Ground),
    );
    let got = [i32::from(px.r), i32::from(px.g), i32::from(px.b)];
    let close = got.iter().zip(want).all(|(g, w)| (g - w).abs() <= 1);
    assert!(
        close,
        "disabled primary background {got:?}, expected accent at 40 % = {want:?}"
    );
}

/// Finding 2: changing the selection of a select whose list is open must repaint the list
/// (check mark and bold option move); the incremental frame must equal a full repaint.
#[test]
fn f2_set_selected_repaints_the_open_popup() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(720.0, 760.0));
    let s = common::display_settings(&mut ui).unwrap();
    let mut canvas = Canvas::new(720, 760, PixelFormat::Rgba8).unwrap();
    let mut full = Canvas::new(720, 760, PixelFormat::Rgba8).unwrap();
    ui.paint(&mut canvas);
    click(&mut ui, s.resolution);
    assert!(ui.is_open(s.resolution));
    ui.paint(&mut canvas);
    ui.set_selected(s.resolution, 2).unwrap();
    ui.paint(&mut canvas);
    ui.paint_all(&mut full);
    let stale = canvas
        .data()
        .chunks_exact(4)
        .zip(full.data().chunks_exact(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(
        stale, 0,
        "{stale} stale pixels after set_selected with the list open"
    );
}

fn caret(ui: &Ui, id: NodeId) -> usize {
    match ui.widget(id) {
        Some(Widget::TextField(field)) => field.caret(),
        _ => usize::MAX,
    }
}

/// Finding 3: the physical arrow keys move the caret in the direction they point to, also
/// when the text runs against the direction of the UI (Latin text in the Arabic UI and
/// Arabic text in the English UI).
#[test]
fn f3_arrow_keys_move_the_caret_visually() {
    // Arabic UI, Latin text: the logical start is the visual left end.
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let f = ui
        .add(
            root,
            TextField::new(Label::key("demo.device")).text("hello"),
        )
        .unwrap();
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Home);
    assert_eq!(caret(&ui, f), 0);
    key(&mut ui, Key::ArrowRight);
    assert_eq!(
        caret(&ui, f),
        1,
        "Arabic UI, text 'hello': Right moves over 'h'"
    );
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(caret(&ui, f), 0, "Arabic UI, text 'hello': Left moves back");
    // English UI, Arabic text: the logical start is the visual right end.
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let f = ui
        .add(root, TextField::new(Label::key("demo.device")).text("سلام"))
        .unwrap();
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Home);
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(
        caret(&ui, f),
        2,
        "English UI, Arabic text: Left moves over the first letter"
    );
    key(&mut ui, Key::ArrowRight);
    assert_eq!(
        caret(&ui, f),
        0,
        "English UI, Arabic text: Right moves back"
    );
    // Unchanged behaviour for text in the direction of the UI.
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, Size::new(400.0, 300.0));
    let root = ui.root();
    let f = ui
        .add(root, TextField::new(Label::key("demo.device")).text("سلام"))
        .unwrap();
    key(&mut ui, Key::Tab);
    key(&mut ui, Key::Home);
    key(&mut ui, Key::ArrowLeft);
    assert_eq!(caret(&ui, f), 2);
}

/// Finding 4: the ellipsis stands next to the place where the text was cut. When the kept
/// text ends in a run against the paragraph direction (a number or a Latin word in Arabic,
/// Arabic in English) the ellipsis must not jump to the other end of that run.
#[test]
fn f4_ellipsis_stays_at_the_cut() {
    let mut engine = TextEngine::new().unwrap();
    let cases = [
        ("الرقم 1234567890123456789", Lang::Ar),
        ("Hello wonderful long world", Lang::Ar),
        ("الشبكة Wonderful-Network-5G-Extended", Lang::Ar),
        ("هذا نص عربي طويل جدا يجب أن يقطع", Lang::En),
        ("هذا نص عربي طويل جدا يجب أن يقطع", Lang::Ar),
        ("Hello wonderful long world", Lang::En),
    ];
    for (text, lang) in cases {
        let style = TextStyle::new(14.0, lang).with_overflow(Overflow::Ellipsis);
        let layout = engine.layout(text, &style, Some(90.0));
        assert!(layout.truncated && layout.width <= 90.0, "{text:?}");
        let is_ellipsis = |cluster: u32| layout.text[cluster as usize..].starts_with('…');
        let dots = layout
            .glyphs
            .iter()
            .find(|g| is_ellipsis(g.cluster))
            .unwrap();
        // The last kept character: the glyph with the largest cluster before the ellipsis.
        let last = layout
            .glyphs
            .iter()
            .filter(|g| g.cluster < dots.cluster && g.advance > 0.0)
            .max_by_key(|g| g.cluster)
            .unwrap();
        let slack = 0.01;
        if last.rtl {
            assert!(
                (dots.x + dots.advance - last.x).abs() <= slack,
                "{lang:?} {text:?} -> {:?}: ellipsis at {} but the cut (left edge of the last \
                 right-to-left glyph) is at {}",
                layout.text,
                dots.x,
                last.x
            );
        } else {
            assert!(
                (dots.x - (last.x + last.advance)).abs() <= slack,
                "{lang:?} {text:?} -> {:?}: ellipsis at {} but the cut (right edge of the last \
                 left-to-right glyph) is at {}",
                layout.text,
                dots.x,
                last.x + last.advance
            );
        }
    }
}
