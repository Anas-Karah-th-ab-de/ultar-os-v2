//! Radio group: exactly one of a few options, all visible.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{size, state, ColorRole, InputMode, Radius, Space};

/// A group of radio options, one below the other. The group is one focus stop; the arrow
/// keys move the choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioGroup {
    /// Accessible name of the group (catalog key).
    pub name: Label,
    /// The options.
    pub options: Vec<Label>,
    /// Index of the chosen option; `None` while nothing is chosen yet.
    pub selected: Option<usize>,
}

const DOT: f32 = size::BOX;
/// Width of the ring of a chosen option (boards: 2).
const RING: f32 = size::FOCUS_RING;
const GAP: Space = Space::S12;

impl RadioGroup {
    /// A group with nothing chosen.
    pub fn new(name: Label, options: Vec<Label>) -> RadioGroup {
        RadioGroup {
            name,
            options,
            selected: None,
        }
    }

    /// The same group with option `index` chosen (ignored when out of range).
    pub fn selected(mut self, index: usize) -> RadioGroup {
        if index < self.options.len() {
            self.selected = Some(index);
        }
        self
    }

    /// The option the keyboard acts on: the chosen one, or the first.
    pub(crate) fn current(&self) -> usize {
        self.selected.unwrap_or(0)
    }

    /// The row of option `index`.
    pub(crate) fn row(&self, rect: Rect, index: usize, mode: InputMode) -> Rect {
        let height = mode.min_target();
        Rect::new(rect.x, rect.y + index as f32 * height, rect.w, height)
    }

    /// The option under `pos`.
    pub(crate) fn part_at(&self, rect: Rect, pos: Point, mode: InputMode) -> Option<usize> {
        (0..self.options.len()).find(|i| self.row(rect, *i, mode).contains(pos))
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let mut widest = 0.0f32;
        for option in &self.options {
            widest = widest.max(cx.text_size(option, style::CONTROL_TEXT, None).w);
        }
        let natural = DOT + GAP.points() + widest;
        let rows = self.options.len().max(1) as f32;
        Size::new(natural.min(avail).max(DOT), rows * cx.mode.min_target())
    }

    /// Paints the group; `parts` are the options under the pointer (hovered, pressed).
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        parts: (Option<usize>, Option<usize>),
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        for (index, option) in self.options.iter().enumerate() {
            let row = self.row(rect, index, cx.mode);
            let mut inner = row;
            let slot = style::take_start(&mut inner, DOT, direction);
            style::take_start(&mut inner, GAP.points(), direction);
            let dot = style::centered(slot, Size::new(DOT, DOT));
            let local = Visual {
                hovered: v.hovered && parts.0 == Some(index),
                pressed: v.pressed && parts.1 == Some(index),
                ..*v
            };
            self.paint_dot(p, dot, &local, self.selected == Some(index));
            let color = p.color(ColorRole::Text);
            let spec = style::CONTROL_TEXT;
            style::draw_label(p, cx, option, spec, inner, TextAlign::Start, color);
            if v.focused && index == self.current() {
                // Inside the row: rows touch, a ring outside would cover the neighbours.
                let ring = p.color(state::FOCUS_RING);
                p.stroke(row, Some(Radius::Control), size::FOCUS_RING, ring);
            }
        }
        p.set_opacity(previous);
    }

    /// Boards: a 2-point `accent_text` ring with an 8-point `accent_text` dot when chosen,
    /// a `control_border` ring otherwise.
    fn paint_dot(&self, p: &mut Painter<'_>, dot: Rect, v: &Visual, chosen: bool) {
        let bg = p.color(style::neutral_background(v));
        p.fill(dot, Some(Radius::Round), bg);
        if chosen {
            let accent = p.color(ColorRole::AccentText);
            p.stroke(dot, Some(Radius::Round), RING, accent);
            let inner = style::centered(dot, Size::new(Space::S8.points(), Space::S8.points()));
            p.fill(inner, Some(Radius::Round), accent);
        } else {
            let ring = p.color(ColorRole::ControlBorder);
            p.stroke(dot, Some(Radius::Round), p.hairline(), ring);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_are_hit_row_by_row() {
        let options = (0..3).map(|i| Label::data(&i.to_string())).collect();
        let group = RadioGroup::new(Label::key("g"), options);
        assert_eq!((group.selected, group.current()), (None, 0));
        let group = group.selected(9).selected(2);
        assert_eq!(group.selected, Some(2));
        let mode = InputMode::Pointer;
        let h = mode.min_target();
        let rect = Rect::new(0.0, 0.0, 4.0 * h, 3.0 * h);
        assert_eq!(group.part_at(rect, Point::new(1.0, 0.0), mode), Some(0));
        assert_eq!(group.part_at(rect, Point::new(1.0, 2.5 * h), mode), Some(2));
        assert_eq!(group.part_at(rect, Point::new(1.0, 3.0 * h), mode), None);
        assert_eq!(group.part_at(rect, Point::new(-1.0, h), mode), None);
    }
}
