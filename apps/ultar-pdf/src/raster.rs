//! Drawing into the BGRX buffer of the page area: a rendered page stretched and turned into
//! a rectangle, translucent highlights, frames and small numbers. Everything clips to the
//! buffer, so a caller may pass rectangles that reach beyond it.

use crate::state::{source_uv, Rot};
use ultar_office::pdf::PageImage;

/// A BGRX buffer of `w`×`h` device pixels.
pub struct Buf {
    pub w: i32,
    pub h: i32,
    pub data: Vec<u8>,
}

/// A colour with its opacity (0–255).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba(pub u8, pub u8, pub u8, pub u8);

impl Buf {
    /// A buffer filled with `c`.
    pub fn new(w: u32, h: u32, c: Rgba) -> Buf {
        let px = [c.2, c.1, c.0, 255];
        Buf {
            w: w as i32,
            h: h as i32,
            data: px.repeat(w as usize * h as usize),
        }
    }

    fn at(&self, x: i32, y: i32) -> usize {
        (y as usize * self.w as usize + x as usize) * 4
    }

    /// The colour at a pixel (for tests).
    pub fn pixel(&self, x: i32, y: i32) -> Option<(u8, u8, u8)> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        let i = self.at(x, y);
        Some((self.data[i + 2], self.data[i + 1], self.data[i]))
    }

    /// Blends `c` over the rectangle `x0..x1`, `y0..y1` (device pixels, end exclusive).
    pub fn blend(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, c: Rgba) {
        let a = c.3 as u32;
        for y in y0.max(0)..y1.min(self.h) {
            for x in x0.max(0)..x1.min(self.w) {
                let i = self.at(x, y);
                for (k, v) in [c.2, c.1, c.0].into_iter().enumerate() {
                    let old = self.data[i + k] as u32;
                    self.data[i + k] = ((old * (255 - a) + v as u32 * a) / 255) as u8;
                }
            }
        }
    }

    /// A one-pixel (or `t`-pixel) frame inside the rectangle.
    pub fn frame(&mut self, x0: i32, y0: i32, x1: i32, y1: i32, t: i32, c: Rgba) {
        self.blend(x0, y0, x1, y0 + t, c);
        self.blend(x0, y1 - t, x1, y1, c);
        self.blend(x0, y0 + t, x0 + t, y1 - t, c);
        self.blend(x1 - t, y0 + t, x1, y1 - t, c);
    }

    /// Draws `img` stretched into the rectangle, turned by `rot` (nearest pixel).
    pub fn picture(&mut self, x0: i32, y0: i32, w: i32, h: i32, img: &PageImage, rot: Rot) {
        if w <= 0
            || h <= 0
            || img.w == 0
            || img.h == 0
            || img.rgba.len() < img.w as usize * img.h as usize * 4
        {
            return;
        }
        let (sw, sh) = (img.w as usize, img.h as usize);
        let straight = rot == Rot::None && img.w as i32 == w && img.h as i32 == h;
        for y in y0.max(0)..(y0 + h).min(self.h) {
            let v = ((y - y0) as f32 + 0.5) / h as f32;
            for x in x0.max(0)..(x0 + w).min(self.w) {
                let (sx, sy) = if straight {
                    ((x - x0) as usize, (y - y0) as usize)
                } else {
                    let u = ((x - x0) as f32 + 0.5) / w as f32;
                    let (su, sv) = source_uv(u, v, rot);
                    (
                        ((su * sw as f32) as usize).min(sw - 1),
                        ((sv * sh as f32) as usize).min(sh - 1),
                    )
                };
                let s = (sy * sw + sx) * 4;
                let d = self.at(x, y);
                self.data[d] = img.rgba[s + 2];
                self.data[d + 1] = img.rgba[s + 1];
                self.data[d + 2] = img.rgba[s];
            }
        }
    }

    /// Two crossing lines in the rectangle: the page could not be drawn.
    pub fn cross(&mut self, x0: i32, y0: i32, w: i32, h: i32, c: Rgba) {
        let steps = w.max(h).max(1);
        for i in 0..=steps {
            let x = x0 + (w as i64 * i as i64 / steps as i64) as i32;
            let y = y0 + (h as i64 * i as i64 / steps as i64) as i32;
            let y2 = y0 + h - (y - y0);
            self.blend(x, y, x + 2, y + 2, c);
            self.blend(x, y2, x + 2, y2 + 2, c);
        }
    }

    /// The digits of `n` (0–9 only, as APP-RULES §10 wants), each `3*k` by `5*k` pixels,
    /// centred on `cx` with its top at `y`.
    pub fn number(&mut self, n: usize, cx: i32, y: i32, k: i32, c: Rgba) {
        let digits: Vec<usize> = n.to_string().bytes().map(|b| (b - b'0') as usize).collect();
        let width = digits.len() as i32 * 4 * k - k;
        let mut x = cx - width / 2;
        for d in digits {
            for (row, bits) in GLYPHS[d].iter().enumerate() {
                for col in 0..3 {
                    if bits >> (2 - col) & 1 == 1 {
                        let (px, py) = (x + col * k, y + row as i32 * k);
                        self.blend(px, py, px + k, py + k, c);
                    }
                }
            }
            x += 4 * k;
        }
    }
}

/// A 3×5 pixel font for the digits.
const GLYPHS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgba = Rgba(255, 255, 255, 255);

    /// A 2×1 picture: a red pixel then a blue one.
    fn two() -> PageImage {
        PageImage {
            w: 2,
            h: 1,
            rgba: vec![255, 0, 0, 255, 0, 0, 255, 255],
        }
    }

    #[test]
    fn a_picture_is_stretched_and_clipped() {
        let mut b = Buf::new(8, 4, Rgba(0, 0, 0, 255));
        b.picture(0, 0, 4, 2, &two(), Rot::None);
        assert_eq!(b.pixel(0, 0), Some((255, 0, 0)));
        assert_eq!(b.pixel(1, 1), Some((255, 0, 0)));
        assert_eq!(b.pixel(2, 0), Some((0, 0, 255)));
        assert_eq!(b.pixel(4, 0), Some((0, 0, 0)));
        // partly outside: no panic, the visible part is right
        b.picture(6, 2, 4, 4, &two(), Rot::None);
        assert_eq!(b.pixel(6, 2), Some((255, 0, 0)));
        b.picture(-2, -1, 4, 2, &two(), Rot::None);
        assert_eq!(b.pixel(0, 0), Some((0, 0, 255)));
    }

    #[test]
    fn a_turned_picture_samples_the_rotated_spot() {
        // 2×1 picture turned clockwise shows 1 wide and 2 high: the red pixel (left) is on top.
        let mut b = Buf::new(2, 2, Rgba(0, 0, 0, 255));
        b.picture(0, 0, 1, 2, &two(), Rot::Cw);
        assert_eq!(b.pixel(0, 0), Some((255, 0, 0)));
        assert_eq!(b.pixel(0, 1), Some((0, 0, 255)));
        let mut c = Buf::new(2, 2, Rgba(0, 0, 0, 255));
        c.picture(0, 0, 1, 2, &two(), Rot::Ccw);
        assert_eq!(c.pixel(0, 0), Some((0, 0, 255)));
        let mut h = Buf::new(2, 1, Rgba(0, 0, 0, 255));
        h.picture(0, 0, 2, 1, &two(), Rot::Half);
        assert_eq!(h.pixel(0, 0), Some((0, 0, 255)));
    }

    #[test]
    fn a_short_buffer_in_a_picture_draws_nothing() {
        let bad = PageImage {
            w: 4,
            h: 4,
            rgba: vec![0; 8],
        };
        let mut b = Buf::new(4, 4, WHITE);
        b.picture(0, 0, 4, 4, &bad, Rot::None);
        assert_eq!(b.pixel(0, 0), Some((255, 255, 255)));
    }

    #[test]
    fn blending_mixes_by_opacity_and_clips() {
        let mut b = Buf::new(4, 4, WHITE);
        b.blend(-5, -5, 2, 2, Rgba(0, 0, 0, 128));
        assert_eq!(b.pixel(0, 0), Some((127, 127, 127)));
        assert_eq!(b.pixel(2, 2), Some((255, 255, 255)));
        b.blend(3, 3, 99, 99, Rgba(0, 0, 0, 255));
        assert_eq!(b.pixel(3, 3), Some((0, 0, 0)));
        assert_eq!(b.pixel(9, 9), None);
    }

    #[test]
    fn digits_are_drawn_as_their_shape() {
        let mut b = Buf::new(20, 8, WHITE);
        b.number(10, 10, 1, 1, Rgba(0, 0, 0, 255));
        // "1": the top bar of the 1 is the middle pixel of its first row
        let dark = |x, y| b.pixel(x, y) == Some((0, 0, 0));
        let left = 10 - (2 * 4 - 1) / 2;
        assert!(dark(left + 1, 1));
        assert!(!dark(left, 1));
        // "0": a ring, empty in the middle
        assert!(dark(left + 4, 1) && !dark(left + 5, 3));
    }
}
