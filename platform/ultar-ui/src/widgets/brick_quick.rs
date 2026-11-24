//! Painting of the quick settings tile's shape and of its "choose" part.

use super::style::{self, Visual};
use crate::color::Color;
use crate::geom::Rect;
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::tokens::{size, ColorRole, Radius, Space};

/// Fill and edge of a quick tile (the shape of the whole tile).
pub(super) fn quick_shape(p: &mut Painter<'_>, shape: Rect, v: &Visual, on: bool) {
    let (base, fg, edge) = if on {
        (
            ColorRole::Accent,
            ColorRole::OnAccent,
            ColorRole::AccentText,
        )
    } else {
        (ColorRole::Raised, ColorRole::Text, ColorRole::ControlBorder)
    };
    let base_color = p.color(base);
    let fill = if on {
        style::filled(base_color, p.color(fg), v)
    } else if v.pressed {
        p.color(ColorRole::Line)
    } else if v.hovered {
        p.color(ColorRole::Hover)
    } else {
        base_color
    };
    p.fill(shape, Some(Radius::Card), fill);
    let edge = p.color(edge);
    p.stroke(shape, Some(Radius::Card), p.hairline(), edge);
}

/// The "choose" part at the end of a quick tile.
pub(super) fn paint_more(p: &mut Painter<'_>, rect: Rect, v: &Visual, on: bool) {
    let direction = p.env().direction;
    let reach = Space::S12.points();
    let mut wide = rect;
    // The shape reaches back under the tile; only this part's pixels are painted.
    let shape = if direction.is_rtl() {
        Rect::new(wide.x, wide.y, wide.w + reach, wide.h)
    } else {
        wide.x -= reach;
        Rect::new(wide.x, wide.y, wide.w + reach, wide.h)
    };
    let saved = p.clip(rect);
    quick_shape(p, shape, v, on);
    p.restore_clip(saved);
    let edge = p.color(if on {
        ColorRole::AccentText
    } else {
        ColorRole::ControlBorder
    });
    let x = if direction.is_rtl() {
        rect.right() - p.hairline() / 2.0
    } else {
        rect.x + p.hairline() / 2.0
    };
    let line = |p: &mut Painter<'_>, color: Color| {
        p.line(
            crate::geom::Point::new(x, rect.y),
            crate::geom::Point::new(x, rect.bottom()),
            p.hairline(),
            color,
        );
    };
    line(p, edge);
    let fg = p.color(if on {
        ColorRole::OnAccent
    } else {
        ColorRole::Text
    });
    p.icon(
        IconId::Forward,
        rect,
        size::ICON_SMALL,
        QuarterTurns::None,
        fg,
    );
}
