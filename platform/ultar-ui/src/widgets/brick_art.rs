//! Pictures of the shell bricks that are not icons of the set: the brand mark (the
//! overview button) and the two-display pictograms of the display modes
//! (`design/boards/gen.py`: `mark`, `picto`). Strokes use the icon stroke width.

use super::style::{self, Visual};
use super::{Label, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

const STRONG: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);

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

/// The brand mark through the icon pipeline (`IconId::Mark`, docs/BRAND.md §3): the
/// `accent_text` colour of the theme, never mirrored.
fn paint_mark(p: &mut Painter<'_>, rect: Rect, points: f32) {
    let color = p.color(ColorRole::AccentText);
    p.icon(IconId::Mark, rect, points, QuarterTurns::None, color);
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

/// A display mode card: the picture above its name, the accent edge and a check mark when
/// chosen.
pub(super) fn paint_mode(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (label, picture, chosen): (&Label, Picture, bool),
) {
    let bg = if chosen || v.hovered {
        ColorRole::Hover
    } else if v.pressed {
        ColorRole::Line
    } else {
        ColorRole::Raised
    };
    let bg = p.color(bg);
    p.fill(rect, Some(Radius::Card), bg);
    let (width, border) = if chosen {
        (size::FOCUS_RING, ColorRole::AccentText)
    } else {
        (size::BORDER, ColorRole::ControlBorder)
    };
    let border = p.color(border);
    p.stroke(rect, Some(Radius::Card), width, border);
    let gap = Space::S12.points();
    let art = Size::new(Space::S48.points(), size::ICON);
    let text = p.layout(&label.resolve(cx.tr), STRONG, None);
    let text_h = text.height / p.env().scale;
    let top = rect.y + (rect.h - art.h - gap - text_h) / 2.0;
    let art_rect = Rect::new(rect.x + (rect.w - art.w) / 2.0, top, art.w, art.h);
    paint_picture(p, picture, art_rect, size::ICON);
    let fg = p.color(ColorRole::Text);
    let text_w = text.width / p.env().scale;
    let mark = if chosen {
        size::ICON_SMALL + Space::S4.points()
    } else {
        0.0
    };
    let line = Rect::new(
        rect.x + (rect.w - text_w - mark) / 2.0,
        top + art.h + gap,
        text_w + mark,
        text_h,
    );
    let mut rest = line;
    let direction = p.env().direction;
    if chosen {
        let glyph = style::take_start(&mut rest, size::ICON_SMALL, direction);
        let check = p.color(ColorRole::AccentText);
        p.icon(
            IconId::Check,
            glyph,
            size::ICON_SMALL,
            QuarterTurns::None,
            check,
        );
        let _ = style::take_start(&mut rest, Space::S4.points(), direction);
    }
    p.text(&text, rest, TextAlign::Start, fg);
}
