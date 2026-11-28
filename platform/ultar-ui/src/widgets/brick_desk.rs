//! The desktop shortcut (`BrickKind::DeskIcon`): the app's tile above its name, the whole
//! cell operated – hover fills it quietly, the selected shortcut keeps the fill and gets the
//! accent edge (like a selected file on the desktop).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::{Painter, TextSpec};
use crate::text::TextAlign;
use crate::tokens::{ColorRole, Radius, Space, TypeSize};

const NAME: TextSpec = TextSpec::new(TypeSize::S12);

/// Lines of the name below the tile.
pub const NAME_LINES: f32 = 2.0;

/// The cell around a tile of `side`: room for the name beside the tile and two lines of it
/// below.
pub(super) fn measure(cx: &mut MeasureCx<'_>, side: u16) -> Size {
    let line = cx.text_size(&Label::data("Ag"), NAME, None).h;
    let side = f32::from(side);
    let w = side + 2.0 * Space::S24.points();
    let h = Space::S8.points() + side + Space::S8.points() + NAME_LINES * line + Space::S8.points();
    Size::new(w, h)
}

/// Paints the cell: the fill and edge of its state, the tile with the icon at half its
/// edge, the name centred below.
pub(super) fn paint(
    p: &mut Painter<'_>,
    cx: &PaintCx<'_>,
    rect: Rect,
    v: &Visual,
    (name, icon, side, selected): (&Label, IconId, u16, bool),
) {
    let fill = if v.pressed {
        Some(ColorRole::Line)
    } else if selected || v.hovered {
        Some(ColorRole::Hover)
    } else {
        None
    };
    if let Some(role) = fill {
        let bg = p.color(role);
        p.fill(rect, Some(Radius::Control), bg);
    }
    if selected {
        let edge = p.color(ColorRole::Accent);
        p.stroke(rect, Some(Radius::Control), p.hairline(), edge);
    }
    let side = f32::from(side);
    let top = rect.y + Space::S8.points();
    let tile = Rect::new(rect.x + (rect.w - side) / 2.0, top, side, side);
    let raised = p.color(ColorRole::Raised);
    p.fill(tile, Some(Radius::Control), raised);
    let line = p.color(ColorRole::Line);
    p.stroke(tile, Some(Radius::Control), p.hairline(), line);
    let fg = p.color(ColorRole::Text);
    p.icon(icon, tile, side / 2.0, QuarterTurns::None, fg);
    let below_top = tile.bottom() + Space::S8.points();
    let below = Rect::new(
        rect.x + Space::S4.points(),
        below_top,
        rect.w - Space::S8.points(),
        (rect.bottom() - below_top - Space::S4.points()).max(0.0),
    );
    style::draw_label(p, cx, name, NAME, below, TextAlign::Center, fg);
}
