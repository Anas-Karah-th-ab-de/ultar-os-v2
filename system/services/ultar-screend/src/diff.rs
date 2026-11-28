//! Which rectangles of the screen changed since the viewer last got them.
//!
//! The screen is cut into tiles of [`TILE`] pixels. A tile is *known* once its content was
//! handed to the viewer (the mesh took the rectangle); it is *changed* while it is unknown
//! or differs from what the viewer has. Changed tiles of one row are joined to rectangles
//! of at most [`MAX_RUN`] tiles, so a rectangle never has more than `64 * 256` pixels (the
//! limit of one mesh frame). A rectangle the mesh did not take (slow link) is simply still
//! changed on the next look: nothing is queued, nothing is lost.

/// Side of a tile in pixels.
pub const TILE: u32 = 64;
/// Most tiles of one row joined to one rectangle.
pub const MAX_RUN: u32 = 4;
/// Bytes per pixel (`B, G, R, X`).
pub const BPP: usize = 4;

/// A rectangle in pixels.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rect {
    /// Column.
    pub x: u32,
    /// Row.
    pub y: u32,
    /// Width.
    pub w: u32,
    /// Height.
    pub h: u32,
}

/// What the viewer has of the screen.
pub struct Differ {
    width: u32,
    height: u32,
    cols: u32,
    reference: Vec<u8>,
    known: Vec<bool>,
}

impl Differ {
    /// Nothing known yet for a screen of `width` x `height` pixels.
    pub fn new(width: u32, height: u32) -> Self {
        let cols = width.div_ceil(TILE);
        let rows = height.div_ceil(TILE);
        Self {
            width,
            height,
            cols,
            reference: vec![0; width as usize * height as usize * BPP],
            known: vec![false; (cols * rows) as usize],
        }
    }

    /// Size of the screen.
    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    fn fits(&self, frame: &[u8]) -> bool {
        frame.len() == self.reference.len()
    }

    fn tile(&self, tx: u32, ty: u32) -> Rect {
        let (x, y) = (tx * TILE, ty * TILE);
        Rect {
            x,
            y,
            w: TILE.min(self.width - x),
            h: TILE.min(self.height - y),
        }
    }

    fn row_bytes(&self, y: u32, x: u32, w: u32) -> std::ops::Range<usize> {
        let start = (y as usize * self.width as usize + x as usize) * BPP;
        start..start + w as usize * BPP
    }

    fn changed_tile(&self, frame: &[u8], tx: u32, ty: u32) -> bool {
        if !self.known[(ty * self.cols + tx) as usize] {
            return true;
        }
        let t = self.tile(tx, ty);
        (t.y..t.y + t.h).any(|y| {
            let r = self.row_bytes(y, t.x, t.w);
            self.reference[r.clone()] != frame[r]
        })
    }

    /// The changed rectangles of `frame` (at most `limit`), row by row. Empty when `frame`
    /// is not a picture of this screen's size.
    pub fn changed(&self, frame: &[u8], limit: usize) -> Vec<Rect> {
        let mut out = Vec::new();
        if !self.fits(frame) {
            return out;
        }
        let rows = self.height.div_ceil(TILE);
        for ty in 0..rows {
            let mut tx = 0;
            while tx < self.cols {
                if !self.changed_tile(frame, tx, ty) {
                    tx += 1;
                    continue;
                }
                let first = tx;
                while tx < self.cols && tx - first < MAX_RUN && self.changed_tile(frame, tx, ty) {
                    tx += 1;
                }
                let (a, b) = (self.tile(first, ty), self.tile(tx - 1, ty));
                out.push(Rect {
                    x: a.x,
                    y: a.y,
                    w: b.x + b.w - a.x,
                    h: a.h,
                });
                if out.len() >= limit {
                    return out;
                }
            }
        }
        out
    }

    /// The pixels of `r` out of `frame`, row after row.
    pub fn extract(&self, frame: &[u8], r: Rect) -> Vec<u8> {
        let mut out = Vec::with_capacity(r.w as usize * r.h as usize * BPP);
        if !self.fits(frame) || r.x + r.w > self.width || r.y + r.h > self.height {
            return out;
        }
        for y in r.y..r.y + r.h {
            out.extend_from_slice(&frame[self.row_bytes(y, r.x, r.w)]);
        }
        out
    }

    /// The viewer now has `r` as it is in `frame`.
    pub fn commit(&mut self, frame: &[u8], r: Rect) {
        if !self.fits(frame) || r.x + r.w > self.width || r.y + r.h > self.height {
            return;
        }
        for y in r.y..r.y + r.h {
            let range = self.row_bytes(y, r.x, r.w);
            self.reference[range.clone()].copy_from_slice(&frame[range]);
        }
        // Rectangles are made of whole tiles, so every tile inside is known now.
        for ty in r.y / TILE..(r.y + r.h).div_ceil(TILE) {
            for tx in r.x / TILE..(r.x + r.w).div_ceil(TILE) {
                self.known[(ty * self.cols + tx) as usize] = true;
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, v: u8) -> Vec<u8> {
        vec![v; w as usize * h as usize * BPP]
    }

    fn poke(f: &mut [u8], w: u32, x: u32, y: u32, v: u8) {
        f[(y as usize * w as usize + x as usize) * BPP] = v;
    }

    #[test]
    fn everything_is_changed_until_it_was_taken() {
        let d = Differ::new(200, 70);
        let f = frame(200, 70, 0);
        let rects = d.changed(&f, 100);
        // Four tile columns (64, 64, 64, 8) in rows of 64 and 6 pixels.
        assert_eq!(
            rects,
            vec![
                Rect {
                    x: 0,
                    y: 0,
                    w: 200,
                    h: 64
                },
                Rect {
                    x: 0,
                    y: 64,
                    w: 200,
                    h: 6
                }
            ]
        );
        let covered: u32 = rects.iter().map(|r| r.w * r.h).sum();
        assert_eq!(covered, 200 * 70);
    }

    #[test]
    fn a_taken_rectangle_is_quiet_until_the_screen_changes_there() {
        let (w, h) = (300, 130);
        let mut d = Differ::new(w, h);
        let mut f = frame(w, h, 7);
        for r in d.changed(&f, usize::MAX) {
            d.commit(&f, r);
        }
        assert!(d.changed(&f, 10).is_empty());
        poke(&mut f, w, 130, 70, 9); // tile column 2, row 1
        let rects = d.changed(&f, 10);
        assert_eq!(
            rects,
            vec![Rect {
                x: 128,
                y: 64,
                w: 64,
                h: 64
            }]
        );
        // Not taken (slow link): still changed next time.
        assert_eq!(d.changed(&f, 10), rects);
        d.commit(&f, rects[0]);
        assert!(d.changed(&f, 10).is_empty());
    }

    #[test]
    fn rectangles_never_exceed_one_mesh_frame_and_extract_matches() {
        let (w, h) = (1920, 1080);
        let d = Differ::new(w, h);
        let mut f = frame(w, h, 0);
        poke(&mut f, w, 1919, 1079, 5);
        let all = d.changed(&f, usize::MAX);
        assert!(all.iter().all(|r| (r.w * r.h) as usize <= 128 * 128));
        assert_eq!(all.iter().map(|r| r.w * r.h).sum::<u32>(), w * h);
        let last = all[all.len() - 1];
        let bytes = d.extract(&f, last);
        assert_eq!(bytes.len(), (last.w * last.h) as usize * BPP);
        assert_eq!(
            bytes[bytes.len() - BPP],
            5,
            "the last pixel is the poked one"
        );
    }

    #[test]
    fn the_limit_and_foreign_sizes_are_respected() {
        let d = Differ::new(640, 480);
        assert_eq!(d.changed(&frame(640, 480, 0), 3).len(), 3);
        assert!(d.changed(&frame(10, 10, 0), 3).is_empty());
        assert!(d
            .extract(
                &frame(640, 480, 0),
                Rect {
                    x: 600,
                    y: 0,
                    w: 100,
                    h: 1
                }
            )
            .is_empty());
        let mut d = Differ::new(64, 64);
        d.commit(
            &frame(1, 1, 0),
            Rect {
                x: 0,
                y: 0,
                w: 1,
                h: 1,
            },
        );
        assert_eq!(
            d.changed(&frame(64, 64, 0), 5).len(),
            1,
            "a foreign frame commits nothing"
        );
    }
}
