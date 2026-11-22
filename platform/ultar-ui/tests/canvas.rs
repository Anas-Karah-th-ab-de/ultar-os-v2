//! Software canvas: shapes, anti-aliasing, blending, clipping, damage, blit, formats.

use ultar_ui::canvas::{Canvas, CanvasError, PixelFormat, MAX_DIMENSION};
use ultar_ui::tokens::{ColorRole, DARK, LIGHT};
use ultar_ui::{Color, PixelRect, Point, Rect};

fn canvas(w: u32, h: u32) -> Canvas {
    Canvas::new(w, h, PixelFormat::Rgba8).unwrap()
}

fn text() -> Color {
    DARK.get(ColorRole::Text)
}

fn ground() -> Color {
    DARK.get(ColorRole::Ground)
}

#[test]
fn creation_and_errors() {
    assert_eq!(
        Canvas::new(0, 10, PixelFormat::Rgba8).unwrap_err(),
        CanvasError::InvalidSize {
            width: 0,
            height: 10
        }
    );
    assert!(Canvas::new(10, MAX_DIMENSION + 1, PixelFormat::Bgrx8).is_err());
    let c = canvas(4, 3);
    assert_eq!(
        (c.width(), c.height(), c.format()),
        (4, 3, PixelFormat::Rgba8)
    );
    assert_eq!(c.data().len(), 4 * 3 * 4);
    assert_eq!(c.pixel(0, 0), Some(Color::TRANSPARENT));
    assert_eq!(c.pixel(4, 0), None);
    assert_eq!(c.pixel(-1, 0), None);
    assert!(c.damage().is_empty());
    let x = Canvas::new(2, 2, PixelFormat::Bgrx8).unwrap();
    assert_eq!(x.pixel(1, 1).unwrap().a, u8::MAX, "BGRX starts opaque");
    let mut c = canvas(4, 4);
    let err = c.fill_mask(0, 0, 2, 2, &[0; 3], text()).unwrap_err();
    assert_eq!(
        err,
        CanvasError::LengthMismatch {
            expected: 4,
            actual: 3
        }
    );
    assert!(
        err.to_string().contains('3')
            && CanvasError::InvalidSize {
                width: 0,
                height: 1
            }
            .to_string()
            .contains("0x1")
    );
}

#[test]
fn pixel_aligned_rect_is_exact_and_fractional_edges_are_blended() {
    let mut c = canvas(20, 20);
    c.clear(ground());
    c.fill_rrect(Rect::new(4.0, 4.0, 8.0, 8.0), 0.0, text());
    assert_eq!(c.pixel(4, 4), Some(text()));
    assert_eq!(c.pixel(11, 11), Some(text()));
    assert_eq!(c.pixel(3, 4), Some(ground()));
    assert_eq!(c.pixel(12, 11), Some(ground()));
    let mut c = canvas(20, 20);
    c.clear(ground());
    c.fill_rrect(Rect::new(4.5, 4.0, 8.0, 8.0), 0.0, text());
    let half = c.pixel(4, 8).unwrap();
    assert!(
        half != ground() && half != text(),
        "half-covered column is a mix: {half:?}"
    );
    assert_eq!(half, ground().mix(text(), 0.5));
    assert_eq!(c.pixel(5, 8), Some(text()));
}

#[test]
fn rounded_corners_are_anti_aliased() {
    let mut c = canvas(40, 40);
    c.clear(ground());
    c.fill_rrect(Rect::new(0.0, 0.0, 40.0, 40.0), 12.0, text());
    assert_eq!(
        c.pixel(0, 0),
        Some(ground()),
        "the very corner is outside the arc"
    );
    assert_eq!(c.pixel(20, 20), Some(text()));
    assert_eq!(c.pixel(20, 0), Some(text()));
    assert_eq!(c.pixel(0, 20), Some(text()));
    let partial = (0..12)
        .filter_map(|i| c.pixel(i, 11 - i))
        .filter(|p| *p != ground() && *p != text())
        .count();
    assert!(
        partial >= 1,
        "the arc crosses the diagonal with intermediate coverage"
    );
    // The shape is symmetric in all four corners.
    for (x, y) in [(2, 3), (1, 6), (4, 1)] {
        let p = c.pixel(x, y);
        assert_eq!(p, c.pixel(39 - x, y));
        assert_eq!(p, c.pixel(x, 39 - y));
        assert_eq!(p, c.pixel(39 - x, 39 - y));
    }
    // A radius larger than half the box is clamped: a circle.
    let mut c = canvas(20, 20);
    c.fill_rrect(Rect::new(0.0, 0.0, 20.0, 20.0), 9999.0, text());
    assert_eq!(c.pixel(0, 0).unwrap().a, 0);
    assert_eq!(c.pixel(10, 10), Some(text()));
    assert!(
        c.pixel(0, 10).unwrap().a >= 250,
        "the leftmost pixel of the circle is almost fully covered"
    );
}

#[test]
fn stroke_draws_only_the_border() {
    let mut c = canvas(40, 30);
    c.clear(ground());
    c.stroke_rrect(Rect::new(5.0, 5.0, 30.0, 20.0), 6.0, 2.0, text());
    assert_eq!(c.pixel(20, 5), Some(text()));
    assert_eq!(c.pixel(20, 6), Some(text()));
    assert_eq!(c.pixel(20, 7), Some(ground()), "inside is untouched");
    assert_eq!(c.pixel(20, 15), Some(ground()));
    assert_eq!(c.pixel(5, 15), Some(text()));
    assert_eq!(c.pixel(34, 15), Some(text()));
    assert_eq!(c.pixel(20, 24), Some(text()));
    assert_eq!(c.pixel(4, 15), Some(ground()));
    assert_eq!(
        c.pixel(5, 5),
        Some(ground()),
        "corner pixel is outside the rounded border"
    );
    let before = c.data().to_vec();
    c.stroke_rrect(Rect::new(5.0, 5.0, 30.0, 20.0), 6.0, 0.0, text());
    c.fill_rrect(Rect::new(5.0, 5.0, 0.0, 20.0), 0.0, text());
    assert_eq!(
        c.data(),
        &before[..],
        "zero width and empty rects draw nothing"
    );
}

#[test]
fn alpha_blending_is_source_over() {
    let mut c = canvas(4, 4);
    c.clear(ground());
    let translucent = text().with_alpha(0.5);
    c.fill_rrect(Rect::new(0.0, 0.0, 4.0, 4.0), 0.0, translucent);
    let got = c.pixel(1, 1).unwrap();
    let want = ground().mix(text(), f32::from(translucent.a) / 255.0);
    for (a, b) in [(got.r, want.r), (got.g, want.g), (got.b, want.b)] {
        assert!(a.abs_diff(b) <= 1, "{got:?} vs {want:?}");
    }
    assert_eq!(got.a, 255);
    c.fill_rrect(Rect::new(0.0, 0.0, 4.0, 4.0), 0.0, Color::TRANSPARENT);
    assert_eq!(c.pixel(1, 1), Some(got));
    // On a transparent canvas the alpha channel accumulates.
    let mut c = canvas(2, 2);
    c.fill_rrect(Rect::new(0.0, 0.0, 2.0, 2.0), 0.0, translucent);
    assert_eq!(c.pixel(0, 0).unwrap().a, translucent.a);
}

#[test]
fn clipping_limits_every_operation() {
    let mut c = canvas(30, 30);
    c.clear(ground());
    let previous = c.set_clip(PixelRect::new(10, 10, 20, 20));
    assert_eq!(previous, c.bounds());
    c.fill_rrect(Rect::new(0.0, 0.0, 30.0, 30.0), 0.0, text());
    c.line(Point::new(0.0, 25.0), Point::new(30.0, 25.0), 3.0, text());
    c.shadow(Rect::new(0.0, 0.0, 5.0, 5.0), 0.0, 8.0, text());
    c.fill_mask(0, 0, 2, 2, &[255; 4], text()).unwrap();
    c.clear(text());
    assert_eq!(c.pixel(9, 15), Some(ground()));
    assert_eq!(c.pixel(20, 15), Some(ground()));
    assert_eq!(c.pixel(15, 25), Some(ground()));
    assert_eq!(c.pixel(1, 1), Some(ground()));
    assert_eq!(c.pixel(10, 10), Some(text()));
    assert_eq!(c.pixel(19, 19), Some(text()));
    c.set_clip(PixelRect::new(-50, -50, 500, 500));
    assert_eq!(c.clip(), c.bounds(), "the clip never exceeds the surface");
    c.set_clip(PixelRect::new(5, 5, 5, 5));
    c.clear(text());
    assert_eq!(c.pixel(5, 5), Some(ground()), "an empty clip draws nothing");
    c.reset_clip();
    assert_eq!(c.clip(), c.bounds());
}

#[test]
fn damage_lists_what_was_drawn() {
    let mut c = canvas(200, 100);
    c.fill_rrect(Rect::new(10.0, 10.0, 20.0, 20.0), 4.0, text());
    c.fill_rrect(Rect::new(150.0, 60.0, 20.0, 20.0), 4.0, text());
    assert_eq!(c.damage().rects().len(), 2);
    assert!(c.damage().rects().contains(&PixelRect::new(10, 10, 30, 30)));
    assert!(c
        .damage()
        .rects()
        .contains(&PixelRect::new(150, 60, 170, 80)));
    let taken = c.take_damage();
    assert_eq!(taken.area(), 800);
    assert!(c.damage().is_empty());
    c.fill_rrect(Rect::new(500.0, 500.0, 20.0, 20.0), 0.0, text());
    assert!(
        c.damage().is_empty(),
        "drawing outside the surface damages nothing"
    );
    c.line(Point::new(50.0, 50.0), Point::new(60.0, 50.0), 2.0, text());
    let line = c.damage().bounds();
    assert!(line.x0 <= 49 && line.x1 >= 61 && line.y0 <= 49 && line.y1 >= 51 && line.height() <= 6);
}

#[test]
fn lines_and_polylines() {
    let mut c = canvas(40, 40);
    c.clear(ground());
    c.line(Point::new(5.0, 20.5), Point::new(35.0, 20.5), 3.0, text());
    assert_eq!(c.pixel(20, 20), Some(text()));
    assert_eq!(c.pixel(20, 19), Some(text()));
    assert_eq!(c.pixel(20, 21), Some(text()));
    assert_eq!(c.pixel(20, 17), Some(ground()));
    // Overlapping segments of one translucent polyline are not blended twice.
    let mut c = canvas(40, 40);
    c.clear(ground());
    let half = text().with_alpha(0.5);
    let corner = [
        Point::new(5.0, 5.5),
        Point::new(30.5, 5.5),
        Point::new(30.5, 30.0),
    ];
    c.stroke_polyline(&corner, false, 4.0, half);
    assert_eq!(
        c.pixel(30, 5),
        c.pixel(15, 5),
        "the joint is as dark as the straight part"
    );
    let mut closed = canvas(40, 40);
    closed.stroke_polyline(&corner, true, 2.0, text());
    assert!(
        closed.pixel(18, 18).unwrap().a > 0,
        "closing draws the diagonal back to the start"
    );
    let mut none = canvas(8, 8);
    none.stroke_polyline(&[], false, 2.0, text());
    none.line(Point::new(1.0, 1.0), Point::new(5.0, 5.0), 0.0, text());
    assert!(none.damage().is_empty());
    none.stroke_polyline(&[Point::new(4.0, 4.0)], false, 3.0, text());
    assert!(none.pixel(4, 4).unwrap().a > 0, "a single point is a dot");
}

#[test]
fn shadow_fades_outwards() {
    let mut c = canvas(120, 120);
    let shadow = Color::BLACK.with_alpha(0.4);
    c.shadow(Rect::new(40.0, 40.0, 40.0, 40.0), 8.0, 32.0, shadow);
    let alpha = |x: i32, y: i32| c.pixel(x, y).unwrap().a;
    assert_eq!(alpha(60, 60), shadow.a, "full strength well inside");
    let (inner, edge, outer) = (alpha(60, 44), alpha(60, 40), alpha(60, 30));
    assert!(
        inner > edge && edge > outer && outer > 0,
        "{inner} {edge} {outer}"
    );
    assert!(
        edge.abs_diff(shadow.a / 2) <= 2,
        "half strength on the outline"
    );
    assert_eq!(alpha(60, 20), 0, "nothing beyond half the blur");
    assert_eq!(alpha(60, 40), alpha(40, 60));
    assert_eq!(alpha(60, 30), alpha(60, 89));
}

#[test]
fn blit_copies_clips_and_converts() {
    let mut source = canvas(10, 10);
    source.clear(text());
    source.fill_rrect(
        Rect::new(0.0, 0.0, 5.0, 10.0),
        0.0,
        LIGHT.get(ColorRole::Accent),
    );
    let mut target = Canvas::new(20, 20, PixelFormat::Bgrx8).unwrap();
    target.take_damage();
    target.blit(&source, source.bounds(), 15, 15);
    assert_eq!(
        target.pixel(15, 15),
        Some(LIGHT.get(ColorRole::Accent)),
        "RGBA to BGRX keeps the colour"
    );
    assert_eq!(target.pixel(14, 15).unwrap(), Color::BLACK);
    assert_eq!(target.damage().bounds(), PixelRect::new(15, 15, 20, 20));
    target.blit(&source, PixelRect::new(5, 0, 10, 10), -2, 0);
    assert_eq!(target.pixel(0, 0), Some(text()));
    assert_eq!(target.pixel(2, 0), Some(text()));
    assert_eq!(target.pixel(3, 0), Some(Color::BLACK));
    target.blit(&source, PixelRect::new(50, 50, 60, 60), 0, 0);
    target.blit(&source, source.bounds(), 100, 100);
    let mut same = canvas(10, 10);
    same.blit(&source, source.bounds(), 0, 0);
    assert_eq!(same.data(), source.data());
    assert_eq!(target.to_rgba().len(), 20 * 20 * 4);
    assert_eq!(&target.to_rgba()[..4], &[text().r, text().g, text().b, 255]);
}

#[test]
fn both_formats_render_the_same_picture() {
    let draw = |c: &mut Canvas| {
        c.clear(ground());
        c.fill_rrect(Rect::new(3.3, 2.2, 30.0, 20.0), 6.0, text().with_alpha(0.7));
        c.stroke_rrect(
            Rect::new(1.0, 1.0, 38.0, 28.0),
            8.0,
            1.5,
            DARK.get(ColorRole::Accent),
        );
        c.shadow(
            Rect::new(10.0, 10.0, 10.0, 10.0),
            2.0,
            6.0,
            Color::BLACK.with_alpha(0.3),
        );
        c.line(
            Point::new(0.0, 0.0),
            Point::new(40.0, 30.0),
            1.5,
            DARK.get(ColorRole::Danger),
        );
    };
    let mut rgba = canvas(40, 30);
    let mut bgrx = Canvas::new(40, 30, PixelFormat::Bgrx8).unwrap();
    draw(&mut rgba);
    draw(&mut bgrx);
    assert_eq!(rgba.to_rgba(), bgrx.to_rgba());
    assert_ne!(rgba.data(), bgrx.data());
}
