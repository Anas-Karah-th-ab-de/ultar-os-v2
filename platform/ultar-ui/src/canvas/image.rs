//! Pictures: a small RGBA image (a file thumbnail) scaled into a rectangle.

use super::{blend, Canvas, BPP};
use crate::geom::PixelRect;

impl Canvas {
    /// Draws the straight-alpha RGBA picture `rgba` of `width` × `height` pixels into
    /// `target`, scaled by nearest neighbour and blended over what is there. A picture whose
    /// length does not match its size, or an empty one, draws nothing.
    pub fn draw_rgba(&mut self, rgba: &[u8], width: u32, height: u32, target: PixelRect) {
        let (w, h) = (width as usize, height as usize);
        if w == 0 || h == 0 || rgba.len() != w.saturating_mul(h).saturating_mul(BPP) {
            return;
        }
        let area = target.intersect(&self.clip);
        if area.is_empty() || target.width() <= 0 || target.height() <= 0 {
            return;
        }
        let rgba_order = self.format == super::PixelFormat::Rgba8;
        let (tw, th) = (i64::from(target.width()), i64::from(target.height()));
        for y in area.y0..area.y1 {
            let sy = ((i64::from(y - target.y0) * h as i64) / th).clamp(0, h as i64 - 1) as usize;
            let row = self.row_mut(y, area.x0, area.x1);
            for (k, dst) in row.chunks_exact_mut(BPP).enumerate() {
                let x = i64::from(area.x0) + k as i64 - i64::from(target.x0);
                let sx = ((x * w as i64) / tw).clamp(0, w as i64 - 1) as usize;
                let at = (sy * w + sx) * BPP;
                let [r, g, b, a] = [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]];
                let px = if rgba_order {
                    [r, g, b, 255]
                } else {
                    [b, g, r, 255]
                };
                blend::blend_px(dst, px, u32::from(a));
            }
        }
        self.damage.add(area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::PixelFormat;
    use crate::Color;

    #[test]
    fn a_picture_is_scaled_and_clipped() {
        let mut c = Canvas::new(8, 8, PixelFormat::Rgba8).unwrap();
        c.clear(Color::BLACK);
        // 2 × 1: red | blue, drawn 4 × 2 at (2, 2)
        let rgba = [255, 0, 0, 255, 0, 0, 255, 255];
        c.draw_rgba(&rgba, 2, 1, PixelRect::new(2, 2, 6, 4));
        assert_eq!(c.pixel(2, 2).unwrap().r, 255);
        assert_eq!(c.pixel(5, 3).unwrap().b, 255);
        assert_eq!(c.pixel(6, 3).unwrap().r, 0);
        // wrong length: nothing
        c.draw_rgba(&rgba[..4], 2, 1, PixelRect::new(0, 0, 8, 8));
        assert_eq!(c.pixel(0, 0).unwrap().r, 0);
    }
}
