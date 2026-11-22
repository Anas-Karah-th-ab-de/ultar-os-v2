//! On/off switch.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{size, ColorRole, Radius, Space};

/// A switch; "on" is shown by the knob at the end side and the accent colour.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Switch {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Current state.
    pub on: bool,
    /// Clock value (ms) of the last toggle, for the knob animation.
    pub(crate) toggled_at: Option<u64>,
}

const TRACK_W: f32 = Space::S48.points();
const TRACK_H: f32 = size::ICON;
const KNOB: f32 = size::ICON_SMALL;

impl Switch {
    /// A switch in the given state.
    pub fn new(name: Label, on: bool) -> Switch {
        Switch {
            name,
            on,
            toggled_at: None,
        }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>) -> Size {
        Size::new(TRACK_W, TRACK_H.max(cx.mode.min_target()))
    }

    /// Paints the switch; `position` is the knob position from 0 (off) to 1 (on).
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        position: f32,
    ) {
        let previous = p.set_opacity(v.opacity());
        let track = style::centered(rect, Size::new(TRACK_W, TRACK_H));
        let (track_role, knob_role) = if self.on {
            (ColorRole::Accent, ColorRole::OnAccent)
        } else {
            (ColorRole::ControlBorder, ColorRole::Surface)
        };
        let knob_color = p.color(knob_role);
        let track_color = style::filled(p.color(track_role), knob_color, v);
        p.fill(track, Some(Radius::Round), track_color);
        let inset = (TRACK_H - KNOB) / 2.0;
        let travel = TRACK_W - KNOB - 2.0 * inset;
        let from_start = inset + travel * position.clamp(0.0, 1.0);
        let x = if p.env().direction.is_rtl() {
            track.right() - from_start - KNOB
        } else {
            track.x + from_start
        };
        let knob = Rect::new(x, track.y + inset, KNOB, KNOB);
        p.fill(knob, Some(Radius::Round), knob_color);
        if v.loading {
            let ring = p.color(track_role);
            p.spinner(knob, cx.spin, ring);
        }
        if v.focused {
            style::focus_ring(p, track, Radius::Round);
        }
        p.set_opacity(previous);
    }
}
