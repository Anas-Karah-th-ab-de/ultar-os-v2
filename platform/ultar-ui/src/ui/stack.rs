//! Overlay layout (`Flex::stack`): every child lies in the same box, the later ones on top
//! (drawn later, hit first). A growing child fills the box; the others keep their natural
//! size and sit where `main` (inline: start, centre, end) and `cross` (block: top, centre,
//! bottom; stretch = full height) put them. Boards: the close button inside the corner of a
//! window card (Overview), a badge on a tile.

use super::{arrange, cross_offset, measure, Inner};
use crate::geom::Size;
use crate::layout::{CrossAlign, Flex, LogicalRect, MainAlign};
use crate::ui::{Node, NodeId};
use crate::widgets::MeasureCx;

/// Size of a stack at `avail` width: the largest child plus the padding.
pub(super) fn measure_stack(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    avail: f32,
) -> Size {
    let inner = (avail - flex.padding.inline_sum()).max(0.0);
    let count = nodes[id.index()].children.len();
    let (mut w, mut h, mut grows) = (0.0f32, 0.0f32, false);
    for n in 0..count {
        let child = nodes[id.index()].children[n];
        grows |= nodes[child.index()].sizing.grow > 0;
        let size = measure(nodes, cx, child, inner);
        w = w.max(size.w);
        h = h.max(size.h);
    }
    let width = if grows {
        avail
    } else {
        w + flex.padding.inline_sum()
    };
    Size::new(width.min(avail.max(0.0)), h + flex.padding.block_sum())
}

/// Inline offset of a child of width `w` in the stack's box.
fn main_offset(align: MainAlign, free: f32) -> f32 {
    match align {
        MainAlign::Start | MainAlign::SpaceBetween => 0.0,
        MainAlign::Center => free / 2.0,
        MainAlign::End => free,
    }
}

/// Places every child of a stack in its box.
pub(super) fn arrange_stack(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    inner: &Inner,
) {
    let count = nodes[id.index()].children.len();
    for n in 0..count {
        let child = nodes[id.index()].children[n];
        let frame = if nodes[child.index()].sizing.grow > 0 {
            LogicalRect::new(inner.start, inner.top, inner.width, inner.height)
        } else {
            let size = measure(nodes, cx, child, inner.width);
            let width = size.w.min(inner.width);
            let height = if flex.cross == CrossAlign::Stretch {
                inner.height
            } else {
                size.h.min(inner.height.max(size.h))
            };
            LogicalRect::new(
                inner.start + main_offset(flex.main, inner.width - width),
                inner.top + cross_offset(flex.cross, inner.height - height),
                width,
                height,
            )
        };
        arrange(nodes, cx, child, frame);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets_follow_the_alignment() {
        assert_eq!(main_offset(MainAlign::Start, 10.0), 0.0);
        assert_eq!(main_offset(MainAlign::SpaceBetween, 10.0), 0.0);
        assert_eq!(main_offset(MainAlign::Center, 10.0), 5.0);
        assert_eq!(main_offset(MainAlign::End, 10.0), 10.0);
    }
}
