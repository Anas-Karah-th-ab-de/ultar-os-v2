//! Logical layout: everything is described with start/end, never left/right. The direction
//! ([`Direction::Rtl`] or [`Direction::Ltr`]) is applied only when a rectangle is resolved
//! for painting or hit-testing, so the same tree serves both directions.

mod flex;

pub use flex::{distribute, Slot};

use crate::geom::Rect;
use crate::tokens::Space;
use ultar_i18n::Direction;

/// Main axis of a flex container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// Children follow each other from start to end.
    Inline,
    /// Children follow each other from top to bottom.
    Block,
}

/// Placement of children along the main axis when space is left over.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MainAlign {
    /// Packed at the start (top for block axis).
    #[default]
    Start,
    /// Centred.
    Center,
    /// Packed at the end.
    End,
    /// First at the start, last at the end, equal space between.
    SpaceBetween,
}

/// Placement of children across the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CrossAlign {
    /// At the start (top for an inline axis).
    Start,
    /// Centred.
    Center,
    /// At the end.
    End,
    /// Filling the container.
    #[default]
    Stretch,
}

/// A size request along one axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// As large as the content needs.
    #[default]
    Auto,
    /// A fixed number of logical points (content sizes such as a panel width; spacing is
    /// never given this way).
    Points(f32),
}

/// Inner spacing of a container, from the spacing scale only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Padding {
    /// Space at the start edge.
    pub start: Option<Space>,
    /// Space at the end edge.
    pub end: Option<Space>,
    /// Space at the top.
    pub top: Option<Space>,
    /// Space at the bottom.
    pub bottom: Option<Space>,
}

fn points(space: Option<Space>) -> f32 {
    space.map_or(0.0, Space::points)
}

impl Padding {
    /// No padding.
    pub const NONE: Padding = Padding {
        start: None,
        end: None,
        top: None,
        bottom: None,
    };

    /// The same step on all four sides.
    pub const fn all(space: Space) -> Padding {
        Padding::symmetric(space, space)
    }

    /// `inline` at start and end, `block` at top and bottom.
    pub const fn symmetric(inline: Space, block: Space) -> Padding {
        Padding {
            start: Some(inline),
            end: Some(inline),
            top: Some(block),
            bottom: Some(block),
        }
    }

    /// Start plus end, in points.
    pub fn inline_sum(&self) -> f32 {
        points(self.start) + points(self.end)
    }

    /// Top plus bottom, in points.
    pub fn block_sum(&self) -> f32 {
        points(self.top) + points(self.bottom)
    }

    /// The start inset in points.
    pub fn start_points(&self) -> f32 {
        points(self.start)
    }

    /// The top inset in points.
    pub fn top_points(&self) -> f32 {
        points(self.top)
    }
}

/// A row or column container.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Flex {
    /// Main axis.
    pub axis: Axis,
    /// Space between children.
    pub gap: Option<Space>,
    /// Inner spacing.
    pub padding: Padding,
    /// Main-axis placement.
    pub main: MainAlign,
    /// Cross-axis placement.
    pub cross: CrossAlign,
    /// A row that runs out of room continues on a new line (the gap also separates the
    /// lines). Only for rows; children keep their natural width (growing is ignored).
    pub wrap: bool,
}

impl Flex {
    /// Children from start to end, centred vertically.
    pub const fn row() -> Flex {
        Flex {
            axis: Axis::Inline,
            gap: None,
            padding: Padding::NONE,
            main: MainAlign::Start,
            cross: CrossAlign::Center,
            wrap: false,
        }
    }

    /// Children from top to bottom, filling the width.
    pub const fn column() -> Flex {
        Flex {
            axis: Axis::Block,
            gap: None,
            padding: Padding::NONE,
            main: MainAlign::Start,
            cross: CrossAlign::Stretch,
            wrap: false,
        }
    }

    /// The same row wrapping onto further lines when its children do not fit
    /// (boards: the account buttons).
    pub const fn wrap(mut self) -> Flex {
        self.wrap = true;
        self
    }

    /// The same container with a gap between children.
    pub const fn gap(mut self, gap: Space) -> Flex {
        self.gap = Some(gap);
        self
    }

    /// The same container with inner spacing.
    pub const fn padding(mut self, padding: Padding) -> Flex {
        self.padding = padding;
        self
    }

    /// The same container with another main-axis placement.
    pub const fn main(mut self, main: MainAlign) -> Flex {
        self.main = main;
        self
    }

    /// The same container with another cross-axis placement.
    pub const fn cross(mut self, cross: CrossAlign) -> Flex {
        self.cross = cross;
        self
    }

    /// The gap in points.
    pub fn gap_points(&self) -> f32 {
        points(self.gap)
    }
}

/// How a node takes part in its parent's layout.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sizing {
    /// Requested width.
    pub width: Length,
    /// Requested height.
    pub height: Length,
    /// Share of the parent's left-over main-axis space (0 = none).
    pub grow: u8,
}

impl Sizing {
    /// Content-sized, not growing.
    pub const AUTO: Sizing = Sizing {
        width: Length::Auto,
        height: Length::Auto,
        grow: 0,
    };

    /// Takes a share of the left-over main-axis space.
    pub const fn grow(factor: u8) -> Sizing {
        Sizing {
            width: Length::Auto,
            height: Length::Auto,
            grow: factor,
        }
    }

    /// A fixed width in logical points.
    pub const fn width(points: f32) -> Sizing {
        Sizing {
            width: Length::Points(points),
            height: Length::Auto,
            grow: 0,
        }
    }

    /// The same sizing with a fixed height in logical points.
    pub const fn with_height(mut self, points: f32) -> Sizing {
        self.height = Length::Points(points);
        self
    }
}

/// A rectangle whose horizontal position is measured from the start edge of the surface.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LogicalRect {
    /// Distance of the rectangle's start edge from the surface's start edge.
    pub start: f32,
    /// Distance from the top.
    pub top: f32,
    /// Extent along the inline axis.
    pub width: f32,
    /// Extent along the block axis.
    pub height: f32,
}

impl LogicalRect {
    /// A logical rectangle from its parts.
    pub const fn new(start: f32, top: f32, width: f32, height: f32) -> LogicalRect {
        LogicalRect {
            start,
            top,
            width,
            height,
        }
    }

    /// The physical rectangle on a surface of `surface_width` for a direction.
    pub fn resolve(&self, direction: Direction, surface_width: f32) -> Rect {
        let x = if direction.is_rtl() {
            surface_width - self.start - self.width
        } else {
            self.start
        };
        Rect::new(x, self.top, self.width, self.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_rect_mirrors_in_rtl() {
        let r = LogicalRect::new(
            Space::S16.points(),
            Space::S8.points(),
            Space::S48.points(),
            Space::S24.points(),
        );
        let width = Space::S48.points() * 4.0;
        let ltr = r.resolve(Direction::Ltr, width);
        let rtl = r.resolve(Direction::Rtl, width);
        assert_eq!(ltr.x, Space::S16.points());
        assert_eq!(rtl.right(), width - Space::S16.points());
        assert_eq!((ltr.y, ltr.w, ltr.h), (rtl.y, rtl.w, rtl.h));
        assert_eq!(
            ltr.x + ltr.w / 2.0,
            width - (rtl.x + rtl.w / 2.0),
            "mirrored about the centre"
        );
    }

    #[test]
    fn padding_and_builders() {
        let p = Padding::symmetric(Space::S16, Space::S8);
        assert_eq!(p.inline_sum(), 2.0 * Space::S16.points());
        assert_eq!(p.block_sum(), 2.0 * Space::S8.points());
        assert_eq!(Padding::NONE.inline_sum() + Padding::NONE.block_sum(), 0.0);
        assert_eq!(Padding::all(Space::S4).top_points(), Space::S4.points());
        assert_eq!(Padding::all(Space::S4).start_points(), Space::S4.points());
        let flex = Flex::row()
            .gap(Space::S8)
            .padding(p)
            .main(MainAlign::End)
            .cross(CrossAlign::Start);
        assert_eq!(
            (flex.axis, flex.gap_points()),
            (Axis::Inline, Space::S8.points())
        );
        assert_eq!((flex.main, flex.cross), (MainAlign::End, CrossAlign::Start));
        assert_eq!(Flex::column().axis, Axis::Block);
        assert_eq!(Sizing::grow(2).grow, 2);
        let fixed = Sizing::width(Space::S48.points()).with_height(Space::S24.points());
        assert_eq!(fixed.width, Length::Points(Space::S48.points()));
        assert_eq!(fixed.height, Length::Points(Space::S24.points()));
        assert_eq!(Sizing::AUTO, Sizing::default());
    }
}
