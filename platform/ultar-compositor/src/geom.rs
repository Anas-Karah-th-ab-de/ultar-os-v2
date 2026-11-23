//! Integer rectangles and bounded damage regions.

/// A rectangle with integer coordinates (device pixels or logical points,
/// depending on where it is used). Width and height are never negative.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Rect {
    /// First column.
    pub x: i32,
    /// First row.
    pub y: i32,
    /// Width.
    pub w: i32,
    /// Height.
    pub h: i32,
}

impl Rect {
    /// A rectangle; negative sizes become 0.
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect {
            x,
            y,
            w: if w < 0 { 0 } else { w },
            h: if h < 0 { 0 } else { h },
        }
    }

    /// True if it covers no pixel.
    pub const fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }

    /// One past the last column.
    pub const fn max_x(&self) -> i32 {
        self.x.saturating_add(self.w)
    }

    /// One past the last row.
    pub const fn max_y(&self) -> i32 {
        self.y.saturating_add(self.h)
    }

    /// Area in pixels.
    pub fn area(&self) -> i64 {
        i64::from(self.w.max(0)) * i64::from(self.h.max(0))
    }

    /// The common part, if any.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.max_x().min(other.max_x());
        let y1 = self.max_y().min(other.max_y());
        (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// The smallest rectangle containing both (empty ones are ignored).
    pub fn union(&self, other: &Rect) -> Rect {
        if self.is_empty() {
            return *other;
        }
        if other.is_empty() {
            return *self;
        }
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        Rect::new(
            x0,
            y0,
            self.max_x().max(other.max_x()) - x0,
            self.max_y().max(other.max_y()) - y0,
        )
    }

    /// True if the point lies inside.
    pub const fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.max_x() && y < self.max_y()
    }

    /// True if `other` lies completely inside.
    pub fn contains_rect(&self, other: &Rect) -> bool {
        other.is_empty()
            || (other.x >= self.x
                && other.y >= self.y
                && other.max_x() <= self.max_x()
                && other.max_y() <= self.max_y())
    }

    /// Moved by an offset.
    pub const fn translate(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(
            self.x.saturating_add(dx),
            self.y.saturating_add(dy),
            self.w,
            self.h,
        )
    }

    /// Grown by `d` on every side (shrunk for negative `d`).
    pub const fn inflate(&self, d: i32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d)
    }

    /// Multiplied by `scale` with edges rounded outwards (logical points →
    /// device pixels for damage: never too small).
    pub fn scale_out(&self, scale: f64) -> Rect {
        let x0 = (f64::from(self.x) * scale).floor() as i32;
        let y0 = (f64::from(self.y) * scale).floor() as i32;
        let x1 = (f64::from(self.max_x()) * scale).ceil() as i32;
        let y1 = (f64::from(self.max_y()) * scale).ceil() as i32;
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }

    /// Multiplied by `scale` with edges rounded to the nearest pixel
    /// (placement: neighbours share edges exactly).
    pub fn scale_round(&self, scale: f64) -> Rect {
        let x0 = (f64::from(self.x) * scale).round() as i32;
        let y0 = (f64::from(self.y) * scale).round() as i32;
        let x1 = (f64::from(self.max_x()) * scale).round() as i32;
        let y1 = (f64::from(self.max_y()) * scale).round() as i32;
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
}

/// Most rectangles a region keeps before collapsing to its bounds.
pub const MAX_REGION_RECTS: usize = 32;

/// A set of rectangles (damage). Overlapping or touching additions are
/// merged; beyond [`MAX_REGION_RECTS`] the region becomes its bounding box,
/// so memory and work per frame stay bounded.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Region {
    rects: Vec<Rect>,
}

impl Region {
    /// An empty region.
    pub fn new() -> Region {
        Region::default()
    }

    /// The rectangles (non-overlapping is not guaranteed).
    pub fn rects(&self) -> &[Rect] {
        &self.rects
    }

    /// True if nothing is damaged.
    pub fn is_empty(&self) -> bool {
        self.rects.is_empty()
    }

    /// Bounding box.
    pub fn bounds(&self) -> Rect {
        self.rects
            .iter()
            .fold(Rect::default(), |acc, r| acc.union(r))
    }

    /// Adds a rectangle.
    pub fn add(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        let mut merged = rect;
        loop {
            let touching = self.rects.iter().position(|r| {
                r.inflate(1).intersect(&merged).is_some()
                    && r.union(&merged).area() <= r.area() + merged.area()
            });
            match touching {
                Some(i) => {
                    let r = self.rects.swap_remove(i);
                    merged = merged.union(&r);
                }
                None => break,
            }
        }
        if self.rects.iter().any(|r| r.contains_rect(&merged)) {
            return;
        }
        self.rects.retain(|r| !merged.contains_rect(r));
        self.rects.push(merged);
        if self.rects.len() > MAX_REGION_RECTS {
            let bounds = self.bounds();
            self.rects.clear();
            self.rects.push(bounds);
        }
    }

    /// Clips every rectangle to `clip`.
    pub fn clip(&mut self, clip: &Rect) {
        self.rects = self
            .rects
            .iter()
            .filter_map(|r| r.intersect(clip))
            .collect();
    }

    /// Removes everything.
    pub fn clear(&mut self) {
        self.rects.clear();
    }

    /// Takes the content, leaving the region empty.
    pub fn take(&mut self) -> Region {
        std::mem::take(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_operations() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(5, 5, 10, 10);
        assert_eq!(a.intersect(&b), Some(Rect::new(5, 5, 5, 5)));
        assert_eq!(a.intersect(&Rect::new(10, 0, 5, 5)), None);
        assert_eq!(a.union(&b), Rect::new(0, 0, 15, 15));
        assert_eq!(Rect::default().union(&b), b);
        assert_eq!(b.union(&Rect::default()), b);
        assert!(a.contains(9, 9) && !a.contains(10, 0));
        assert!(a.contains_rect(&Rect::new(1, 1, 2, 2)));
        assert!(!a.contains_rect(&b));
        assert_eq!(Rect::new(0, 0, -5, 3).w, 0);
        assert!(Rect::new(0, 0, 0, 3).is_empty());
        assert_eq!(a.translate(2, -1), Rect::new(2, -1, 10, 10));
        assert_eq!(a.inflate(2), Rect::new(-2, -2, 14, 14));
        assert_eq!(Rect::new(1, 1, 1, 1).scale_out(1.5), Rect::new(1, 1, 2, 2));
        assert_eq!(
            Rect::new(1, 1, 1, 1).scale_round(1.5),
            Rect::new(2, 2, 1, 1)
        );
        assert_eq!(a.area(), 100);
        assert_eq!(Rect::new(i32::MAX - 1, 0, 10, 1).max_x(), i32::MAX);
    }

    #[test]
    fn region_merges_and_stays_bounded() {
        let mut r = Region::new();
        r.add(Rect::new(0, 0, 10, 10));
        r.add(Rect::new(10, 0, 10, 10));
        assert_eq!(r.rects(), &[Rect::new(0, 0, 20, 10)]);
        r.add(Rect::new(2, 2, 2, 2));
        assert_eq!(r.rects().len(), 1);
        r.add(Rect::new(100, 100, 5, 5));
        assert_eq!(r.rects().len(), 2);
        r.add(Rect::default());
        assert_eq!(r.rects().len(), 2);
        for i in 0..100 {
            r.add(Rect::new(i * 20, 500, 2, 2));
        }
        assert!(r.rects().len() <= MAX_REGION_RECTS);
        let bounds = r.bounds();
        assert!(bounds.contains_rect(&Rect::new(0, 0, 20, 10)));
        r.clip(&Rect::new(0, 0, 50, 50));
        assert!(r
            .rects()
            .iter()
            .all(|x| Rect::new(0, 0, 50, 50).contains_rect(x)));
        let taken = r.take();
        assert!(r.is_empty() && !taken.is_empty());
        let mut big = Region::new();
        big.add(Rect::new(5, 5, 2, 2));
        big.add(Rect::new(0, 0, 100, 100));
        assert_eq!(big.rects(), &[Rect::new(0, 0, 100, 100)]);
        big.clear();
        assert!(big.is_empty());
    }
}
