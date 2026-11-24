//! Wrapping rows (`Flex::wrap`): children keep their natural size and continue on a new
//! line when the row is full; the gap separates children and lines alike.

use super::{arrange, cross_offset, measure, Inner};
use crate::geom::Size;
use crate::layout::{Flex, LogicalRect};
use crate::ui::{Node, NodeId};
use crate::widgets::MeasureCx;

/// The natural sizes of the children of `id` and the lines they fall into: for each line
/// the range of children and its height.
fn lines(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    gap: f32,
    width: f32,
) -> (Vec<Size>, Vec<(usize, usize, f32)>) {
    let count = nodes[id.index()].children.len();
    let mut sizes = Vec::with_capacity(count);
    let mut lines = Vec::new();
    let (mut first, mut used, mut tallest) = (0usize, 0.0f32, 0.0f32);
    for n in 0..count {
        let child = nodes[id.index()].children[n];
        let size = measure(nodes, cx, child, width);
        let needed = if n == first {
            size.w
        } else {
            used + gap + size.w
        };
        if n > first && needed > width + 0.01 {
            lines.push((first, n, tallest));
            (first, used, tallest) = (n, size.w, size.h);
        } else {
            used = needed;
            tallest = tallest.max(size.h);
        }
        sizes.push(size);
    }
    if count > first {
        lines.push((first, count, tallest));
    }
    (sizes, lines)
}

/// Size of a wrapping row at `avail` width.
pub(super) fn measure_wrap(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    avail: f32,
) -> Size {
    let inner = (avail - flex.padding.inline_sum()).max(0.0);
    let gap = flex.gap_points();
    let (sizes, lines) = lines(nodes, cx, id, gap, inner);
    let widest = lines
        .iter()
        .map(|(a, b, _)| {
            let w: f32 = sizes[*a..*b].iter().map(|s| s.w).sum();
            w + gap * (b - a).saturating_sub(1) as f32
        })
        .fold(0.0f32, f32::max);
    let height: f32 =
        lines.iter().map(|l| l.2).sum::<f32>() + gap * lines.len().saturating_sub(1) as f32;
    Size::new(
        (widest + flex.padding.inline_sum()).min(avail.max(0.0)),
        height + flex.padding.block_sum(),
    )
}

/// Places the children of a wrapping row line by line from the start edge.
pub(super) fn arrange_wrap(
    nodes: &mut [Node],
    cx: &mut MeasureCx<'_>,
    id: NodeId,
    flex: &Flex,
    inner: &Inner,
) {
    let gap = flex.gap_points();
    let (sizes, lines) = lines(nodes, cx, id, gap, inner.width);
    let mut top = inner.top;
    for (first, end, height) in lines {
        let mut start = inner.start;
        for (n, size) in sizes.iter().copied().enumerate().take(end).skip(first) {
            let child = nodes[id.index()].children[n];
            let width = size.w.min(inner.width);
            let y = top + cross_offset(flex.cross, height - size.h);
            arrange(nodes, cx, child, LogicalRect::new(start, y, width, size.h));
            start += width + gap;
        }
        top += height + gap;
    }
}
