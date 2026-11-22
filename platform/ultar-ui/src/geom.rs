//! Geometry value types: logical points (`f32`) and device pixels (`i32`).

/// A position in logical points or pixels (context decides).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    /// Horizontal coordinate, growing to the physical right.
    pub x: f32,
    /// Vertical coordinate, growing downwards.
    pub y: f32,
}

impl Point {
    /// A point from its coordinates.
    pub const fn new(x: f32, y: f32) -> Point {
        Point { x, y }
    }
}

/// A width and a height.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Size {
    /// A size from width and height.
    pub const fn new(w: f32, h: f32) -> Size {
        Size { w, h }
    }
}

/// An axis-aligned rectangle with a physical origin (top-left corner).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// A rectangle from origin and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect { x, y, w, h }
    }

    /// Right edge.
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge.
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Centre point.
    pub fn center(&self) -> Point {
        Point::new(self.x + self.w / 2.0, self.y + self.h / 2.0)
    }

    /// `true` when the rectangle covers no area.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// `true` when `p` lies inside (right and bottom edges excluded).
    pub fn contains(&self, p: Point) -> bool {
        p.x >= self.x && p.x < self.right() && p.y >= self.y && p.y < self.bottom()
    }

    /// The rectangle grown by `d` on every side (shrunk for negative `d`, never below zero size).
    pub fn inflate(&self, d: f32) -> Rect {
        let w = (self.w + 2.0 * d).max(0.0);
        let h = (self.h + 2.0 * d).max(0.0);
        Rect::new(self.x - d, self.y - d, w, h)
    }

    /// The common area of two rectangles (empty when they do not overlap).
    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }

    /// `true` when the two rectangles share some area.
    pub fn intersects(&self, other: &Rect) -> bool {
        !self.intersect(other).is_empty()
    }

    /// The rectangle moved by `(dx, dy)`.
    pub fn offset(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }

    /// The rectangle with every coordinate multiplied by `s`.
    pub fn scale(&self, s: f32) -> Rect {
        Rect::new(self.x * s, self.y * s, self.w * s, self.h * s)
    }
}

/// A rectangle in whole device pixels: `x0..x1` × `y0..y1` (end exclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PixelRect {
    /// First column.
    pub x0: i32,
    /// First row.
    pub y0: i32,
    /// One past the last column.
    pub x1: i32,
    /// One past the last row.
    pub y1: i32,
}

impl PixelRect {
    /// A pixel rectangle from its bounds.
    pub const fn new(x0: i32, y0: i32, x1: i32, y1: i32) -> PixelRect {
        PixelRect { x0, y0, x1, y1 }
    }

    /// The smallest pixel rectangle covering `r` (given in pixels).
    pub fn covering(r: &Rect) -> PixelRect {
        PixelRect::new(
            r.x.floor() as i32,
            r.y.floor() as i32,
            r.right().ceil() as i32,
            r.bottom().ceil() as i32,
        )
    }

    /// Width in pixels (0 when empty).
    pub fn width(&self) -> i32 {
        (self.x1 - self.x0).max(0)
    }

    /// Height in pixels (0 when empty).
    pub fn height(&self) -> i32 {
        (self.y1 - self.y0).max(0)
    }

    /// Number of pixels covered.
    pub fn area(&self) -> i64 {
        i64::from(self.width()) * i64::from(self.height())
    }

    /// `true` when no pixel is covered.
    pub fn is_empty(&self) -> bool {
        self.x1 <= self.x0 || self.y1 <= self.y0
    }

    /// The common pixels of both rectangles.
    pub fn intersect(&self, o: &PixelRect) -> PixelRect {
        PixelRect::new(
            self.x0.max(o.x0),
            self.y0.max(o.y0),
            self.x1.min(o.x1),
            self.y1.min(o.y1),
        )
    }

    /// The bounding box of both rectangles; an empty one is ignored.
    pub fn union(&self, o: &PixelRect) -> PixelRect {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        PixelRect::new(
            self.x0.min(o.x0),
            self.y0.min(o.y0),
            self.x1.max(o.x1),
            self.y1.max(o.y1),
        )
    }

    /// `true` when the rectangles overlap or share an edge (so their union wastes nothing).
    pub fn touches(&self, o: &PixelRect) -> bool {
        self.x0 <= o.x1 && o.x0 <= self.x1 && self.y0 <= o.y1 && o.y0 <= self.y1
    }

    /// The same area as a float rectangle.
    pub fn to_rect(&self) -> Rect {
        Rect::new(
            self.x0 as f32,
            self.y0 as f32,
            self.width() as f32,
            self.height() as f32,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_operations() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&b), Rect::new(5.0, 5.0, 5.0, 5.0));
        assert!(a.intersects(&b));
        assert!(!a.intersects(&Rect::new(10.0, 0.0, 5.0, 5.0)));
        assert!(a.contains(Point::new(0.0, 0.0)) && !a.contains(Point::new(10.0, 5.0)));
        assert_eq!(a.inflate(2.0), Rect::new(-2.0, -2.0, 14.0, 14.0));
        assert!(a.inflate(-6.0).is_empty());
        assert_eq!(a.center(), Point::new(5.0, 5.0));
        assert_eq!(
            a.offset(1.0, 2.0).scale(2.0),
            Rect::new(2.0, 4.0, 20.0, 20.0)
        );
    }

    #[test]
    fn pixel_rect_operations() {
        let a = PixelRect::covering(&Rect::new(0.2, 0.2, 9.5, 9.5));
        assert_eq!(a, PixelRect::new(0, 0, 10, 10));
        assert_eq!(a.area(), 100);
        let b = PixelRect::new(10, 0, 20, 10);
        assert!(a.touches(&b) && a.intersect(&b).is_empty());
        assert_eq!(a.union(&b), PixelRect::new(0, 0, 20, 10));
        assert_eq!(a.union(&PixelRect::default()), a);
        assert_eq!(PixelRect::default().union(&b), b);
        assert!(!a.touches(&PixelRect::new(12, 0, 20, 10)));
        assert_eq!(b.to_rect(), Rect::new(10.0, 0.0, 10.0, 10.0));
    }
}
