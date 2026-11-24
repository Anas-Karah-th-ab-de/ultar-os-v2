//! Painting of the phone-size shell bricks (boards CompactHome, CompactApps, CompactQuick):
//! the app cell of the home screen and the app grid, the status button of the status bar
//! and the grab handle of the gesture bar and of a sheet.

use super::style::{self, Visual};
use super::{Label, PaintCx};
use crate::geom::Rect;
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space, TypeSize};

/// Edge of the icon box of a phone app cell (board: 48).
pub(super) const CELL_ICON_BOX: f32 = Space::S48.points();
/// Height of a phone app cell (board: `min-height: 96px`).
pub(super) const CELL_HEIGHT: f32 = 24.0 * Space::S4.points();
/// Thickness of a grab handle (board: 4).
pub(super) const HANDLE_THICKNESS: f32 = Space::S4.points();
/// Gap between the parts of the status button (board: 8).
pub(super) const STATUS_GAP: f32 = Space::S8.points();

const NAME: TextSpec = TextSpec::new(TypeSize::S12).weight(Weight::Medium);
/// The text of the status button (the battery level).
pub(super) const LEVEL: TextSpec = TextSpec::new(TypeSize::S12);

/// The app cell of a phone: the icon in its box, the name below, centred.
pub(super) fn paint_cell(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    (name, icon): (&Label, IconId),
) {
    let inset = Space::S8.points();
    let side = CELL_ICON_BOX;
    let tile = Rect::new(rect.x + (rect.w - side) / 2.0, rect.y + inset, side, side);
    let bg = p.color(ColorRole::Raised);
    p.fill(tile, Some(Radius::Control), bg);
    let border = p.color(ColorRole::Line);
    p.stroke(tile, Some(Radius::Control), p.hairline(), border);
    let fg = p.color(ColorRole::Text);
    p.icon(icon, tile, size::ICON, QuarterTurns::None, fg);
    let top = tile.bottom() + inset;
    let below = Rect::new(
        rect.x + Space::S4.points(),
        top,
        rect.w - Space::S8.points(),
        (rect.bottom() - top - inset).max(0.0),
    );
    style::draw_label(p, cx, name, NAME, below, TextAlign::Center, fg);
}

/// The grab handle: a round bar of `width` in the middle of its node.
pub(super) fn paint_handle(p: &mut Painter<'_>, rect: Rect, v: &Visual, width: f32) {
    let w = width.min(rect.w);
    let bar = Rect::new(
        rect.x + (rect.w - w) / 2.0,
        rect.y + (rect.h - HANDLE_THICKNESS) / 2.0,
        w,
        HANDLE_THICKNESS,
    );
    let role = if v.hovered || v.pressed {
        ColorRole::Text
    } else {
        ColorRole::TextMuted
    };
    let color = p.color(role);
    p.fill(bar, Some(Radius::Round), color);
}

/// The status button of a phone: the icons in reading order, then the battery level.
pub(super) fn paint_status(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    (icons, level): (&[IconId], Option<&Label>),
) {
    let direction = p.env().direction;
    let mut inner = style::inset_inline(rect, Space::S8);
    let fg = p.color(ColorRole::Text);
    for icon in icons {
        let glyph = style::take_start(&mut inner, size::ICON_SMALL, direction);
        p.icon(*icon, glyph, size::ICON_SMALL, QuarterTurns::None, fg);
        let _ = style::take_start(&mut inner, STATUS_GAP, direction);
    }
    if let Some(level) = level {
        style::draw_label(p, cx, level, LEVEL, inner, TextAlign::Start, fg);
    }
}
