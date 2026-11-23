//! Icon rasterisation into cached coverage masks.

use super::path::{PathError, Subpath};
use super::{IconId, VIEW_BOX};
use crate::canvas::stroke::stroke_segment;
use crate::geom::Point;
use crate::text::CacheStats;
use std::collections::HashMap;
use std::rc::Rc;

/// Bytes of icon masks kept before the cache is emptied.
pub const ICON_CACHE_BYTES: usize = 512 * 1024;
/// Largest icon edge in pixels that is drawn.
const MAX_ICON_EDGE: f32 = 1024.0;

/// Clockwise rotation of an icon in steps of 90°.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum QuarterTurns {
    /// As drawn.
    #[default]
    None,
    /// 90° clockwise.
    One,
    /// 180°.
    Two,
    /// 270° clockwise.
    Three,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct IconKey {
    icon: IconId,
    size_bits: u32,
    stroke_bits: u32,
    flip: bool,
    turns: QuarterTurns,
}

/// A rasterised icon: coverage of `edge` × `edge` pixels.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct IconMask {
    pub(crate) edge: usize,
    pub(crate) data: Vec<u8>,
}

/// Parsed icon outlines and their rasterised masks (bounded).
pub struct IconCache {
    outlines: HashMap<IconId, Vec<Subpath>>,
    masks: HashMap<IconKey, Rc<IconMask>>,
    bytes: usize,
    hits: u64,
    misses: u64,
}

/// Rotates first, then mirrors: a chevron turned downwards stays downwards when mirrored.
fn transform(p: Point, flip: bool, turns: QuarterTurns) -> Point {
    let p = match turns {
        QuarterTurns::None => p,
        QuarterTurns::One => Point::new(VIEW_BOX - p.y, p.x),
        QuarterTurns::Two => Point::new(VIEW_BOX - p.x, VIEW_BOX - p.y),
        QuarterTurns::Three => Point::new(p.y, VIEW_BOX - p.x),
    };
    if flip {
        Point::new(VIEW_BOX - p.x, p.y)
    } else {
        p
    }
}

impl IconCache {
    /// Parses every icon of the set once.
    pub fn new() -> Result<IconCache, PathError> {
        let mut outlines = HashMap::with_capacity(IconId::ALL.len());
        for icon in IconId::ALL {
            outlines.insert(*icon, icon.outline()?);
        }
        Ok(IconCache {
            outlines,
            masks: HashMap::new(),
            bytes: 0,
            hits: 0,
            misses: 0,
        })
    }

    /// The mask of `icon` at `size` pixels with a stroke of `stroke` pixels.
    pub(crate) fn mask(
        &mut self,
        icon: IconId,
        size: f32,
        stroke: f32,
        flip: bool,
        turns: QuarterTurns,
    ) -> Rc<IconMask> {
        let key = IconKey {
            icon,
            size_bits: size.to_bits(),
            stroke_bits: stroke.to_bits(),
            flip,
            turns,
        };
        if let Some(mask) = self.masks.get(&key) {
            self.hits += 1;
            return Rc::clone(mask);
        }
        self.misses += 1;
        let mask = Rc::new(self.rasterise(icon, size, stroke, flip, turns));
        if self.bytes + mask.data.len() > ICON_CACHE_BYTES {
            self.masks.clear();
            self.bytes = 0;
        }
        self.bytes += mask.data.len();
        self.masks.insert(key, Rc::clone(&mask));
        mask
    }

    fn rasterise(
        &self,
        icon: IconId,
        size: f32,
        stroke: f32,
        flip: bool,
        turns: QuarterTurns,
    ) -> IconMask {
        if !(1.0..=MAX_ICON_EDGE).contains(&size) {
            return IconMask {
                edge: 0,
                data: Vec::new(),
            };
        }
        let edge = size.ceil() as usize;
        let mut data = vec![0u8; edge * edge];
        let scale = size / VIEW_BOX;
        let map = |p: &Point| {
            let t = transform(*p, flip, turns);
            Point::new(t.x * scale, t.y * scale)
        };
        for sub in self.outlines.get(&icon).map_or(&[][..], Vec::as_slice) {
            let count = if sub.closed {
                sub.points.len()
            } else {
                sub.points.len() - 1
            };
            for i in 0..count {
                let a = map(&sub.points[i]);
                let b = map(&sub.points[(i + 1) % sub.points.len()]);
                stroke_segment(&mut data, edge, edge, a, b, stroke / 2.0);
            }
        }
        IconMask { edge, data }
    }

    /// Counters of the mask cache.
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.masks.len(),
            cost: self.bytes,
            hits: self.hits,
            misses: self.misses,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::size;

    /// Mean ink position along the middle row (or middle column when `vertical`).
    fn tip(mask: &IconMask, vertical: bool) -> f32 {
        let mid = mask.edge / 2;
        let at = |i: usize| {
            if vertical {
                mask.data[i * mask.edge + mid]
            } else {
                mask.data[mid * mask.edge + i]
            }
        };
        let weight: f32 = (0..mask.edge).map(|i| f32::from(at(i))).sum();
        let moment: f32 = (0..mask.edge)
            .map(|i| f32::from(at(i)) * (i as f32 + 0.5))
            .sum();
        moment / weight
    }

    fn chevron(cache: &mut IconCache, flip: bool, turns: QuarterTurns) -> Rc<IconMask> {
        cache.mask(IconId::Forward, size::ICON, size::STROKE, flip, turns)
    }

    #[test]
    fn masks_are_cached_and_mirrored() {
        let mut cache = IconCache::new().unwrap();
        let middle = size::ICON / 2.0;
        let plain = chevron(&mut cache, false, QuarterTurns::None);
        assert_eq!(plain.edge, size::ICON as usize);
        let right = tip(&plain, false);
        assert!(
            right > middle + 2.0,
            "the chevron tip is right of the middle: {right}"
        );
        let flipped = chevron(&mut cache, true, QuarterTurns::None);
        let left = tip(&flipped, false);
        assert!(
            (left - (size::ICON - right)).abs() < 0.01,
            "mirrored exactly: {left} {right}"
        );
        let again = chevron(&mut cache, false, QuarterTurns::None);
        assert!(Rc::ptr_eq(&plain, &again));
        assert_eq!((cache.stats().hits, cache.stats().misses), (1, 2));
    }

    #[test]
    fn rotation_happens_before_mirroring() {
        let mut cache = IconCache::new().unwrap();
        let middle = size::ICON / 2.0;
        let down = chevron(&mut cache, false, QuarterTurns::One);
        assert!(
            tip(&down, true) > middle + 2.0,
            "a quarter turn points it down"
        );
        let up = chevron(&mut cache, false, QuarterTurns::Three);
        assert!(tip(&up, true) < middle - 2.0);
        let back = chevron(&mut cache, false, QuarterTurns::Two);
        let flipped = chevron(&mut cache, true, QuarterTurns::None);
        assert_eq!(
            back.data, flipped.data,
            "half a turn of a symmetric chevron equals mirroring"
        );
        let down_mirrored = chevron(&mut cache, true, QuarterTurns::One);
        assert!(
            tip(&down_mirrored, true) > middle + 2.0,
            "mirroring keeps a downward chevron downward"
        );
    }

    #[test]
    fn every_icon_has_ink_at_both_sizes_and_the_cache_is_bounded() {
        let mut cache = IconCache::new().unwrap();
        for icon in IconId::ALL {
            for size in [size::ICON_SMALL, size::ICON] {
                let stroke = size::STROKE * size / size::ICON;
                let mask = cache.mask(*icon, size, stroke, false, QuarterTurns::None);
                // Small dots (`more`, `grid`) may fall between pixels at 16: any coverage
                // counts as ink there.
                let ink = mask.data.iter().filter(|v| **v > 0).count();
                assert!(ink >= 4, "{} at {size}: {ink}", icon.name());
            }
        }
        for step in 0..3000 {
            let size = size::ICON + step as f32 / 16.0;
            cache.mask(IconId::Gear, size, size::STROKE, false, QuarterTurns::Two);
            cache.mask(IconId::Gear, size, size::STROKE, false, QuarterTurns::Three);
            assert!(cache.stats().cost <= ICON_CACHE_BYTES);
        }
        let none = cache.mask(IconId::Gear, 0.0, size::STROKE, false, QuarterTurns::None);
        assert_eq!(none.edge, 0);
        let huge = cache.mask(IconId::Gear, 1.0e7, size::STROKE, false, QuarterTurns::None);
        assert!(huge.data.is_empty());
    }
}
