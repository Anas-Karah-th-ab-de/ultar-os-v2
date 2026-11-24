//! Pictures of the shell bricks that are not icons of the set: the brand mark (the
//! overview button) and the two-display pictograms of the display modes
//! (`design/boards/gen.py`: `mark`, `picto`). Strokes use the icon stroke width.

use crate::geom::{Point, Rect};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::tokens::{size, ColorRole};

/// What a brick shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Picture {
    /// An icon of the set.
    Icon(IconId),
    /// The brand mark (overview), in the accent text colour; never mirrored.
    Mark,
    /// Two displays, extended side by side.
    DisplaysExtend,
    /// Two displays showing the same.
    DisplaysMirror,
    /// Only the first display (this device) on.
    DisplaysFirst,
    /// Only the second display on.
    DisplaysSecond,
}

/// Segments of a full circle drawn as a polyline (smooth at 48 and below).
const ARC_STEPS: usize = 48;

/// Paints `picture` centred in `rect` at `points` (icon edge).
pub(crate) fn paint_picture(p: &mut Painter<'_>, picture: Picture, rect: Rect, points: f32) {
    match picture {
        Picture::Icon(icon) => {
            let fg = p.color(ColorRole::Text);
            p.icon(icon, rect, points, QuarterTurns::None, fg);
        }
        Picture::Mark => paint_mark(p, rect, points),
        Picture::DisplaysExtend => {
            paint_displays(p, rect, points, (Screen::Line, Screen::Line), true)
        }
        Picture::DisplaysMirror => paint_displays(p, rect, points, (Screen::On, Screen::On), false),
        Picture::DisplaysFirst => paint_displays(p, rect, points, (Screen::On, Screen::Off), false),
        Picture::DisplaysSecond => {
            paint_displays(p, rect, points, (Screen::Off, Screen::On), false)
        }
    }
}

/// `M12 3a9 9 0 1 0 9 9` and `M12 8a4 4 0 1 0 4 4` in a 24 box: two three-quarter circles
/// around the centre from the top, the long way round to the end (right).
fn paint_mark(p: &mut Painter<'_>, rect: Rect, points: f32) {
    let unit = points / 24.0;
    let c = rect.center();
    let color = p.color(ColorRole::AccentText);
    let width = size::STROKE * unit;
    for radius in [9.0 * unit, 4.0 * unit] {
        // From -90° (top) counter-clockwise through left and bottom to 0° (right).
        let steps = ARC_STEPS * 3 / 4;
        let mut previous = Point::new(c.x, c.y - radius);
        for i in 1..=steps {
            let a = -std::f32::consts::FRAC_PI_2
                - std::f32::consts::PI * 1.5 * (i as f32 / steps as f32);
            let next = Point::new(c.x + radius * a.cos(), c.y + radius * a.sin());
            p.line(previous, next, width, color);
            previous = next;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    /// Filled: on.
    On,
    /// Dashed outline: off.
    Off,
    /// Outline: part of an extended desktop.
    Line,
}

/// Two 20 × 16 screens in a 48 × 24 box (24 = `points`); extend joins them with a line.
fn paint_displays(
    p: &mut Painter<'_>,
    rect: Rect,
    points: f32,
    (a, b): (Screen, Screen),
    join: bool,
) {
    let unit = points / 24.0;
    let box_w = 48.0 * unit;
    let origin = Point::new(rect.center().x - box_w / 2.0, rect.center().y - 12.0 * unit);
    let color = p.color(ColorRole::Text);
    let width = size::STROKE * unit;
    for (x, screen) in [(2.0, a), (26.0, b)] {
        let r = Rect::new(
            origin.x + x * unit,
            origin.y + 4.0 * unit,
            20.0 * unit,
            16.0 * unit,
        );
        match screen {
            Screen::On => {
                p.fill(r, None, color);
                p.stroke(r, None, width, color);
            }
            Screen::Line => p.stroke(r, None, width, color),
            Screen::Off => dashed(p, r, width, size::FOCUS_RING * unit, color),
        }
    }
    if join {
        let y = origin.y + 12.0 * unit;
        p.line(
            Point::new(origin.x + 16.0 * unit, y),
            Point::new(origin.x + 32.0 * unit, y),
            width,
            color,
        );
    }
}

/// A dashed rectangle outline: dashes and gaps of `dash` points.
fn dashed(p: &mut Painter<'_>, r: Rect, width: f32, dash: f32, color: crate::color::Color) {
    let edges = [
        (Point::new(r.x, r.y), Point::new(r.right(), r.y)),
        (
            Point::new(r.right(), r.y),
            Point::new(r.right(), r.bottom()),
        ),
        (
            Point::new(r.right(), r.bottom()),
            Point::new(r.x, r.bottom()),
        ),
        (Point::new(r.x, r.bottom()), Point::new(r.x, r.y)),
    ];
    for (from, to) in edges {
        let (dx, dy) = (to.x - from.x, to.y - from.y);
        let length = (dx * dx + dy * dy).sqrt();
        let count = (length / (2.0 * dash)).floor().max(1.0) as usize;
        for i in 0..count {
            let t0 = (2 * i) as f32 * dash / length;
            let t1 = ((2 * i + 1) as f32 * dash / length).min(1.0);
            p.line(
                Point::new(from.x + dx * t0, from.y + dy * t0),
                Point::new(from.x + dx * t1, from.y + dy * t1),
                width,
                color,
            );
        }
    }
}
