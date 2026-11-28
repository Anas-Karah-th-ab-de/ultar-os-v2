//! The selection: a cell, a range, whole rows or whole columns.

use ultar_office::sheet_refs::{MAX_COLS, MAX_ROWS};

/// What the selection spans beyond its corners.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Span {
    /// Just the rectangle between the corners.
    #[default]
    Cells,
    /// Whole rows.
    Rows,
    /// Whole columns.
    Cols,
}

/// A selection: the corner where it started, the corner that moves, and the active cell
/// (the one the formula bar shows and typing goes to; it stays where it was while the range
/// grows, and walks inside the range with Enter and Tab).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sel {
    /// Where the selection began.
    pub anchor: (u32, u32),
    /// The corner that moves with Shift+arrows or a drag.
    pub cur: (u32, u32),
    /// The active cell.
    pub active: (u32, u32),
    /// Rows or columns as a whole.
    pub span: Span,
}

impl Default for Sel {
    fn default() -> Sel {
        Sel::at(0, 0)
    }
}

impl Sel {
    /// One cell.
    pub fn at(r: u32, c: u32) -> Sel {
        Sel { anchor: (r, c), cur: (r, c), active: (r, c), span: Span::Cells }
    }

    /// The rectangle `(row0, col0, row1, col1)`; whole rows or columns reach to the limit.
    pub fn rect(&self) -> (u32, u32, u32, u32) {
        let (r0, r1) = (self.anchor.0.min(self.cur.0), self.anchor.0.max(self.cur.0));
        let (c0, c1) = (self.anchor.1.min(self.cur.1), self.anchor.1.max(self.cur.1));
        match self.span {
            Span::Cells => (r0, c0, r1, c1),
            Span::Rows => (r0, 0, r1, MAX_COLS - 1),
            Span::Cols => (0, c0, MAX_ROWS - 1, c1),
        }
    }

    /// The rectangle cut to the used area `extent` (rows, columns), at least the active cell.
    pub fn used(&self, extent: (u32, u32)) -> (u32, u32, u32, u32) {
        let (r0, c0, r1, c1) = self.rect();
        let r1 = if self.span == Span::Cols { r1.min(extent.0.saturating_sub(1).max(r0)) } else { r1 };
        let c1 = if self.span == Span::Rows { c1.min(extent.1.saturating_sub(1).max(c0)) } else { c1 };
        (r0, c0, r1, c1)
    }

    /// `true` when `(r, c)` is inside.
    pub fn contains(&self, r: u32, c: u32) -> bool {
        let (r0, c0, r1, c1) = self.rect();
        (r0..=r1).contains(&r) && (c0..=c1).contains(&c)
    }

    /// `true` for one cell.
    pub fn single(&self) -> bool {
        self.span == Span::Cells && self.anchor == self.cur
    }

    /// The place to keep on screen: the moving corner while the range grows, else the
    /// active cell.
    pub fn follow(&self) -> (u32, u32) {
        if self.active == self.anchor {
            self.cur
        } else {
            self.active
        }
    }

    /// Selects just `(r, c)`.
    pub fn select(&mut self, r: u32, c: u32) {
        *self = Sel::at(r.min(MAX_ROWS - 1), c.min(MAX_COLS - 1));
    }

    /// Moves by `(dr, dc)`: the active cell, or with `extend` (Shift+arrows) the moving corner.
    pub fn step(&mut self, dr: i64, dc: i64, extend: bool) {
        let base = if extend { self.cur } else { self.active };
        let r = (i64::from(base.0) + dr).clamp(0, i64::from(MAX_ROWS) - 1) as u32;
        let c = (i64::from(base.1) + dc).clamp(0, i64::from(MAX_COLS) - 1) as u32;
        if extend {
            self.cur = (r, c);
        } else {
            self.select(r, c);
        }
    }

    /// Extends to `(r, c)` (drag with the mouse or a finger).
    pub fn extend_to(&mut self, r: u32, c: u32) {
        self.cur = (r.min(MAX_ROWS - 1), c.min(MAX_COLS - 1));
    }

    /// Selects row `r` (extending from the anchor row when `extend`).
    pub fn row(&mut self, r: u32, extend: bool) {
        if extend && self.span == Span::Rows {
            self.cur.0 = r;
        } else {
            *self = Sel { anchor: (r, 0), cur: (r, 0), active: (r, 0), span: Span::Rows };
        }
    }

    /// Selects column `c`.
    pub fn col(&mut self, c: u32, extend: bool) {
        if extend && self.span == Span::Cols {
            self.cur.1 = c;
        } else {
            *self = Sel { anchor: (0, c), cur: (0, c), active: (0, c), span: Span::Cols };
        }
    }

    /// Selects every used cell.
    pub fn all(&mut self, extent: (u32, u32)) {
        *self = Sel {
            anchor: (0, 0),
            cur: (extent.0.saturating_sub(1), extent.1.saturating_sub(1)),
            active: (0, 0),
            span: Span::Cells,
        };
    }

    /// Moves the active cell: Enter (`down`) and Tab, back with Shift. In a range it walks
    /// inside the range and wraps; a single cell just moves.
    pub fn advance(&mut self, down: bool, back: bool) {
        if self.single() {
            let (dr, dc) = if down { (1, 0) } else { (0, 1) };
            let (dr, dc) = if back { (-dr, -dc) } else { (dr, dc) };
            self.step(dr, dc, false);
            return;
        }
        let (r0, c0, r1, c1) = self.rect();
        let (r1, c1) = (r1.min(r0 + 100_000), c1.min(c0 + 16_384));
        let (mut r, mut c) = self.active;
        let sign: i64 = if back { -1 } else { 1 };
        if down {
            let n = i64::from(r) + sign;
            if n < i64::from(r0) || n > i64::from(r1) {
                r = if back { r1 } else { r0 };
                c = if back { if c <= c0 { c1 } else { c - 1 } } else if c >= c1 { c0 } else { c + 1 };
            } else {
                r = n as u32;
            }
        } else {
            let n = i64::from(c) + sign;
            if n < i64::from(c0) || n > i64::from(c1) {
                c = if back { c1 } else { c0 };
                r = if back { if r <= r0 { r1 } else { r - 1 } } else if r >= r1 { r0 } else { r + 1 };
            } else {
                c = n as u32;
            }
        }
        self.active = (r, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rectangles_and_shift_arrows() {
        let mut s = Sel::at(2, 2);
        s.step(1, 1, true);
        s.step(1, 0, true);
        assert_eq!(s.rect(), (2, 2, 4, 3));
        assert_eq!(s.active, (2, 2));
        assert!(s.contains(3, 3) && !s.contains(5, 3));
        // an arrow without Shift moves the active cell and drops the range
        s.step(-10, 0, false);
        assert_eq!(s.active, (0, 2));
        assert!(s.single());
    }

    #[test]
    fn rows_and_columns() {
        let mut s = Sel::default();
        s.row(4, false);
        s.row(6, true);
        assert_eq!(s.rect(), (4, 0, 6, MAX_COLS - 1));
        assert_eq!(s.used((10, 3)), (4, 0, 6, 2));
        s.col(1, false);
        assert_eq!(s.used((5, 3)), (0, 1, 4, 1));
    }

    #[test]
    fn enter_and_tab_walk_inside_a_range_without_changing_it() {
        let mut s = Sel::at(0, 0);
        s.extend_to(1, 1);
        s.advance(true, false);
        assert_eq!(s.active, (1, 0));
        s.advance(true, false);
        assert_eq!(s.active, (0, 1));
        s.advance(false, true);
        assert_eq!(s.active, (0, 0));
        s.advance(false, true);
        assert_eq!(s.active, (1, 1));
        assert_eq!(s.rect(), (0, 0, 1, 1));
    }
}
