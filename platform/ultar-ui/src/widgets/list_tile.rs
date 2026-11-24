//! A list item drawn as a tile of a grid: a file in a folder or an icon on the desktop
//! (boards "Main": 104 wide, padding 8 / 4, the icon 24 in a 48 box (radius 8, `raised`,
//! `line` border), the name under it at 12, one line; selected = `hover` with a
//! `control_border` border).

use super::style::{self, Visual};
use super::{ListItem, MeasureCx, PaintCx, Thumbnail};
use crate::geom::{Rect, Size};
use crate::icon::QuarterTurns;
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign};
use crate::tokens::{size, state, ColorRole, Radius, Space, TypeSize};

/// Width of a tile (boards: 104).
pub const TILE_WIDTH: f32 = 2.0 * Space::S48.points() + Space::S8.points();
/// Edge of the box around the icon.
const BOX: f32 = Space::S48.points();
const PAD_BLOCK: Space = Space::S8;
const PAD_INLINE: Space = Space::S4;
const GAP: Space = Space::S4;
const CAPTION: TextSpec = TextSpec::new(TypeSize::S12).overflow(Overflow::Ellipsis);

impl ListItem {
    /// The same item as a tile of a grid (put the items of such a list in a wrapping row).
    pub fn tile(mut self) -> ListItem {
        self.tile = true;
        self
    }

    /// The same item showing `picture` in its box instead of the icon.
    pub fn thumbnail(mut self, picture: Thumbnail) -> ListItem {
        self.thumbnail = Some(picture);
        self
    }

    pub(crate) fn measure_tile(&self, cx: &mut MeasureCx<'_>) -> Size {
        let caption = super::list::line_box(cx.env, TypeSize::S12);
        let h = 2.0 * PAD_BLOCK.points() + BOX + GAP.points() + caption;
        Size::new(TILE_WIDTH, h.max(cx.mode.min_target()))
    }

    pub(crate) fn paint_tile(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        if self.selected || v.hovered || v.pressed {
            let bg = p.color(if self.selected {
                ColorRole::Hover
            } else {
                style::neutral_background(v)
            });
            p.fill(rect, Some(Radius::Control), bg);
        }
        if self.selected {
            let border = p.color(ColorRole::ControlBorder);
            p.stroke(rect, Some(Radius::Control), size::BORDER, border);
        }
        let boxed = Rect::new(
            rect.x + (rect.w - BOX) / 2.0,
            rect.y + PAD_BLOCK.points(),
            BOX,
            BOX,
        );
        let raised = p.color(ColorRole::Raised);
        p.fill(boxed, Some(Radius::Control), raised);
        let line = p.color(ColorRole::Line);
        p.stroke(boxed, Some(Radius::Control), size::BORDER, line);
        match (&self.thumbnail, self.icon) {
            (Some(picture), _) => p.image(picture, fit(boxed.inflate(-size::BORDER), picture)),
            (None, Some(icon)) => {
                let color = p.color(ColorRole::Text);
                p.icon(icon, boxed, size::ICON, QuarterTurns::None, color);
            }
            (None, None) => {}
        }
        let text = self.label.resolve(cx.tr);
        let inner_w = rect.w - 2.0 * PAD_INLINE.points();
        let layout = p.layout(&text, CAPTION, Some(inner_w));
        let h = layout.height / p.env().scale;
        let top = boxed.y + BOX + GAP.points();
        let area = Rect::new(rect.x + PAD_INLINE.points(), top, inner_w, h);
        let color = p.color(ColorRole::Text);
        p.text(&layout, area, TextAlign::Center, color);
        if v.focused {
            let ring = p.color(state::FOCUS_RING);
            p.stroke(rect, Some(Radius::Control), size::FOCUS_RING, ring);
        }
        p.set_opacity(previous);
    }
}

/// The largest rectangle of the picture's aspect centred in `area`.
fn fit(area: Rect, picture: &Thumbnail) -> Rect {
    let (w, h) = (picture.width() as f32, picture.height() as f32);
    let k = (area.w / w).min(area.h / h);
    let (fw, fh) = (w * k, h * k);
    Rect::new(
        area.x + (area.w - fw) / 2.0,
        area.y + (area.h - fh) / 2.0,
        fw,
        fh,
    )
}
