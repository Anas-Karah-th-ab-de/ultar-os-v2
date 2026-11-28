//! Where the rows and columns of the shown part of a sheet lie (in points), and which part of
//! the grid a point hits. Only the rows and columns on screen are ever visited, so a sheet of
//! a million rows costs the same as one of ten.

use ultar_office::sheet::Sheet;

/// Default row height in points, mouse and finger.
pub const ROW_POINTER: f32 = 22.0;
/// Default row height for a finger.
pub const ROW_TOUCH: f32 = 36.0;
/// Narrowest column and lowest row the user can drag to.
pub const MIN_COL: f32 = 12.0;
/// Lowest row.
pub const MIN_ROW: f32 = 14.0;

/// A visible row or column.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seg {
    /// Row or column number.
    pub idx: u32,
    /// Start in points (left edge of a column, top edge of a row), already mirrored for a
    /// right-to-left sheet.
    pub pos: f32,
    /// Extent in points.
    pub size: f32,
    /// One of the frozen rows or columns.
    pub frozen: bool,
}

/// The visible geometry of one frame.
#[derive(Debug, Clone, Default)]
pub struct Frame {
    /// Visible columns, frozen first, then from the first scrolled one.
    pub cols: Vec<Seg>,
    /// Visible rows.
    pub rows: Vec<Seg>,
    /// Width of the row-number column.
    pub hdr_w: f32,
    /// Height of the column-letter row.
    pub hdr_h: f32,
    /// Width of the view.
    pub w: f32,
    /// Height of the view.
    pub h: f32,
    /// Column A at the right.
    pub rtl: bool,
    /// Touch sizes (larger edge handles).
    pub touch: bool,
}

/// What a point of the view lies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The corner (select all).
    Corner,
    /// A column letter.
    Col(u32),
    /// A row number.
    Row(u32),
    /// The border after a column letter (drag to resize).
    ColEdge(u32),
    /// The border below a row number.
    RowEdge(u32),
    /// A cell.
    Cell(u32, u32),
    /// Nothing (beside the last column, below the last row).
    None,
}

/// Width of a column in points (the file stores characters).
pub fn col_points(sh: &Sheet, c: u32) -> f32 {
    sh.width(c) * 7.0 + 5.0
}

/// Stored width for a width in points.
pub fn col_chars(points: f32) -> f32 {
    ((points - 5.0) / 7.0).max(1.0)
}

/// Height of a row in points.
pub fn row_points(sh: &Sheet, r: u32, touch: bool) -> f32 {
    sh.heights
        .get(&r)
        .copied()
        .unwrap_or(if touch { ROW_TOUCH } else { ROW_POINTER })
        .max(MIN_ROW)
}

impl Frame {
    /// The geometry of `sh` seen through a view `w` × `h` points, scrolled to the row and
    /// column `first` (never inside the frozen part).
    pub fn of(sh: &Sheet, first: (u32, u32), w: f32, h: f32, touch: bool) -> Frame {
        let hdr_w = if touch { 52.0 } else { 48.0 };
        let hdr_h = if touch { 36.0 } else { 24.0 };
        let (fr, fc) = (sh.frozen_rows, sh.frozen_cols);
        let (r0, c0) = (first.0.max(fr), first.1.max(fc));
        let rtl = sh.rtl;
        let mut cols = Vec::new();
        let mut x = 0.0;
        let area = (w - hdr_w).max(0.0);
        let mut c = 0;
        while x < area && c < ultar_office::sheet_refs::MAX_COLS {
            if c == fc {
                c = c0;
            }
            let size = col_points(sh, c);
            let pos = if rtl { area - x - size } else { hdr_w + x };
            cols.push(Seg { idx: c, pos, size, frozen: c < fc });
            x += size;
            c += 1;
        }
        let mut rows = Vec::new();
        let mut y = hdr_h;
        let mut r = 0;
        while y < h && r < ultar_office::sheet_refs::MAX_ROWS {
            if r == fr {
                r = r0;
            }
            let size = row_points(sh, r, touch);
            rows.push(Seg { idx: r, pos: y, size, frozen: r < fr });
            y += size;
            r += 1;
        }
        Frame { cols, rows, hdr_w, hdr_h, w, h, rtl, touch }
    }

    /// The column segment of `c`, when visible.
    pub fn col(&self, c: u32) -> Option<Seg> {
        self.cols.iter().copied().find(|s| s.idx == c)
    }

    /// The row segment of `r`, when visible.
    pub fn row(&self, r: u32) -> Option<Seg> {
        self.rows.iter().copied().find(|s| s.idx == r)
    }

    /// `true` when the cell is completely on screen.
    pub fn shows(&self, r: u32, c: u32) -> bool {
        let inside = |s: Seg, lo: f32, hi: f32| s.pos >= lo - 0.5 && s.pos + s.size <= hi + 0.5;
        let col = self.col(c).is_some_and(|s| inside(s, 0.0, self.w));
        let row = self.row(r).is_some_and(|s| inside(s, 0.0, self.h));
        col && row
    }

    /// The rectangle `(x, y, w, h)` of a cell, when visible at all.
    pub fn cell(&self, r: u32, c: u32) -> Option<(f32, f32, f32, f32)> {
        let (col, row) = (self.col(c)?, self.row(r)?);
        Some((col.pos, row.pos, col.size, row.size))
    }

    /// What lies at `(x, y)`.
    pub fn hit(&self, x: f32, y: f32) -> Hit {
        let tol = if self.touch { 12.0 } else { 4.0 };
        let in_hdr_w = if self.rtl { x >= self.w - self.hdr_w } else { x < self.hdr_w };
        if y < self.hdr_h {
            if in_hdr_w {
                return Hit::Corner;
            }
            for s in &self.cols {
                let edge = if self.rtl { s.pos } else { s.pos + s.size };
                if (x - edge).abs() <= tol && !self.edge_taken(x, edge, s) {
                    return Hit::ColEdge(s.idx);
                }
            }
            return self
                .cols
                .iter()
                .find(|s| x >= s.pos && x < s.pos + s.size)
                .map_or(Hit::None, |s| Hit::Col(s.idx));
        }
        if in_hdr_w {
            for s in &self.rows {
                if (y - (s.pos + s.size)).abs() <= tol.min(s.size / 3.0) {
                    return Hit::RowEdge(s.idx);
                }
            }
            return self
                .rows
                .iter()
                .find(|s| y >= s.pos && y < s.pos + s.size)
                .map_or(Hit::None, |s| Hit::Row(s.idx));
        }
        let col = self.cols.iter().find(|s| x >= s.pos && x < s.pos + s.size);
        let row = self.rows.iter().find(|s| y >= s.pos && y < s.pos + s.size);
        match (row, col) {
            (Some(r), Some(c)) => Hit::Cell(r.idx, c.idx),
            _ => Hit::None,
        }
    }

    /// A very narrow column would let its edge swallow the whole letter: the middle third
    /// always selects.
    fn edge_taken(&self, x: f32, _edge: f32, s: &Seg) -> bool {
        s.size < 3.0 * 4.0 && (x - (s.pos + s.size / 2.0)).abs() < s.size / 6.0
    }
}

/// Scrolls `first` so that `(r, c)` is completely visible in a view `w` × `h`.
pub fn reveal(sh: &Sheet, first: &mut (u32, u32), at: (u32, u32), w: f32, h: f32, touch: bool) {
    let (fr, fc) = (sh.frozen_rows, sh.frozen_cols);
    if at.0 >= fr {
        if at.0 < first.0.max(fr) {
            first.0 = at.0;
        } else {
            for _ in 0..64 {
                let f = Frame::of(sh, *first, w, h, touch);
                let fits = f.row(at.0).is_some_and(|s| s.pos + s.size <= h + 0.5);
                if fits || first.0 >= at.0 {
                    break;
                }
                // the row is below: jump towards it, then step
                let last = f.rows.last().map_or(first.0, |s| s.idx);
                first.0 = (first.0 + at.0.saturating_sub(last).max(1)).min(at.0);
            }
        }
    }
    if at.1 >= fc {
        if at.1 < first.1.max(fc) {
            first.1 = at.1;
        } else {
            while first.1 < at.1 {
                let f = Frame::of(sh, *first, w, h, touch);
                let fits = f
                    .col(at.1)
                    .is_some_and(|s| s.pos >= -0.5 && s.pos + s.size <= f.w + 0.5);
                if fits {
                    break;
                }
                first.1 += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet() -> Sheet {
        let mut s = Sheet::new("S");
        s.frozen_rows = 1;
        s.frozen_cols = 1;
        s
    }

    #[test]
    fn frozen_parts_stay_while_scrolling() {
        let s = sheet();
        let f = Frame::of(&s, (500, 20), 600.0, 300.0, false);
        assert_eq!(f.rows[0].idx, 0);
        assert_eq!(f.rows[1].idx, 500);
        assert_eq!(f.cols[0].idx, 0);
        assert_eq!(f.cols[1].idx, 20);
        assert!(f.rows[0].frozen && !f.rows[1].frozen);
    }

    #[test]
    fn a_right_to_left_sheet_starts_at_the_right() {
        let mut s = Sheet::new("S");
        s.rtl = true;
        let f = Frame::of(&s, (0, 0), 600.0, 300.0, false);
        let a = f.col(0).unwrap();
        assert!((a.pos + a.size - (600.0 - f.hdr_w)).abs() < 0.01);
        assert!(f.col(1).unwrap().pos < a.pos);
        // the row numbers are at the right edge
        assert_eq!(f.hit(590.0, 40.0), Hit::Row(0));
        assert_eq!(f.hit(a.pos + 2.0, 40.0), Hit::Cell(0, 0));
    }

    #[test]
    fn hits() {
        let s = Sheet::new("S");
        let f = Frame::of(&s, (0, 0), 600.0, 300.0, false);
        assert_eq!(f.hit(5.0, 5.0), Hit::Corner);
        let c0 = f.col(0).unwrap();
        assert_eq!(f.hit(c0.pos + 10.0, 5.0), Hit::Col(0));
        assert_eq!(f.hit(c0.pos + c0.size - 1.0, 5.0), Hit::ColEdge(0));
        let r1 = f.row(1).unwrap();
        assert_eq!(f.hit(5.0, r1.pos + 10.0), Hit::Row(1));
        assert_eq!(f.hit(5.0, r1.pos + r1.size - 1.0), Hit::RowEdge(1));
        assert_eq!(f.hit(c0.pos + 5.0, r1.pos + 2.0), Hit::Cell(1, 0));
        assert_eq!(f.hit(c0.pos + 5.0, r1.pos + r1.size - 1.0), Hit::Cell(1, 0));
    }

    #[test]
    fn reveal_scrolls_to_a_far_cell_and_back() {
        let s = Sheet::new("S");
        let mut first = (0, 0);
        reveal(&s, &mut first, (99_000, 40), 600.0, 300.0, false);
        let f = Frame::of(&s, first, 600.0, 300.0, false);
        assert!(f.shows(99_000, 40), "{first:?}");
        reveal(&s, &mut first, (3, 0), 600.0, 300.0, false);
        assert_eq!(first, (3, 0));
    }

    #[test]
    fn a_huge_sheet_costs_only_the_visible_part() {
        let s = Sheet::new("S");
        let f = Frame::of(&s, (1_000_000, 16_000), 1000.0, 800.0, false);
        assert!(f.rows.len() < 60 && f.cols.len() < 30);
    }
}
