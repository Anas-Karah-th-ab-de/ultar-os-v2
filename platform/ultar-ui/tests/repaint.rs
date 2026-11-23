//! Damage tracking and partial repaint: only what changed is painted, and the result equals
//! a full repaint; theme, language and size changes repaint everything.

mod common;

use common::{center, click, display_settings};
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::event::{Event, Key};
use ultar_ui::tokens::{Space, ThemeKind};
use ultar_ui::ui::Ui;
use ultar_ui::{Lang, Size};

const SURFACE: Size = Size::new(720.0, 760.0);

fn canvas_for(ui: &Ui, format: PixelFormat) -> Canvas {
    Canvas::new(ui.size().w as u32, ui.size().h as u32, format).unwrap()
}

#[test]
fn a_hover_damages_only_the_widget() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    let s = display_settings(&mut ui).unwrap();
    let mut canvas = canvas_for(&ui, PixelFormat::Rgba8);
    assert!(ui.needs_paint());
    ui.paint(&mut canvas);
    assert_eq!(
        canvas.take_damage().bounds(),
        canvas.bounds(),
        "the first frame paints everything"
    );
    assert!(!ui.needs_paint());
    ui.paint(&mut canvas);
    assert!(
        canvas.damage().is_empty(),
        "nothing changed, nothing painted"
    );

    let pos = center(&mut ui, s.apply);
    ui.handle(Event::PointerMove { pos });
    assert!(ui.needs_paint());
    ui.paint(&mut canvas);
    let damage = canvas.take_damage();
    let button = ui.rect(s.apply).unwrap();
    let bounds = damage.bounds().to_rect();
    let slack = 2.0 * Space::S8.points() + 2.0;
    assert!(
        bounds.w <= button.w + slack && bounds.h <= button.h + slack,
        "{bounds:?}"
    );
    assert!(bounds.x <= button.x && bounds.right() >= button.right());
    assert!(
        damage.area() * 50 < canvas.bounds().area(),
        "less than 2 % of the surface"
    );
}

#[test]
fn incremental_frames_equal_a_full_repaint() {
    for format in [PixelFormat::Rgba8, PixelFormat::Bgrx8] {
        let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
        let s = display_settings(&mut ui).unwrap();
        let mut canvas = canvas_for(&ui, format);
        ui.set_reduce_motion(true);
        ui.paint(&mut canvas);
        // Several changes at once: hover moves, a switch toggles, a select opens.
        let pos = center(&mut ui, s.apply);
        ui.handle(Event::PointerMove { pos });
        click(&mut ui, s.primary);
        click(&mut ui, s.scale);
        common::key(&mut ui, Key::ArrowUp);
        ui.paint(&mut canvas);
        let mut full = canvas_for(&ui, format);
        ui.paint_all(&mut full);
        assert_eq!(
            canvas.data(),
            full.data(),
            "{format:?}: incremental frames equal a full repaint"
        );

        // Closing the popup restores what was under it, including under its shadow.
        canvas.take_damage();
        common::key(&mut ui, Key::Escape);
        ui.handle(Event::PointerLeave);
        ui.paint(&mut canvas);
        let closed_damage = canvas.take_damage();
        assert!(!closed_damage.is_empty());
        assert!(closed_damage.area() < canvas.bounds().area() / 2);
        ui.paint_all(&mut full);
        assert_eq!(canvas.data(), full.data());
    }
}

#[test]
fn theme_language_and_size_changes_repaint_everything() {
    let mut ui = common::ui(Lang::Ar, ThemeKind::Dark, SURFACE);
    display_settings(&mut ui).unwrap();
    let mut canvas = canvas_for(&ui, PixelFormat::Rgba8);
    ui.paint(&mut canvas);
    let dark = canvas.to_rgba();
    canvas.take_damage();
    ui.set_theme(ThemeKind::Light);
    ui.paint(&mut canvas);
    assert_eq!(canvas.take_damage().bounds(), canvas.bounds());
    assert_ne!(canvas.to_rgba(), dark);
    ui.set_theme(ThemeKind::Light);
    assert!(
        !ui.needs_paint(),
        "setting the same theme again changes nothing"
    );
    ui.set_lang(Lang::En);
    assert!(ui.needs_paint());
    ui.paint(&mut canvas);
    let english = canvas.to_rgba();
    ui.set_lang(Lang::Ar);
    ui.set_theme(ThemeKind::Dark);
    ui.paint(&mut canvas);
    assert_eq!(
        canvas.to_rgba(),
        dark,
        "switching back gives the identical picture"
    );
    assert_ne!(english, dark);
    ui.set_size(Size::new(700.0, 760.0)).unwrap();
    assert!(ui.needs_paint());
    ui.set_background(ultar_ui::tokens::ColorRole::Surface);
    ui.paint(&mut canvas);
    assert_eq!(
        canvas.pixel(1, 1),
        Some(ultar_ui::tokens::DARK.get(ultar_ui::tokens::ColorRole::Surface))
    );
}
