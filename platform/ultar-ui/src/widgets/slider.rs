//! Slider: a value on a range, growing from the start side to the end side.

use super::style::{self, Visual};
use super::{Label, MeasureCx};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{size, ColorRole, Radius, Space};
use ultar_i18n::Direction;

/// A slider.
#[derive(Debug, Clone, PartialEq)]
pub struct Slider {
    /// Accessible name (catalog key).
    pub name: Label,
    /// Current value, within `min..=max`.
    pub value: f32,
    /// Smallest value.
    pub min: f32,
    /// Largest value.
    pub max: f32,
    /// Keyboard step and snapping grid (0 = continuous).
    pub step: f32,
}

const THUMB: f32 = size::ICON_SMALL;
const TRACK_H: f32 = Space::S4.points();
const NATURAL_W: f32 = 4.0 * Space::S48.points();

impl Slider {
    /// A slider over `min..=max` (swapped if given in the wrong order) with a keyboard step.
    pub fn new(name: Label, min: f32, max: f32, step: f32) -> Slider {
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        Slider {
            name,
            value: min,
            min,
            max,
            step: step.abs(),
        }
    }

    /// The same slider at `value` (clamped and snapped).
    pub fn value(mut self, value: f32) -> Slider {
        self.value = self.snap(value);
        self
    }

    /// `value` clamped to the range and snapped to the step grid. Not-a-number gives `min`.
    pub fn snap(&self, value: f32) -> f32 {
        if value.is_nan() {
            return self.min;
        }
        let stepped = if self.step > 0.0 {
            self.min + ((value - self.min) / self.step).round() * self.step
        } else {
            value
        };
        stepped.clamp(self.min, self.max)
    }

    /// The value as a fraction of the range, 0..=1.
    pub fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span > 0.0 {
            ((self.value - self.min) / span).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The step used by one key press: the step, or a hundredth of the range if continuous.
    pub fn key_step(&self) -> f32 {
        if self.step > 0.0 {
            self.step
        } else {
            (self.max - self.min) / 100.0
        }
    }

    fn track(rect: Rect) -> Rect {
        let half = THUMB / 2.0;
        Rect::new(
            rect.x + half,
            rect.y + (rect.h - TRACK_H) / 2.0,
            (rect.w - THUMB).max(0.0),
            TRACK_H,
        )
    }

    /// The value for a pointer at physical `x` inside the slider's `rect`.
    pub(crate) fn value_at(&self, rect: Rect, x: f32, direction: Direction) -> f32 {
        let track = Slider::track(rect);
        if track.w <= 0.0 {
            return self.value;
        }
        let along = ((x - track.x) / track.w).clamp(0.0, 1.0);
        let fraction = if direction.is_rtl() {
            1.0 - along
        } else {
            along
        };
        self.snap(self.min + fraction * (self.max - self.min))
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        Size::new(NATURAL_W.min(avail), THUMB.max(cx.mode.min_target()))
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect, v: &Visual) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        let track = Slider::track(rect);
        let rest = p.color(ColorRole::ControlBorder);
        p.fill(track, Some(Radius::Round), rest);
        let mut remaining = track;
        let filled = style::take_start(&mut remaining, track.w * self.fraction(), direction);
        let accent = p.color(ColorRole::Accent);
        p.fill(filled, Some(Radius::Round), accent);
        let center_x = if direction.is_rtl() {
            filled.x
        } else {
            filled.right()
        };
        let thumb = Rect::new(
            center_x - THUMB / 2.0,
            rect.y + (rect.h - THUMB) / 2.0,
            THUMB,
            THUMB,
        );
        if v.hovered || v.pressed {
            let halo = p.color(style::neutral_background(v));
            p.fill(thumb.inflate(Space::S4.points()), Some(Radius::Round), halo);
        }
        let thumb_color = style::filled(accent, p.color(ColorRole::Text), v);
        p.fill(thumb, Some(Radius::Round), thumb_color);
        if v.focused {
            style::focus_ring(p, thumb, Radius::Round);
        }
        p.set_opacity(previous);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider() -> Slider {
        Slider::new(Label::key("x"), 0.0, 100.0, 5.0)
    }

    #[test]
    fn snapping_clamping_and_fraction() {
        let s = slider().value(52.0);
        assert_eq!(s.value, 50.0);
        assert_eq!(s.fraction(), 0.5);
        assert_eq!(s.snap(-10.0), 0.0);
        assert_eq!(s.snap(1000.0), 100.0);
        assert_eq!(s.snap(f32::NAN), 0.0);
        assert_eq!(s.key_step(), 5.0);
        let continuous = Slider::new(Label::key("x"), 10.0, 0.0, 0.0).value(3.3);
        assert_eq!(
            (continuous.min, continuous.max, continuous.value),
            (0.0, 10.0, 3.3)
        );
        assert_eq!(continuous.key_step(), 0.1);
        let flat = Slider::new(Label::key("x"), 1.0, 1.0, 0.0);
        assert_eq!(flat.fraction(), 0.0);
    }

    #[test]
    fn pointer_position_maps_to_value_in_both_directions() {
        let s = slider();
        let rect = Rect::new(0.0, 0.0, NATURAL_W, THUMB);
        let (left, right) = (THUMB / 2.0, NATURAL_W - THUMB / 2.0);
        assert_eq!(s.value_at(rect, left, Direction::Ltr), 0.0);
        assert_eq!(s.value_at(rect, right, Direction::Ltr), 100.0);
        assert_eq!(s.value_at(rect, left, Direction::Rtl), 100.0);
        assert_eq!(s.value_at(rect, right, Direction::Rtl), 0.0);
        assert_eq!(s.value_at(rect, NATURAL_W / 2.0, Direction::Rtl), 50.0);
        assert_eq!(s.value_at(rect, -500.0, Direction::Ltr), 0.0);
        let narrow = Rect::new(0.0, 0.0, THUMB / 2.0, THUMB);
        assert_eq!(s.value_at(narrow, 3.0, Direction::Ltr), s.value);
    }
}
