//! Tooltip: a short explanation of the widget under the pointer or the keyboard focus.

use super::style;
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{ColorRole, Radius, Space};

const PAD_INLINE: Space = Space::S8;
const PAD_BLOCK: Space = Space::S4;
const GAP: Space = Space::S8;
const MAX_TEXT_W: f32 = 5.0 * Space::S48.points();

/// The rectangle of the tooltip for the widget at `anchor`: below it and centred, above it
/// when only that fits, always inside the surface.
pub(crate) fn tooltip_rect(
    cx: &mut MeasureCx<'_>,
    label: &Label,
    anchor: Rect,
    surface: Size,
) -> Rect {
    let limit = MAX_TEXT_W.min(surface.w - 2.0 * (PAD_INLINE.points() + GAP.points()));
    let text = cx.text_size(label, style::SMALL_TEXT, Some(limit.max(0.0)));
    let size = Size::new(
        text.w + 2.0 * PAD_INLINE.points(),
        text.h + 2.0 * PAD_BLOCK.points(),
    );
    let edge = PAD_BLOCK.points();
    let x = (anchor.center().x - size.w / 2.0)
        .min(surface.w - size.w - edge)
        .max(edge);
    let below = anchor.bottom() + GAP.points();
    let above = anchor.y - GAP.points() - size.h;
    let y = if below + size.h > surface.h && above >= 0.0 {
        above
    } else {
        below
    };
    Rect::new(x, y, size.w, size.h)
}

/// Paints a tooltip into `rect`.
pub(crate) fn paint_tooltip(p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, label: &Label) {
    p.shadow(rect, Some(Radius::Control));
    let (bg, border, hairline) = (
        p.color(ColorRole::Raised),
        p.color(ColorRole::ControlBorder),
        p.hairline(),
    );
    p.fill(rect, Some(Radius::Control), bg);
    p.stroke(rect, Some(Radius::Control), hairline, border);
    let inner = style::inset_inline(rect, PAD_INLINE);
    let color = p.color(ColorRole::Text);
    let spec = style::SMALL_TEXT;
    style::draw_label(p, cx, label, spec, inner, TextAlign::Start, color);
}
