//! Painting of the quick settings tile's shape and of its "choose" part.

use super::style::{self, Visual};
use super::{Label, PaintCx};
use crate::color::Color;
use crate::geom::Rect;
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

const SMALL: TextSpec = TextSpec::new(TypeSize::S12);
const STRONG: TextSpec = TextSpec::new(TypeSize::S14).weight(Weight::Semibold);

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

/// The quick settings tile: icon, name and state line on the tile's shape.
pub(super) fn paint_quick(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (label, icon, state, on, joined): (&Label, IconId, &Label, bool, bool),
) {
    let direction = p.env().direction;
    let body = rect;
    let reach = if joined { Space::S12.points() } else { 0.0 };
    // The shape reaches under the "choose" part at the end, which paints its own end.
    let shape = if direction.is_rtl() {
        Rect::new(rect.x - reach, rect.y, rect.w + reach, rect.h)
    } else {
        Rect::new(rect.x, rect.y, rect.w + reach, rect.h)
    };
    let saved = p.clip(rect);
    quick_shape(p, shape, v, on);
    p.restore_clip(saved);
    let fg = p.color(if on {
        ColorRole::OnAccent
    } else {
        ColorRole::Text
    });
    let mut inner = style::inset_inline(body, Space::S12);
    let glyph = style::take_start(&mut inner, size::ICON, direction);
    p.icon(icon, glyph, size::ICON, QuarterTurns::None, fg);
    let _ = style::take_start(&mut inner, Space::S12.points(), direction);
    let name = p
        .layout(&label.resolve(cx.tr), STRONG, Some(inner.w))
        .height;
    let sub = p.layout(&state.resolve(cx.tr), SMALL, Some(inner.w)).height;
    let s = p.env().scale;
    let top = inner.y + (inner.h - (name + sub) / s) / 2.0;
    let first = Rect::new(inner.x, top, inner.w, name / s);
    style::draw_label(p, cx, label, STRONG, first, TextAlign::Start, fg);
    let second = Rect::new(inner.x, top + name / s, inner.w, sub / s);
    style::draw_label(p, cx, state, SMALL, second, TextAlign::Start, fg);
}
