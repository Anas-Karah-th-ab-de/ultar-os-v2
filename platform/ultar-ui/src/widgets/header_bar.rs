//! Header bar: the top bar of a window or page – its title in the middle, controls at the
//! start and the end (the children, laid out as a row with the space between them).

use super::{Label, PaintCx};
use crate::geom::Rect;
use crate::layout::{CrossAlign, Flex, MainAlign, Padding};
use crate::paint::{Painter, TextSpec};
use crate::text::{Overflow, TextAlign, Weight};
use crate::tokens::{size, typography, ColorRole, Space};

/// A header bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeaderBar {
    /// The title (also the accessible name).
    pub title: Label,
    /// Layout of the children: start group, end group.
    pub flex: Flex,
}

const TITLE: TextSpec = TextSpec::new(typography::BODY)
    .weight(Weight::Semibold)
    .overflow(Overflow::Ellipsis);

impl HeaderBar {
    /// A header bar with a title.
    pub fn new(title: Label) -> HeaderBar {
        HeaderBar {
            title,
            flex: Flex::row()
                .padding(Padding::symmetric(Space::S8, Space::S4))
                .gap(Space::S8)
                .main(MainAlign::SpaceBetween)
                .cross(CrossAlign::Center),
        }
    }

    /// The height of a header bar with content of `content` height.
    pub(crate) fn height(content: f32) -> f32 {
        content.max(size::TITLEBAR_HEIGHT)
    }

    pub(crate) fn paint(&self, p: &mut Painter<'_>, cx: &PaintCx<'_>, rect: Rect) {
        let (bar, line, border) = (
            p.color(ColorRole::Bar),
            p.color(ColorRole::Line),
            p.hairline(),
        );
        p.fill(rect, None, bar);
        p.fill(
            Rect::new(rect.x, rect.bottom() - border, rect.w, border),
            None,
            line,
        );
        // The title keeps a third of the width on each side free for the controls.
        let middle = Rect::new(rect.x + rect.w / 3.0, rect.y, rect.w / 3.0, rect.h);
        let color = p.color(ColorRole::Text);
        super::style::draw_label(p, cx, &self.title, TITLE, middle, TextAlign::Center, color);
    }
}
