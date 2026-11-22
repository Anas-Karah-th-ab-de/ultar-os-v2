//! The flex distribution along one axis, independent of the widget tree.

use super::MainAlign;

/// Position and extent of one child along the main axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Slot {
    /// Offset from the container's inner start (or top).
    pub offset: f32,
    /// Extent along the main axis.
    pub extent: f32,
}

/// Places children along the main axis.
///
/// `items` holds each child's natural extent and grow factor. Left-over space goes to growing
/// children in proportion to their factors; when nothing grows, `align` decides where the
/// free space goes. When the children do not fit they keep their natural extents and overflow
/// at the end. `out` is cleared and filled with one slot per child.
pub fn distribute(
    available: f32,
    gap: f32,
    align: MainAlign,
    items: &[(f32, u8)],
    out: &mut Vec<Slot>,
) {
    out.clear();
    if items.is_empty() {
        return;
    }
    let gaps = gap * (items.len() - 1) as f32;
    let natural: f32 = items.iter().map(|(extent, _)| extent).sum();
    let grow_total: u32 = items.iter().map(|(_, grow)| u32::from(*grow)).sum();
    let free = (available - natural - gaps).max(0.0);
    let (mut offset, mut between) = (0.0, gap);
    if grow_total == 0 {
        match align {
            MainAlign::Start => {}
            MainAlign::Center => offset = free / 2.0,
            MainAlign::End => offset = free,
            MainAlign::SpaceBetween if items.len() > 1 => {
                between = gap + free / (items.len() - 1) as f32;
            }
            MainAlign::SpaceBetween => {}
        }
    }
    for (extent, grow) in items {
        let extra = if grow_total > 0 {
            free * f32::from(*grow) / grow_total as f32
        } else {
            0.0
        };
        out.push(Slot {
            offset,
            extent: extent + extra,
        });
        offset += extent + extra + between;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(available: f32, gap: f32, align: MainAlign, items: &[(f32, u8)]) -> Vec<(f32, f32)> {
        let mut out = Vec::new();
        distribute(available, gap, align, items, &mut out);
        out.iter().map(|s| (s.offset, s.extent)).collect()
    }

    #[test]
    fn alignment_of_free_space() {
        let items = [(10.0, 0), (20.0, 0)];
        assert_eq!(
            run(100.0, 5.0, MainAlign::Start, &items),
            vec![(0.0, 10.0), (15.0, 20.0)]
        );
        assert_eq!(
            run(100.0, 5.0, MainAlign::End, &items),
            vec![(65.0, 10.0), (80.0, 20.0)]
        );
        assert_eq!(
            run(100.0, 5.0, MainAlign::Center, &items),
            vec![(32.5, 10.0), (47.5, 20.0)]
        );
        assert_eq!(
            run(100.0, 5.0, MainAlign::SpaceBetween, &items),
            vec![(0.0, 10.0), (80.0, 20.0)]
        );
        assert_eq!(
            run(100.0, 5.0, MainAlign::SpaceBetween, &items[..1]),
            vec![(0.0, 10.0)]
        );
        assert!(run(100.0, 5.0, MainAlign::Start, &[]).is_empty());
    }

    #[test]
    fn growing_children_share_by_factor() {
        let slots = run(
            100.0,
            0.0,
            MainAlign::Center,
            &[(10.0, 1), (10.0, 3), (20.0, 0)],
        );
        assert_eq!(slots, vec![(0.0, 25.0), (25.0, 55.0), (80.0, 20.0)]);
    }

    #[test]
    fn overflow_keeps_natural_sizes() {
        let slots = run(20.0, 2.0, MainAlign::End, &[(15.0, 1), (15.0, 0)]);
        assert_eq!(slots, vec![(0.0, 15.0), (17.0, 15.0)]);
    }
}
