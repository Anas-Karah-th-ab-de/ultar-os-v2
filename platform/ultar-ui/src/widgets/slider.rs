//! Slider: a value on a range, growing from the start side to the end side.

use super::style::{self, Visual};
use super::{Label, MeasureCx, PaintCx};
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
    /// Unit written after the value (`Slider::unit`); `None` writes the share of the range
    /// in percent.
    pub unit: Option<Label>,
}

const THUMB: f32 = size::ICON_SMALL;
const TRACK_H: f32 = Space::S4.points();
const NATURAL_W: f32 = 4.0 * Space::S48.points();
/// The value is written at the end edge in a slot of this width (boards: 48, gap 12).
const VALUE_W: f32 = Space::S48.points();
const VALUE_GAP: Space = Space::S12;

impl Slider {
    /// A slider over `min..=max` (swapped if given in the wrong order) with a keyboard step.
    /// Bounds that are not finite numbers become 0; a step that is not finite means
    /// "continuous".
    pub fn new(name: Label, min: f32, max: f32, step: f32) -> Slider {
        let finite = |v: f32| if v.is_finite() { v } else { 0.0 };
        let (min, max) = (finite(min), finite(max));
        let (min, max) = if min <= max { (min, max) } else { (max, min) };
        Slider {
            name,
            value: min,
            min,
            max,
            step: finite(step).abs(),
            unit: None,
        }
    }

    /// The same slider writing its value itself with `unit` after it ("125%", "6500 K",
    /// `Label::data("")` for a bare number) instead of the share of the range. Decimals
    /// follow the step (step 0.5: one decimal; continuous: up to two).
    pub fn unit(mut self, unit: Label) -> Slider {
        self.unit = Some(unit);
        self
    }

    /// The value as written, with the unit resolved by `tr` (digits 0–9, a left-to-right
    /// island in Arabic text).
    pub(crate) fn value_label(&self, tr: &ultar_i18n::Translator) -> String {
        match &self.unit {
            None => self.value_text(),
            Some(unit) => format!("{}{}", self.number(), unit.resolve(tr)),
        }
    }

    /// The value with as many decimals as the step needs (none for whole steps).
    fn number(&self) -> String {
        let step = if self.step > 0.0 { self.step } else { 0.01 };
        let mut decimals = 0usize;
        while decimals < 3 && (step * 10f32.powi(decimals as i32)).fract().abs() > 1e-3 {
            decimals += 1;
        }
        let text = format!("{:.*}", decimals, self.value);
        if decimals > 0 && self.step <= 0.0 {
            text.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            text
        }
    }

    /// The same slider at `value` (clamped and snapped).
    pub fn value(mut self, value: f32) -> Slider {
        self.value = self.snap(value);
        self
    }

    /// `value` clamped to the range and snapped to the step grid. Not-a-number gives `min`.
    /// The public fields may have been set to anything: a reversed or non-finite range and
    /// a non-finite step never panic and never produce a value that is not a number.
    pub fn snap(&self, value: f32) -> f32 {
        let low = if self.min.is_finite() { self.min } else { 0.0 };
        let high = if self.max.is_finite() {
            self.max.max(low)
        } else {
            low
        };
        if value.is_nan() {
            return low;
        }
        let stepped = if self.step.is_finite() && self.step > 0.0 {
            low + ((value - low) / self.step).round() * self.step
        } else {
            value
        };
        if stepped.is_nan() {
            return low;
        }
        stepped.max(low).min(high)
    }

    /// The value as a fraction of the range, 0..=1.
    pub fn fraction(&self) -> f32 {
        let span = self.max - self.min;
        if span > 0.0 && span.is_finite() && self.value.is_finite() {
            ((self.value - self.min) / span).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// The step used by one key press: the step, or a hundredth of the range if continuous.
    pub fn key_step(&self) -> f32 {
        let span = self.max - self.min;
        if self.step.is_finite() && self.step > 0.0 {
            self.step
        } else if span.is_finite() && span > 0.0 {
            span / 100.0
        } else {
            0.0
        }
    }

    /// The part of `rect` the knob moves in: everything but the value slot at the end
    /// edge (the whole rectangle when it is too narrow for both).
    fn slide_area(rect: Rect, direction: Direction) -> Rect {
        let mut area = rect;
        if rect.w >= 2.0 * (VALUE_W + VALUE_GAP.points()) {
            style::take_end(&mut area, VALUE_W + VALUE_GAP.points(), direction);
        }
        area
    }

    /// The value as written beside the track: the share of the range in percent (digits
    /// 0–9, a left-to-right island in Arabic text).
    pub(crate) fn value_text(&self) -> String {
        format!("{}%", (self.fraction() * 100.0).round() as i32)
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
        let track = Slider::track(Slider::slide_area(rect, direction));
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

    pub(crate) fn measure(
        &self,
        cx: &mut MeasureCx<'_>,
        avail: f32,
        error: Option<&Label>,
    ) -> Size {
        let width = NATURAL_W.min(avail);
        let band = THUMB.max(cx.mode.min_target());
        Size::new(width, band + style::error_height(cx, error, width))
    }

    /// The value text in its slot at the end edge of `band` (boards: 12, `text_muted`).
    fn paint_value(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, band: Rect) {
        let mut slot = band;
        let slot = style::take_end(&mut slot, VALUE_W, p.env().direction);
        let muted = p.color(ColorRole::TextMuted);
        let label = Label::data(&self.value_label(cx.tr));
        let align = crate::text::TextAlign::End;
        style::draw_label(p, cx, &label, style::SMALL_TEXT, slot, align, muted);
    }

    /// Paints the slider. Boards: track `control_border` (red with an error), filled part
    /// `accent_text`, knob `text` with an `accent_text` ring; loading shows a progress ring
    /// in place of the knob.
    pub(crate) fn paint(
        &self,
        p: &mut Painter<'_>,
        cx: &PaintCx<'_>,
        rect: Rect,
        v: &Visual,
        error: Option<&Label>,
    ) {
        let previous = p.set_opacity(v.opacity());
        let direction = p.env().direction;
        let band = Rect::new(rect.x, rect.y, rect.w, cx.mode.min_target().min(rect.h));
        let area = Slider::slide_area(band, direction);
        if area.w < band.w {
            self.paint_value(p, cx, band);
        }
        let track = Slider::track(area);
        let rest = p.color(if v.error {
            ColorRole::Danger
        } else {
            ColorRole::ControlBorder
        });
        p.fill(track, Some(Radius::Round), rest);
        let mut remaining = track;
        let filled = style::take_start(&mut remaining, track.w * self.fraction(), direction);
        let accent = p.color(ColorRole::AccentText);
        p.fill(filled, Some(Radius::Round), accent);
        let center_x = if direction.is_rtl() {
            filled.x
        } else {
            filled.right()
        };
        let thumb = Rect::new(
            center_x - THUMB / 2.0,
            band.y + (band.h - THUMB) / 2.0,
            THUMB,
            THUMB,
        );
        if v.hovered || v.pressed {
            let halo = p.color(style::neutral_background(v));
            p.fill(thumb.inflate(Space::S4.points()), Some(Radius::Round), halo);
        }
        if v.loading {
            p.spinner(thumb, cx.spin, accent);
        } else {
            let knob = p.color(ColorRole::Text);
            p.fill(thumb, Some(Radius::Round), knob);
            p.stroke(thumb, Some(Radius::Round), size::FOCUS_RING, accent);
        }
        style::paint_error(p, cx, error, rect, band.h);
        if v.focused {
            // Boards: the ring surrounds the whole slide area (radius 8 + 4), not the thumb.
            style::focus_ring(p, area, Radius::Card);
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
    fn hostile_numbers_never_panic_or_give_nan() {
        let nan = Slider::new(Label::key("x"), f32::NAN, 1.0, 0.1).value(0.5);
        assert_eq!((nan.min, nan.max, nan.value), (0.0, 1.0, 0.5));
        let infinite = Slider::new(Label::key("x"), f32::NEG_INFINITY, f32::INFINITY, 1.0);
        assert_eq!((infinite.min, infinite.max), (0.0, 0.0));
        let step = Slider::new(Label::key("x"), 0.0, 100.0, f32::INFINITY).value(50.0);
        assert_eq!((step.step, step.value, step.key_step()), (0.0, 50.0, 1.0));
        // Fields are public: break them afterwards.
        let mut broken = slider();
        broken.min = 10.0;
        broken.max = -10.0;
        assert_eq!(broken.snap(3.0), 10.0);
        broken.max = f32::NAN;
        broken.step = f32::INFINITY;
        assert_eq!(broken.snap(3.0), 10.0);
        assert_eq!(broken.fraction(), 0.0);
        assert_eq!(broken.key_step(), 0.0);
        broken.min = f32::NAN;
        assert_eq!(broken.snap(f32::INFINITY), 0.0);
        assert_eq!(broken.snap(f32::NAN), 0.0);
    }

    #[test]
    fn pointer_position_maps_to_value_in_both_directions() {
        let s = slider();
        let rect = Rect::new(0.0, 0.0, NATURAL_W, THUMB);
        // The value slot sits at the end edge: the track is the rest.
        let slot = VALUE_W + VALUE_GAP.points();
        let (left, right) = (THUMB / 2.0, NATURAL_W - slot - THUMB / 2.0);
        assert_eq!(s.value_at(rect, left, Direction::Ltr), 0.0);
        assert_eq!(s.value_at(rect, right, Direction::Ltr), 100.0);
        let (left, right) = (slot + THUMB / 2.0, NATURAL_W - THUMB / 2.0);
        assert_eq!(s.value_at(rect, left, Direction::Rtl), 100.0);
        assert_eq!(s.value_at(rect, right, Direction::Rtl), 0.0);
        assert_eq!(
            s.value_at(rect, slot + (NATURAL_W - slot) / 2.0, Direction::Rtl),
            50.0
        );
        assert_eq!(s.value_at(rect, -500.0, Direction::Ltr), 0.0);
        assert_eq!(s.clone().value(30.0).value_text(), "30%");
        let narrow = Rect::new(0.0, 0.0, THUMB / 2.0, THUMB);
        assert_eq!(s.value_at(narrow, 3.0, Direction::Ltr), s.value);
    }
}
