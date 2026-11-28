//! Questions to a layout: where the caret is, which character lies under a point, which
//! rectangles a selection covers, where an arrow key or Home/End leads. Positions are in
//! pixels: x from the left edge of the paper, y from the top of the document view.

use super::{Layout, Piece, Placed};
use crate::state::{Cell, Cursor};

/// Where a caret is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CaretPos {
    pub x: f32,
    pub y: f32,
    pub h: f32,
    pub page: usize,
}

/// One rectangle of a selection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SelRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

fn byte_of(text: &str, ch: usize) -> usize {
    text.char_indices().nth(ch).map_or(text.len(), |(i, _)| i)
}

fn char_of(text: &str, byte: usize) -> usize {
    text.char_indices().take_while(|(i, _)| *i < byte).count()
}

/// Distance of the caret from the left edge of a piece at character `ch` of the paragraph.
fn piece_caret(p: &Piece, ch: usize) -> f32 {
    let at = ch.clamp(p.start, p.end) - p.start;
    p.layout
        .as_ref()
        .map_or(if at * 2 >= p.end - p.start { p.w } else { 0.0 }, |l| {
            l.caret_x(byte_of(&p.text, at))
        })
}

fn cell_key(c: Option<Cell>) -> (usize, usize) {
    c.map_or((0, 0), |c| (c.row, c.col))
}

impl Layout {
    /// The placed lines of an item (paragraph or table cell), as a range of `placed`.
    pub fn item_lines(&self, block: usize, cell: Option<Cell>) -> Option<(usize, usize)> {
        self.items
            .get(&(block, cell))
            .copied()
            .filter(|(s, e)| s < e)
    }

    /// The placed line the caret is on (a caret between two soft-wrapped lines belongs to
    /// the second one).
    pub fn line_of(&self, c: &Cursor) -> Option<usize> {
        let (s, e) = self.item_lines(c.block, c.cell)?;
        (s..e)
            .find(|&i| {
                let l = self.placed[i].line();
                c.ch < l.end || (c.ch == l.end && l.hard_end)
            })
            .or(Some(e - 1))
    }

    fn picture(&self, block: usize) -> Option<&super::PicBox> {
        self.pics.iter().find(|p| p.block == block)
    }

    /// The caret rectangle of a position.
    pub fn caret(&self, c: &Cursor) -> Option<CaretPos> {
        if let Some(pic) = self.picture(c.block).filter(|_| c.cell.is_none()) {
            let x = if c.ch == 0 { pic.x } else { pic.x + pic.w };
            return Some(CaretPos {
                x,
                y: pic.y,
                h: pic.h,
                page: pic.page,
            });
        }
        let pl = &self.placed[self.line_of(c)?];
        let line = pl.line();
        let x = pl.x + self.caret_in_line(line, c.ch);
        Some(CaretPos {
            x,
            y: pl.y,
            h: line.h,
            page: pl.page,
        })
    }

    /// Caret x inside a line from the left edge of the column.
    fn caret_in_line(&self, line: &super::LineBox, ch: usize) -> f32 {
        let inside = line.pieces.iter().find(|p| p.start <= ch && ch < p.end);
        let before = || {
            line.pieces
                .iter()
                .filter(|p| p.end <= ch)
                .max_by_key(|p| p.end)
        };
        match inside.or_else(before) {
            Some(p) => p.x + piece_caret(p, ch),
            None => line.start_x,
        }
    }

    /// The position under the point `(x, y)` (clamped into the document).
    pub fn hit(&self, x: f32, y: f32) -> Option<Cursor> {
        for pic in &self.pics {
            if y >= pic.y && y <= pic.y + pic.h {
                let ch = usize::from(x > pic.x + pic.w / 2.0);
                return Some(Cursor {
                    block: pic.block,
                    cell: None,
                    ch,
                });
            }
        }
        let best = self
            .placed
            .iter()
            .enumerate()
            .map(|(i, pl)| (i, self.distance(pl, x, y)))
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))?;
        let pl = &self.placed[best.0];
        Some(Cursor {
            block: pl.block,
            cell: pl.cell,
            ch: self.char_at(pl, x),
        })
    }

    /// A rating of how far a point is from a line (vertical first, then to its cell).
    fn distance(&self, pl: &Placed, x: f32, y: f32) -> f32 {
        let h = pl.line().h;
        let dy = if y < pl.y {
            pl.y - y
        } else if y > pl.y + h {
            y - (pl.y + h)
        } else {
            0.0
        };
        let dx = match (pl.cell, self.tables.iter().find(|t| t.block == pl.block)) {
            (Some(c), Some(t)) => {
                let x0 = t.col_x.get(c.col).copied().unwrap_or(t.x);
                if x < x0 {
                    x0 - x
                } else if x > x0 + t.col_w {
                    x - x0 - t.col_w
                } else {
                    0.0
                }
            }
            _ => 0.0,
        };
        dy * 1000.0 + dx
    }

    fn char_at(&self, pl: &Placed, x: f32) -> usize {
        let line = pl.line();
        let rel = x - pl.x;
        let gap = |p: &Piece| (p.x - rel).max(rel - (p.x + p.w)).max(0.0);
        let nearest = line.pieces.iter().min_by(|a, b| {
            gap(a)
                .partial_cmp(&gap(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let Some(p) = nearest else {
            return line.start;
        };
        let Some(l) = &p.layout else {
            return if rel > p.x + p.w / 2.0 {
                p.end
            } else {
                p.start
            };
        };
        let byte = l.index_at_x((rel - p.x).clamp(0.0, p.w));
        p.start + char_of(&p.text, byte)
    }

    /// The rectangles covered by the selection from `a` to `b` (in document order).
    pub fn selection_rects(&self, a: &Cursor, b: &Cursor) -> Vec<SelRect> {
        let key = |c: &Cursor| (c.block, cell_key(c.cell));
        let mut out = Vec::new();
        for pl in &self.placed {
            let k = (pl.block, cell_key(pl.cell));
            if k < key(a) || k > key(b) {
                continue;
            }
            let line = pl.line();
            let from = if k == key(a) { a.ch } else { 0 };
            let to = if k == key(b) { b.ch } else { usize::MAX };
            if to <= line.start && !(line.start == line.end && to > from) || from > line.end {
                continue;
            }
            let mut any = false;
            for p in line.pieces.iter().filter(|p| p.start < to && p.end > from) {
                let (s, e) = (from.max(p.start), to.min(p.end));
                let (x0, x1) = (p.x + piece_caret(p, s), p.x + piece_caret(p, e));
                out.push(SelRect {
                    x: pl.x + x0.min(x1),
                    y: pl.y,
                    w: (x1 - x0).abs(),
                    h: line.h,
                });
                any = true;
            }
            if !any || (to > line.end && line.pieces.is_empty()) {
                out.push(SelRect {
                    x: pl.x + line.start_x - if line.rtl { 6.0 } else { 0.0 },
                    y: pl.y,
                    w: 6.0,
                    h: line.h,
                });
            }
        }
        out
    }

    /// One line up (`dir < 0`) or down, keeping the horizontal place `x` (from the paper).
    pub fn move_vertical(&self, c: &Cursor, dir: isize, x: f32) -> Option<Cursor> {
        let i = self.line_of(c)?;
        let pl = &self.placed[i];
        let j = if dir > 0 { i + 1 } else { i.checked_sub(1)? };
        let next = self.placed.get(j)?;
        if pl.cell.is_some() || next.cell.is_some() {
            // Inside a table the line below is in the same column: look for it by place.
            let y = if dir > 0 {
                pl.y + pl.line().h + 1.0
            } else {
                pl.y - 1.0
            };
            if let Some(t) = self.hit(x, y).filter(|t| t != c && t.cell.is_some()) {
                return Some(t);
            }
        }
        // Otherwise the neighbouring line in document order (also across the space between
        // paragraphs, where a lookup by place could land on the same line again).
        let ch = self.char_at(next, x);
        Some(Cursor {
            block: next.block,
            cell: next.cell,
            ch,
        })
    }

    /// The start of the visual line the caret is on.
    pub fn line_start(&self, c: &Cursor) -> Option<Cursor> {
        let pl = &self.placed[self.line_of(c)?];
        Some(Cursor {
            ch: pl.line().start,
            ..*c
        })
    }

    /// The end of the line (before its trailing spaces on a wrapped line).
    pub fn line_end(&self, c: &Cursor) -> Option<Cursor> {
        let i = self.line_of(c)?;
        let line = self.placed[i].line();
        let (_, e) = self.item_lines(c.block, c.cell)?;
        let wrapped = i + 1 < e && !line.hard_end;
        let trailing = if wrapped {
            line.pieces
                .iter()
                .max_by_key(|p| p.end)
                .filter(|p| p.space)
                .map_or(0, |p| p.end - p.start)
        } else {
            0
        };
        Some(Cursor {
            ch: line.end - trailing,
            ..*c
        })
    }
}
