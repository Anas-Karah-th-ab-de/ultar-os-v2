//! Checkbox: an independent yes/no choice with its label (boards: `checkrow`).

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Point, Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, ColorRole, Radius, Space};

/// A checkbox. "Checked" is shown by the check mark, "mixed" by a dash – never by colour
/// alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    /// The label (also the accessible name).
    pub label: Label,
    /// Current state.
    pub checked: bool,
    /// Neither checked nor unchecked (some of the things it stands for are on). Activating a
    /// mixed checkbox checks it.
    pub mixed: bool,
}

const BOX: f32 = size::BOX;
const GAP: Space = Space::S12;
/// Height of a check row (boards: 48).
const ROW: f32 = Space::S48.points();

impl Checkbox {
    /// A checkbox in the given state.
    pub fn new(label: Label, checked: bool) -> Checkbox {
        Checkbox {
            label,
            checked,
            mixed: false,
        }
    }

    /// The same checkbox in the mixed state.
    pub fn mixed(mut self) -> Checkbox {
        self.mixed = true;
        self
    }

    /// Flips the state (a mixed box becomes checked); returns the new state.
    pub(crate) fn toggle(&mut self) -> bool {
        self.checked = self.mixed || !self.checked;
        self.mixed = false;
        self.checked
    }

    /// The row part of the node (an error line may follow below it).
    pub(crate) fn row(rect: Rect, cx_min: f32) -> Rect {
        Rect::new(rect.x, rect.y, rect.w, ROW.max(cx_min).min(rect.h))
    }

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
    ) -> Size {
        let spec = style::CONTROL_TEXT.weight(Weight::Medium);
        let text = cx.text_size(&self.label, spec, None);
        let natural = BOX + 3.0 * GAP.points() + text.w;
        let width = natural.min(avail).max(BOX);
        let row = ROW.max(cx.mode.min_target());
        Size::new(width, row + style::error_height(cx, error, width))
    }

    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        error: Option<&Label>,
    ) {
        let previous = p.set_opacity(v.opacity());
        let row = Checkbox::row(rect, cx.mode.min_target());
        if v.hovered || v.pressed {
            let bg = p.color(style::neutral_background(v));
            p.fill(row, Some(Radius::Control), bg);
        }
        let direction = p.env().direction;
        // Boards: padding 12 at both ends, so the focus ring never touches box or label.
        let mut inner = style::inset_inline(row, GAP);
        let slot = style::take_start(&mut inner, BOX, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        let mark = style::centered(slot, Size::new(BOX, BOX));
        if v.loading {
            let muted = p.color(ColorRole::TextMuted);
            p.spinner(mark, cx.spin, muted);
        } else {
            self.paint_box(p, mark, v);
        }
        let color = p.color(ColorRole::Text);
        let spec = style::CONTROL_TEXT.weight(Weight::Medium);
        style::draw_label(p, cx, &self.label, spec, inner, TextAlign::Start, color);
        style::paint_error(p, cx, error, rect, row.h);
        if v.focused {
            style::focus_ring(p, row, Radius::Control);
        }
        p.set_opacity(previous);
    }

    /// The box: accent fill with an `accent_text` edge and a dark mark when checked or
    /// mixed; a `control_border` outline when unchecked (red with an error).
    fn paint_box(&self, p: &mut Painter<'_>, mark: Rect, v: &Visual) {
        let border = p.hairline();
        if self.checked || self.mixed {
            let on = p.color(ColorRole::OnAccent);
            let fill = style::filled(p.color(ColorRole::Accent), on, v);
            p.fill(mark, Some(Radius::Control), fill);
            let edge = p.color(if v.error {
                ColorRole::Danger
            } else {
                ColorRole::AccentText
            });
            p.stroke(mark, Some(Radius::Control), border, edge);
            if self.mixed {
                let middle = mark.center();
                let half = Space::S8.points() / 2.0 + Space::S4.points() / 2.0;
                let (from, to) = (
                    Point::new(middle.x - half, middle.y),
                    Point::new(middle.x + half, middle.y),
                );
                p.line(from, to, size::STROKE, on);
            } else {
                p.icon(
                    IconId::Check,
                    mark,
                    size::ICON_SMALL,
                    QuarterTurns::None,
                    on,
                );
            }
        } else {
            let bg = p.color(style::neutral_background(v));
            p.fill(mark, Some(Radius::Control), bg);
            let edge = p.color(if v.error {
                ColorRole::Danger
            } else {
                ColorRole::ControlBorder
            });
            p.stroke(mark, Some(Radius::Control), border, edge);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mixed_box_becomes_checked() {
        let mut mixed = Checkbox::new(Label::key("c"), false).mixed();
        assert!(mixed.toggle() && mixed.checked && !mixed.mixed);
        assert!(!mixed.toggle());
        let row = Checkbox::row(Rect::new(0.0, 0.0, 100.0, 80.0), 44.0);
        assert_eq!(row.h, ROW);
    }
}
