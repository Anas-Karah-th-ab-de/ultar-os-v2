//! Badge: a short status word or count in a pill.

use super::style;
use super::{Label, MeasureCx, PaintCx, Tone};
use crate::geom::{Rect, Size};
use crate::icon::QuarterTurns;
use crate::paint::{Painter, TextSpec};
use crate::text::{TextAlign, Weight};
use crate::tokens::{size, Radius, Space, TypeSize};

/// A badge. Its tone is carried by the optional icon and by its text, not by colour alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Badge {
    /// The text (also the accessible name).
    pub label: Label,
    /// The meaning.
    pub tone: Tone,
    /// Show the tone's icon before the text.
    pub icon: bool,
}

const PAD: Space = Space::S8;
const HEIGHT: f32 = Space::S24.points();
const ICON_GAP: Space = Space::S4;
const BADGE_TEXT: TextSpec = TextSpec::new(TypeSize::S12).weight(Weight::Semibold);

impl Badge {
    /// A neutral badge.
    pub fn new(label: Label) -> Badge {
        Badge {
            label,
            tone: Tone::Neutral,
            icon: false,
        }
    }

    /// The same badge with a tone.
    pub fn tone(mut self, tone: Tone) -> Badge {
        self.tone = tone;
        self
    }

    /// The same badge with the tone's icon before the text.
    pub fn with_icon(mut self) -> Badge {
        self.icon = true;
        self
    }

    fn lead(&self) -> f32 {
        if self.icon {
            size::ICON_SMALL + ICON_GAP.points()
        } else {
            0.0
        }
    }

    pub(crate) fn measure(&self, cx: &mut MeasureCx<'_>, avail: f32) -> Size {
        let text = cx.text_size(&self.label, BADGE_TEXT, None);
        let natural = text.w + self.lead() + 2.0 * PAD.points();
        Size::new(natural.min(avail).max(HEIGHT), HEIGHT)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect) {
        // Boards: the control radius, no fill, text and edge in the colour of the tone.
        let (hairline, tone) = (p.hairline(), p.color(self.tone.role()));
        p.stroke(rect, Some(Radius::Control), hairline, tone);
        let direction = p.env().direction;
        let mut inner = style::inset_inline(rect, PAD);
        if self.icon {
            let slot = style::take_start(&mut inner, size::ICON_SMALL, direction);
            style::take_start(&mut inner, ICON_GAP.points(), direction);
            let edge = size::ICON_SMALL;
            p.icon(self.tone.icon(), slot, edge, QuarterTurns::None, tone);
        }
        let color = tone;
        let spec = BADGE_TEXT.overflow(crate::text::Overflow::Ellipsis);
        style::draw_label(p, cx, &self.label, spec, inner, TextAlign::Center, color);
    }
}
