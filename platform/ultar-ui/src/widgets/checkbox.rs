//! Checkbox: an independent yes/no choice with its label.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::icon::{IconId, QuarterTurns};
use crate::paint::Painter;
use crate::text::TextAlign;
use crate::tokens::{size, ColorRole, Radius, Space};

/// A checkbox. "Checked" is shown by the check mark, not by colour alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Checkbox {
    /// The label (also the accessible name).
    pub label: Label,
    /// Current state.
    pub checked: bool,
}

const BOX: f32 = size::ICON_SMALL;
const GAP: Space = Space::S8;

impl Checkbox {
    /// A checkbox in the given state.
    pub fn new(label: Label, checked: bool) -> Checkbox {
        Checkbox { label, checked }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let text = cx.text_size(&self.label, style::CONTROL_TEXT, None);
        let natural = BOX + GAP.points() + text.w;
        Size::new(natural.min(avail).max(BOX), cx.mode.min_target())
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        let mut inner = rect;
        let slot = style::take_start(&mut inner, BOX, direction);
        style::take_start(&mut inner, GAP.points(), direction);
        let mark = style::centered(slot, Size::new(BOX, BOX));
        if self.checked {
            let on = p.color(ColorRole::OnAccent);
            let fill = style::filled(p.color(ColorRole::Accent), on, v);
            p.fill(mark, None, fill);
            p.icon(IconId::Check, mark, BOX, QuarterTurns::None, on);
        } else {
            let bg = p.color(style::neutral_background(v));
            p.fill(mark, None, bg);
            let border = p.color(ColorRole::ControlBorder);
            p.stroke(mark, None, size::STROKE, border);
        }
        let color = p.color(ColorRole::Text);
        let spec = style::CONTROL_TEXT;
        style::draw_label(p, cx, &self.label, spec, inner, TextAlign::Start, color);
        if v.focused {
            style::focus_ring(p, rect, Radius::Control);
        }
        p.set_opacity(previous);
    }
}
