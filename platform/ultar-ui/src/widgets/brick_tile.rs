//! Painting of the app tile of the bar, the app grid's cell (tile above the name) and the
//! user card of the sign-in screen.

use super::style::{self, Visual};
use super::{Label, PaintCx};
use crate::geom::Rect;
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::TextAlign;
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

const DOT: f32 = Space::S4.points();
const NAME: TextSpec = TextSpec::new(TypeSize::S12);
const STRONG: TextSpec = TextSpec::new(TypeSize::S14).weight(crate::text::Weight::Semibold);

/// The bar's app tile: fill, edge, icon and one dot per window.
pub(super) fn paint_tile(
    p: &mut Painter<'_>,
    rect: Rect,
    v: &Visual,
    (icon, running, active): (IconId, u8, bool),
) {
    let bg = if v.pressed {
        ColorRole::Line
    } else if active || v.hovered {
        ColorRole::Hover
    } else {
        ColorRole::Raised
    };
    let bg = p.color(bg);
    p.fill(rect, Some(Radius::Control), bg);
    let border = p.color(if active {
        ColorRole::ControlBorder
    } else {
        ColorRole::Line
    });
    p.stroke(rect, Some(Radius::Control), p.hairline(), border);
    let gap = Space::S4.points();
    let top = rect.y + (rect.h - size::ICON - gap - DOT) / 2.0;
    let fg = p.color(ColorRole::Text);
    let glyph = Rect::new(rect.x, top, rect.w, size::ICON);
    p.icon(icon, glyph, size::ICON, QuarterTurns::None, fg);
    // One dot per window (at most four), none while only pinned.
    let dots = running.min(4);
    if dots > 0 {
        let n = f32::from(dots);
        let width = n * DOT + (n - 1.0) * gap;
        let dot = p.color(ColorRole::AccentText);
        for i in 0..dots {
            let x = rect.x + (rect.w - width) / 2.0 + f32::from(i) * (DOT + gap);
            let r = Rect::new(x, top + size::ICON + gap, DOT, DOT);
            p.fill(r, Some(Radius::Round), dot);
        }
    }
}

/// The app grid's cell: the tile (touch size, at rest) at the top, the name below.
pub(super) fn paint_cell(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    name: &Label,
    look: (IconId, u8),
) {
    let inset = Space::S8.points();
    let side = size::TARGET_TOUCH_MIN;
    let tile = Rect::new(rect.x + (rect.w - side) / 2.0, rect.y + inset, side, side);
    paint_tile(p, tile, &Visual::default(), (look.0, look.1, false));
    let top = tile.bottom() + inset;
    let below = Rect::new(
        rect.x + Space::S4.points(),
        top,
        rect.w - Space::S8.points(),
        rect.bottom() - top - inset,
    );
    let fg = p.color(ColorRole::Text);
    style::draw_label(p, cx, name, NAME, below, TextAlign::Center, fg);
}

/// The user card: raised, the accent edge and a check mark when chosen; the round picture
/// above the name.
pub(super) fn paint_user(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (name, chosen): (&Label, bool),
) {
    let bg = if v.pressed {
        ColorRole::Line
    } else if chosen || v.hovered {
        ColorRole::Hover
    } else {
        ColorRole::Raised
    };
    let bg = p.color(bg);
    p.fill(rect, Some(Radius::Card), bg);
    let (width, edge) = if chosen {
        (size::FOCUS_RING, ColorRole::AccentText)
    } else {
        (size::BORDER, ColorRole::ControlBorder)
    };
    let edge = p.color(edge);
    p.stroke(rect, Some(Radius::Card), width, edge);
    paint_face(p, cx, rect, (name, chosen));
}

/// The round picture and the name (with the check mark when chosen) of a user card.
fn paint_face(p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, (name, chosen): (&Label, bool)) {
    let side = Space::S48.points();
    let pic = Rect::new(
        rect.x + (rect.w - side) / 2.0,
        rect.y + Space::S12.points(),
        side,
        side,
    );
    let raised = p.color(ColorRole::Raised);
    p.fill(pic, Some(Radius::Round), raised);
    let line = p.color(ColorRole::ControlBorder);
    p.stroke(pic, Some(Radius::Round), p.hairline(), line);
    let fg = p.color(ColorRole::Text);
    p.icon(IconId::User, pic, size::ICON, QuarterTurns::None, fg);
    let top = pic.bottom() + Space::S8.points();
    let row = Rect::new(
        rect.x,
        top,
        rect.w,
        rect.bottom() - top - Space::S8.points(),
    );
    let text = name.resolve(cx.tr);
    let w = p.layout(&text, STRONG, None).width / p.env().scale;
    let mark = if chosen {
        size::ICON_SMALL + Space::S4.points()
    } else {
        0.0
    };
    let start = rect.x + (rect.w - w - mark) / 2.0;
    let rtl = p.env().direction.is_rtl();
    let (text_x, mark_x) = if rtl {
        (start + mark, start)
    } else {
        (start, start + w + Space::S4.points())
    };
    style::draw_label(
        p,
        cx,
        name,
        STRONG,
        Rect::new(text_x, row.y, w, row.h),
        TextAlign::Center,
        fg,
    );
    if chosen {
        let glyph = Rect::new(mark_x, row.y, size::ICON_SMALL, row.h);
        p.icon(
            IconId::Check,
            glyph,
            size::ICON_SMALL,
            QuarterTurns::None,
            fg,
        );
    }
}
