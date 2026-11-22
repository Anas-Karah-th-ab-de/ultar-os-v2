//! Damage tracking: a small, bounded list of dirty pixel rectangles.

use crate::geom::PixelRect;

/// Most rectangles kept before the list collapses into one bounding box.
pub const MAX_DAMAGE_RECTS: usize = 16;

/// Dirty regions of a surface. Overlapping or touching rectangles are merged; the list never
/// holds more than [`MAX_DAMAGE_RECTS`] entries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Damage {
    rects: Vec<PixelRect>,
}

impl Damage {
    /// An empty damage list.
    pub fn new() -> Damage {
        Damage {
            rects: Vec::with_capacity(MAX_DAMAGE_RECTS),
        }
    }

    /// Marks `rect` dirty. Empty rectangles are ignored.
    pub fn add(&mut self, rect: PixelRect) {
        if rect.is_empty() {
            return;
        }
        let mut merged = rect;
        // Absorb every rectangle that overlaps or touches; repeat because the union may grow.
        loop {
            let before = self.rects.len();
            self.rects.retain(|r| {
                if r.touches(&merged) {
                    merged = merged.union(r);
                    false
                } else {
                    true
                }
            });
            if self.rects.len() == before {
                break;
            }
        }
        if self.rects.len() >= MAX_DAMAGE_RECTS {
            for r in self.rects.drain(..) {
                merged = merged.union(&r);
            }
        }
        self.rects.push(merged);
    }

    /// The dirty rectangles (pairwise disjoint and not touching).
    pub fn rects(&self) -> &[PixelRect] {
        &self.rects
    }

    /// `true` when nothing is dirty.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// The bounding box of all dirty rectangles (empty when nothing is dirty).
    pub fn bounds(&self) -> PixelRect {
        self.rects
            .iter()
            .fold(PixelRect::default(), |acc, r| acc.union(r))
    }

    /// Total dirty area in pixels.
    pub fn area(&self) -> i64 {
        self.rects.iter().map(PixelRect::area).sum()
    }

    /// Forgets all damage.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Returns the current damage and leaves the list empty.
    pub fn take(&mut self) -> Damage {
        Damage {
            rects: std::mem::replace(&mut self.rects, Vec::with_capacity(MAX_DAMAGE_RECTS)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_overlapping_and_touching() {
        let mut d = Damage::new();
        d.add(PixelRect::new(0, 0, 10, 10));
        d.add(PixelRect::new(20, 0, 30, 10));
        assert_eq!(d.rects().len(), 2);
        d.add(PixelRect::new(10, 0, 20, 10));
        assert_eq!(d.rects(), &[PixelRect::new(0, 0, 30, 10)]);
        assert_eq!(d.area(), 300);
    }

    #[test]
    fn ignores_empty_and_takes() {
        let mut d = Damage::new();
        d.add(PixelRect::new(5, 5, 5, 9));
        assert!(d.is_empty());
        d.add(PixelRect::new(1, 1, 2, 2));
        let taken = d.take();
        assert!(d.is_empty());
        assert_eq!(taken.bounds(), PixelRect::new(1, 1, 2, 2));
        d.add(PixelRect::new(1, 1, 2, 2));
        d.clear();
        assert!(d.is_empty() && d.bounds().is_empty());
    }

    #[test]
    fn stays_bounded() {
        let mut d = Damage::new();
        for i in 0..200 {
            d.add(PixelRect::new(i * 4, 0, i * 4 + 2, 2));
            assert!(d.rects().len() <= MAX_DAMAGE_RECTS);
        }
        assert_eq!(d.bounds(), PixelRect::new(0, 0, 199 * 4 + 2, 2));
    }
}
