//! Scroll container: content taller than its viewport, moved by wheel, touch drag,
//! scrollbar or keyboard. The focused widget is always scrolled into view.

use super::Label;
use crate::geom::Rect;
use crate::layout::Flex;
use crate::paint::Painter;
use crate::tokens::{ColorRole, Radius, Space};
use ultar_i18n::Direction;

/// A vertical scroll container. Its height comes from its sizing (a fixed height, or a
/// share of the parent's left-over space); its children are laid out at their natural height.
#[derive(Debug, Clone, PartialEq)]
pub struct Scroll {
    /// Layout of the content (a column).
    pub flex: Flex,
    /// Accessible name (catalog key).
    pub name: Option<Label>,
    /// How far the content is moved up, in points.
    pub(crate) offset: f32,
    /// Natural height of the content, known after layout.
    pub(crate) content: f32,
}

/// Thickness of the scrollbar thumb.
const THUMB: Space = Space::S4;
/// Distance of the thumb from the end edge, and half of the strip a pointer can grab.
const EDGE: Space = Space::S4;
/// Shortest thumb.
const THUMB_MIN: Space = Space::S24;
/// Width of the strip at the end edge that takes pointer presses for the scrollbar.
const GRIP: Space = Space::S12;

impl Scroll {
    /// A scroll container with a column of children.
    pub fn new() -> Scroll {
        Scroll {
            flex: Flex::column(),
            name: None,
            offset: 0.0,
            content: 0.0,
        }
    }

    /// The same container with another content layout.
    pub fn flex(mut self, flex: Flex) -> Scroll {
        self.flex = flex;
        self
    }

    /// The same container with an accessible name.
    pub fn name(mut self, name: Label) -> Scroll {
        self.name = Some(name);
        self
    }

    /// The current scroll offset in points (0 = top).
    pub fn offset(&self) -> f32 {
        self.offset
    }

    /// The largest offset for a viewport of `viewport` points height.
    pub fn max_offset(&self, viewport: f32) -> f32 {
        let over = self.content - viewport;
        if over.is_finite() && over > 0.0 {
            over
        } else {
            0.0
        }
    }

    /// `offset` limited to what a viewport of that height can scroll; not-a-number gives 0.
    pub(crate) fn clamp(&self, offset: f32, viewport: f32) -> f32 {
        if offset.is_nan() {
            0.0
        } else {
            offset.max(0.0).min(self.max_offset(viewport))
        }
    }

    /// The strip at the end edge of the viewport that belongs to the scrollbar.
    pub(crate) fn grip(viewport: Rect, direction: Direction) -> Rect {
        let width = GRIP.points().min(viewport.w);
        let x = if direction.is_rtl() {
            viewport.x
        } else {
            viewport.right() - width
        };
        Rect::new(x, viewport.y, width, viewport.h)
    }

    fn thumb_height(&self, viewport: f32) -> f32 {
        (viewport * viewport / self.content.max(1.0))
            .max(THUMB_MIN.points())
            .min(viewport)
    }

    /// The scrollbar thumb; `None` when everything fits.
    pub(crate) fn thumb(&self, viewport: Rect, direction: Direction) -> Option<Rect> {
        let max = self.max_offset(viewport.h);
        if max <= 0.0 {
            return None;
        }
        let height = self.thumb_height(viewport.h);
        let travel = viewport.h - height;
        let top = viewport.y + travel * (self.offset / max).clamp(0.0, 1.0);
        let x = if direction.is_rtl() {
            viewport.x + EDGE.points()
        } else {
            viewport.right() - EDGE.points() - THUMB.points()
        };
        Some(Rect::new(x, top, THUMB.points(), height))
    }

    /// The offset that puts the thumb's top at `top` (physical y).
    pub(crate) fn offset_for_thumb(&self, viewport: Rect, top: f32) -> f32 {
        let travel = viewport.h - self.thumb_height(viewport.h);
        if travel <= 0.0 {
            return 0.0;
        }
        let share = ((top - viewport.y) / travel).clamp(0.0, 1.0);
        share * self.max_offset(viewport.h)
    }

    /// Paints the scrollbar over the content; `active` while hovered or dragged.
    pub(crate) fn paint_bar(&self, p: &mut Painter<'_>, viewport: Rect, active: bool) {
        let direction = p.env().direction;
        if let Some(thumb) = self.thumb(viewport, direction) {
            let role = if active {
                ColorRole::TextMuted
            } else {
                ColorRole::ControlBorder
            };
            let color = p.color(role);
            p.fill(thumb, Some(Radius::Round), color);
        }
    }
}

impl Default for Scroll {
    fn default() -> Scroll {
        Scroll::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tall(content: f32) -> Scroll {
        Scroll {
            content,
            ..Scroll::new()
        }
    }

    #[test]
    fn offsets_are_clamped_and_never_nan() {
        let s = tall(1000.0);
        assert_eq!(s.max_offset(400.0), 600.0);
        assert_eq!(s.clamp(-5.0, 400.0), 0.0);
        assert_eq!(s.clamp(5000.0, 400.0), 600.0);
        assert_eq!(s.clamp(f32::NAN, 400.0), 0.0);
        assert_eq!(s.clamp(f32::INFINITY, 400.0), 600.0);
        assert_eq!(tall(100.0).max_offset(400.0), 0.0);
        assert_eq!(tall(f32::NAN).max_offset(400.0), 0.0);
        assert_eq!(tall(f32::INFINITY).max_offset(400.0), 0.0);
        assert_eq!(Scroll::default().offset(), 0.0);
    }

    #[test]
    fn thumb_travels_with_the_offset_and_back() {
        let view = Rect::new(0.0, 100.0, 300.0, 400.0);
        let mut s = tall(1600.0);
        assert_eq!(tall(300.0).thumb(view, Direction::Ltr), None);
        let top = s.thumb(view, Direction::Ltr).unwrap();
        assert_eq!((top.y, top.h), (100.0, 100.0));
        assert!(top.right() < view.right() && top.x > view.center().x);
        assert!(s.thumb(view, Direction::Rtl).unwrap().x < view.center().x);
        s.offset = s.max_offset(view.h);
        let bottom = s.thumb(view, Direction::Ltr).unwrap();
        assert_eq!(bottom.bottom(), view.bottom());
        assert_eq!(s.offset_for_thumb(view, bottom.y), s.offset);
        assert_eq!(s.offset_for_thumb(view, -1000.0), 0.0);
        assert_eq!(s.offset_for_thumb(view, 1.0e9), 1200.0);
        // A very long content keeps a thumb one can still grab.
        assert_eq!(
            tall(1.0e6).thumb(view, Direction::Ltr).unwrap().h,
            THUMB_MIN.points()
        );
        assert!(Scroll::grip(view, Direction::Ltr).contains(top.center()));
        assert_eq!(Scroll::grip(view, Direction::Rtl).x, view.x);
    }
}
