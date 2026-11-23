//! Progress bar: determinate (a known share) or indeterminate (busy, share unknown).

use super::style;
use super::{Label, MeasureCx};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{ColorRole, Radius, Space};

/// A progress bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    /// Accessible name (what is in progress).
    pub name: Label,
    /// Share done, 0..=1; `None` while the share is unknown.
    pub value: Option<f32>,
}

const BAR: f32 = Space::S4.points();
const HEIGHT: f32 = Space::S16.points();
const NATURAL_W: f32 = 4.0 * Space::S48.points();
/// Share of the track the moving segment of an indeterminate bar covers.
const SEGMENT: f32 = crate::tokens::state::BUSY_SEGMENT;

/// A share clamped to 0..=1; anything that is not a number counts as 0.
fn share(value: f32) -> f32 {
    if value.is_nan() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

impl Progress {
    /// An indeterminate bar.
    pub fn new(name: Label) -> Progress {
        Progress { name, value: None }
    }

    /// The same bar showing `value` (clamped to 0..=1).
    pub fn value(mut self, value: f32) -> Progress {
        self.value = Some(share(value));
        self
    }

    /// Sets the share (clamped to 0..=1) or makes the bar indeterminate.
    pub fn set(&mut self, value: Option<f32>) {
        self.value = value.map(share);
    }

    /// The share in whole percent, if known.
    pub fn percent(&self) -> Option<u8> {
        self.value.map(|v| (share(v) * 100.0).round() as u8)
    }

    pub(crate) fn measure(&self, _cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        Size::new(NATURAL_W.min(avail), HEIGHT)
    }

    /// Paints the bar; `phase` (0..1) moves the segment of an indeterminate bar.
    pub(crate) fn paint(&self, p: &mut Painter<'_>, rect: Rect, phase: f32) {
        let direction = p.env().direction;
        let track = Rect::new(rect.x, rect.y + (rect.h - BAR) / 2.0, rect.w, BAR);
        // The accent-text role keeps 3:1 against the track in both themes (the gold accent
        // itself does not on light backgrounds).
        let (rest, accent) = (
            p.color(ColorRole::ControlBorder),
            p.color(ColorRole::AccentText),
        );
        p.fill(track, Some(Radius::Round), rest);
        let mut remaining = track;
        let done = match self.value {
            Some(value) => style::take_start(&mut remaining, track.w * share(value), direction),
            None => {
                let saved = p.clip(track);
                style::take_start(&mut remaining, track.w * phase.clamp(0.0, 1.0), direction);
                let segment = style::take_start(&mut remaining, track.w * SEGMENT, direction);
                p.fill(segment, Some(Radius::Round), accent);
                p.restore_clip(saved);
                return;
            }
        };
        if !done.is_empty() {
            p.fill(done, Some(Radius::Round), accent);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_clamped_and_never_nan() {
        let name = || Label::key("p");
        assert_eq!(Progress::new(name()).value, None);
        assert_eq!(Progress::new(name()).percent(), None);
        assert_eq!(Progress::new(name()).value(0.426).percent(), Some(43));
        assert_eq!(Progress::new(name()).value(7.0).value, Some(1.0));
        assert_eq!(Progress::new(name()).value(-1.0).value, Some(0.0));
        assert_eq!(Progress::new(name()).value(f32::NAN).value, Some(0.0));
        let mut bar = Progress::new(name());
        bar.set(Some(f32::INFINITY));
        assert_eq!(bar.percent(), Some(100));
        bar.value = Some(f32::NAN);
        assert_eq!(bar.percent(), Some(0));
        bar.set(None);
        assert_eq!(bar.value, None);
    }
}
