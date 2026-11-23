//! Review (agent "ui"): tests for the minor findings; each states the correct behaviour and
//! fails on the reviewed code.
mod common;

use common::{center, click, press, release};
use std::panic::{catch_unwind, AssertUnwindSafe};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{ActionKind, Event};
use ultar_ui::text::{Overflow, TextAlign, TextEngine, TextStyle};
use ultar_ui::tokens::{ColorRole, ThemeKind, DARK};
use ultar_ui::widgets::{Button, Label, Select, Slider, Switch, TextField};
use ultar_ui::{Lang, PixelRect, Point, Rect, Size};

const S: Size = Size::new(400.0, 300.0);

#[test]
fn m1_slider_with_nan_bound_does_not_panic() {
    let built =
        catch_unwind(|| Slider::new(Label::key("demo.brightness"), f32::NAN, 1.0, 0.1).value(0.5));
    assert!(
        built.is_ok(),
        "Slider::new(NaN, 1, ..).value(..) panics in f32::clamp"
    );
}

#[test]
fn m2_slider_with_infinite_step_keeps_a_number() {
    let slider = Slider::new(Label::key("demo.brightness"), 0.0, 100.0, f32::INFINITY).value(50.0);
    assert!(!slider.value.is_nan(), "value became NaN");
}

#[test]
fn m3_switch_knob_does_not_jump_when_toggled_back_mid_animation() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, S);
    let root = ui.root();
    let sw = ui
        .add(root, Switch::new(Label::key("demo.night"), false))
        .unwrap();
    let mut canvas = Canvas::new(400, 300, PixelFormat::Rgba8).unwrap();
    let knob_left = |ui: &mut ultar_ui::ui::Ui, canvas: &mut Canvas, role: ColorRole| {
        ui.paint_all(canvas);
        let r = ui.rect(sw).unwrap();
        let y = (r.y + r.h / 2.0) as i32;
        let color = DARK.get(role);
        (r.x as i32..r.right() as i32)
            .find(|x| canvas.pixel(*x, y) == Some(color))
            .unwrap()
    };
    ui.advance_to(1000);
    click(&mut ui, sw); // on; the knob starts to travel
    ui.advance_to(1010); // 10 of 120 ms
    let travelling = knob_left(&mut ui, &mut canvas, ColorRole::OnAccent);
    click(&mut ui, sw); // off again at the same instant
    let after = knob_left(&mut ui, &mut canvas, ColorRole::Surface);
    assert!(
        (after - travelling).abs() <= 2,
        "knob jumped from x={travelling} to x={after} without time passing"
    );
}

#[test]
fn m4_text_field_refuses_line_and_paragraph_separators() {
    let mut field = TextField::new(Label::key("demo.device"));
    assert!(
        !field.insert('\u{2028}'),
        "U+2028 LINE SEPARATOR accepted into a single-line field"
    );
    assert!(
        !field.insert('\u{2029}'),
        "U+2029 PARAGRAPH SEPARATOR accepted"
    );
}

#[test]
fn m5_ellipsis_of_multi_line_text_keeps_the_whole_first_line() {
    let mut engine = TextEngine::new().unwrap();
    let style = TextStyle::new(14.0, Lang::Ar).with_overflow(Overflow::Ellipsis);
    let layout = engine.layout("سطر أول\nسطر ثان", &style, Some(200.0));
    assert_eq!(
        layout.text, "سطر أول…",
        "the first line fits; only the second is cut"
    );
}

#[test]
fn m6_caret_inside_lam_alef_has_its_own_position() {
    let mut engine = TextEngine::new().unwrap();
    let layout = engine.layout("لا", &TextStyle::new(14.0, Lang::Ar), None);
    let (before, between, after) = (layout.caret_x(0), layout.caret_x(2), layout.caret_x(4));
    assert!(
        between < before && between > after,
        "caret before lam {before}, between lam and alef {between}, after alef {after}"
    );
}

#[test]
fn m7_glyph_cache_stays_within_its_documented_bound() {
    let limit = 64 * 1024;
    let mut engine = TextEngine::with_limits(limit, limit).unwrap();
    let mut canvas = Canvas::new(64, 64, PixelFormat::Rgba8).unwrap();
    let (mut largest, mut worst) = (0usize, 0usize);
    for size in [900.0f32, 950.0, 1000.0, 1050.0, 1100.0] {
        let before = engine.glyph_cache_stats().cost;
        let layout = engine.layout("W", &TextStyle::new(size, Lang::En), None);
        engine.draw(
            &mut canvas,
            &layout,
            Rect::new(0.0, 0.0, 64.0, 64.0),
            TextAlign::Start,
            DARK.get(ColorRole::Text),
        );
        let now = engine.glyph_cache_stats().cost;
        largest = largest.max(now.saturating_sub(before));
        worst = worst.max(now);
    }
    assert!(
        worst <= 2 * limit + largest,
        "held {worst} bytes; documented bound 2 x {limit} + one entry ({largest})"
    );
}

/// Runs one drawing call on a small canvas; false if it panicked (overflow checks are on in test builds).
fn m8_survives(f: &dyn Fn(&mut Canvas)) -> bool {
    let mut canvas = Canvas::new(64, 64, PixelFormat::Rgba8).unwrap();
    catch_unwind(AssertUnwindSafe(|| f(&mut canvas))).is_ok()
}

fn m8_assert_none_failed(results: &[(&str, bool)]) {
    let failed: Vec<&str> = results
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(name, _)| *name)
        .collect();
    assert!(
        failed.is_empty(),
        "arithmetic overflow (panic in builds with overflow checks) in: {failed:?}"
    );
}

#[test]
fn m8_canvas_takes_extreme_coordinates_without_overflow() {
    let color = DARK.get(ColorRole::Text);
    let source = Canvas::new(4, 4, PixelFormat::Rgba8).unwrap();
    m8_assert_none_failed(&[
        (
            "fill_mask x=i32::MAX",
            m8_survives(&|c| {
                let _ = c.fill_mask(i32::MAX, 0, 2, 2, &[255; 4], color);
            }),
        ),
        (
            "fill_mask y=i32::MAX-1",
            m8_survives(&|c| {
                let _ = c.fill_mask(0, i32::MAX - 1, 2, 2, &[255; 4], color);
            }),
        ),
        (
            "blit from i32::MIN",
            m8_survives(&|c| {
                c.blit(
                    &source,
                    PixelRect::new(i32::MIN, i32::MIN, i32::MAX, i32::MAX),
                    0,
                    0,
                );
            }),
        ),
        (
            "blit to i32::MAX",
            m8_survives(&|c| {
                c.blit(&source, PixelRect::new(0, 0, 4, 4), i32::MAX, i32::MAX);
            }),
        ),
    ]);
}

#[test]
fn m8_shapes_and_surfaces_take_extreme_sizes_without_overflow() {
    let color = DARK.get(ColorRole::Text);
    let huge_surface = catch_unwind(|| {
        let mut ui = common::ui(Lang::En, ThemeKind::Dark, Size::new(3e9, 760.0));
        common::display_settings(&mut ui).unwrap();
        let mut canvas = Canvas::new(64, 64, PixelFormat::Rgba8).unwrap();
        ui.paint(&mut canvas);
    })
    .is_ok();
    m8_assert_none_failed(&[
        (
            "fill_rrect radius 1e10",
            m8_survives(&|c| c.fill_rrect(Rect::new(-5e9, -5e9, 1e10, 1e10), 1e10, color)),
        ),
        (
            "stroke_rrect width 3e9",
            m8_survives(&|c| c.stroke_rrect(Rect::new(0.0, 0.0, 50.0, 50.0), 2.0, 3e9, color)),
        ),
        (
            "shadow blur 3e9",
            m8_survives(&|c| c.shadow(Rect::new(0.0, 0.0, 50.0, 50.0), 2.0, 3e9, color)),
        ),
        (
            "PixelRect::width",
            catch_unwind(|| PixelRect::new(i32::MIN, 0, i32::MAX, 1).width()).is_ok(),
        ),
        ("Ui::paint, surface 3e9 wide", huge_surface),
    ]);
}

#[test]
fn m9_every_option_of_an_open_select_is_inside_the_surface() {
    // Surface too low for the list below or above the field.
    //
    // Amended when the finding was fixed (wave 3): six 32-point targets cannot all be visible
    // on a 120-point-high surface, so – as the review's own suggested fix says – the list
    // shows the rows that fit and scrolls. The original loop only pressed and released; it
    // now also turns the wheel over the list, and additionally checks what the test's name
    // promises: every visible row lies inside the surface.
    let size = Size::new(300.0, 120.0);
    let options = || {
        (0..6)
            .map(|i| Label::data(&format!("option {i}")))
            .collect::<Vec<_>>()
    };
    let mut reachable = std::collections::BTreeSet::new();
    for wheel in 0..6 {
        for y in (0..120).step_by(2) {
            let mut ui = common::ui(Lang::En, ThemeKind::Dark, size);
            let root = ui.root();
            let select = ui
                .add(
                    root,
                    Select::new(Label::key("demo.orientation"), options()).selected(0),
                )
                .unwrap();
            click(&mut ui, select);
            let pos = Point::new(ui.rect(select).unwrap().x + 20.0, y as f32);
            for _ in 0..wheel {
                ui.handle(Event::Scroll {
                    pos: Point::new(pos.x, 60.0),
                    lines: 1.0,
                });
            }
            press(&mut ui, pos);
            for action in release(&mut ui, pos).actions {
                if let ActionKind::Selected(index) = action.kind {
                    assert!(
                        (36.0..120.0).contains(&pos.y),
                        "option {index} chosen at y={y}, outside the list"
                    );
                    reachable.insert(index);
                }
            }
        }
    }
    // Option 0 is the chosen one; choosing it again reports nothing.
    assert_eq!(
        reachable.into_iter().collect::<Vec<_>>(),
        vec![1, 2, 3, 4, 5],
        "options reachable by pointer on a 120-point-high surface"
    );
}

#[test]
fn m10_a_touch_tap_is_not_lost_while_a_mouse_button_is_held_elsewhere() {
    let mut ui = common::ui(Lang::En, ThemeKind::Dark, S);
    let root = ui.root();
    let button = ui.add(root, Button::new(Label::key("demo.apply"))).unwrap();
    let switch = ui
        .add(root, Switch::new(Label::key("demo.night"), false))
        .unwrap();
    let (pb, ps) = (center(&mut ui, button), center(&mut ui, switch));
    ui.handle(Event::TouchDown { id: 7, pos: pb });
    press(&mut ui, ps);
    let tap = ui.handle(Event::TouchUp { id: 7, pos: pb });
    let mouse = release(&mut ui, ps);
    let kinds: Vec<_> = tap
        .actions
        .iter()
        .chain(&mouse.actions)
        .map(|a| a.kind)
        .collect();
    assert!(!kinds.is_empty(), "both the tap and the click were dropped");
}
