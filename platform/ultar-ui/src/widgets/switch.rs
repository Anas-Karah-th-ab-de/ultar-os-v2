//! On/off switch.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{motion, size, ColorRole, Radius, Space};

/// A switch; "on" is shown by the knob at the end side and the accent colour.
#[derive(Debug, Clone, PartialEq)]
pub struct Switch {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Current state.
    pub on: bool,
    /// Clock value (ms) of the last toggle, for the knob animation.
    pub(crate) toggled_at: Option<u64>,
    /// Knob position (0 = off, 1 = on) at that moment: a toggle while the knob still
    /// travels continues from where the knob is.
    pub(crate) from: f32,
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
            from: if on { 1.0 } else { 0.0 },
        }
    }

    /// Where the knob comes to rest: 0 (off) or 1 (on).
    pub(crate) fn target(&self) -> f32 {
        if self.on {
            1.0
        } else {
            0.0
        }
    }

    /// Duration of the current knob travel: the full feedback time for the full way,
    /// proportionally less for a shorter one.
    fn travel_ms(&self) -> f32 {
        motion::FEEDBACK as f32 * (self.target() - self.from).abs()
    }

    /// Knob position at clock `now_ms`, 0 (off) to 1 (on).
    pub(crate) fn position(&self, now_ms: u64) -> f32 {
        let (target, duration) = (self.target(), self.travel_ms());
        match self.toggled_at {
            Some(start) if duration > 0.0 => {
                let progress = (now_ms.saturating_sub(start) as f32 / duration).clamp(0.0, 1.0);
                self.from + (target - self.from) * progress
            }
            _ => target,
        }
    }

    /// `true` when a started knob travel has arrived.
    pub(crate) fn settled(&self, now_ms: u64) -> bool {
        self.toggled_at
            .is_some_and(|start| now_ms.saturating_sub(start) as f32 >= self.travel_ms())
    }

    /// Flips the switch at clock `now_ms`; with `animate` the knob travels from where it is.
    pub(crate) fn toggle(&mut self, now_ms: u64, animate: bool) {
        let here = self.position(now_ms);
        self.on = !self.on;
        self.from = if animate { here } else { self.target() };
        self.toggled_at = animate.then_some(now_ms);
    }

    /// Sets the state without animation.
    pub(crate) fn set(&mut self, on: bool) {
        self.on = on;
        self.from = self.target();
        self.toggled_at = None;
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>) -> Size {
        Size::new(TRACK_W, TRACK_H.max(cx.mode.min_target()))
    }

    /// Boards: an error draws a two-border `danger` edge around the track, on or off.
    fn paint_error_edge(p: &mut Painter<'_>, track: Rect, v: &Visual) {
        if v.error {
            let danger = p.color(ColorRole::Danger);
            p.stroke(track, Some(Radius::Round), 2.0 * p.hairline(), danger);
        }
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
        // Off: an outline in `control_border` with a knob in `text`. On: a track in
        // `accent_text` (3:1 against every background in both themes; the gold `accent`
        // reaches only 2:1 on light ones) with a knob in `surface`.
        let (knob_role, track_role) = if self.on {
            (ColorRole::Surface, ColorRole::AccentText)
        } else {
            (ColorRole::Text, ColorRole::ControlBorder)
        };
        let knob_color = p.color(knob_role);
        let border = p.hairline();
        if self.on {
            let accent = p.color(ColorRole::AccentText);
            let fill = style::filled(accent, knob_color, v);
            p.fill(track, Some(Radius::Round), fill);
            // Boards: the state layer darkens the fill only; the edge stays `accent_text`.
            p.stroke(track, Some(Radius::Round), border, accent);
        } else {
            let bg = p.color(style::neutral_background(v));
            p.fill(track, Some(Radius::Round), bg);
            let edge = p.color(ColorRole::ControlBorder);
            p.stroke(track, Some(Radius::Round), border, edge);
        }
        Switch::paint_error_edge(p, track, v);
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
            // Boards: the ring surrounds the whole control (48 × target), not only the track.
            style::focus_ring(
                p,
                style::centered(rect, Size::new(TRACK_W, rect.h)),
                Radius::Round,
            );
        }
        p.set_opacity(previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_toggle_in_mid_travel_continues_from_the_knob() {
        let full = u64::from(motion::FEEDBACK);
        let mut switch = Switch::new(Label::key("s"), false);
        assert_eq!((switch.position(0), switch.settled(0)), (0.0, false));
        switch.toggle(1000, true);
        assert_eq!(switch.position(1000), 0.0);
        let quarter = switch.position(1000 + full / 4);
        assert!((quarter - 0.25).abs() < 0.01);
        switch.toggle(1000 + full / 4, true);
        assert_eq!(switch.position(1000 + full / 4), quarter, "no jump");
        assert!(!switch.settled(1000 + full / 4));
        // A quarter of the way back takes a quarter of the time.
        assert!(switch.settled(1000 + full / 2));
        assert_eq!(switch.position(1000 + full / 2), 0.0);
        switch.toggle(5000, false);
        assert_eq!(
            (switch.on, switch.position(5000), switch.toggled_at),
            (true, 1.0, None)
        );
        switch.set(false);
        assert_eq!((switch.target(), switch.position(9000)), (0.0, 0.0));
        // A clock that starts below the feedback time must not underflow.
        let mut early = Switch::new(Label::key("s"), true);
        early.toggle(3, true);
        early.toggle(5, true);
        assert!(early.position(5) > 0.9);
    }
}
