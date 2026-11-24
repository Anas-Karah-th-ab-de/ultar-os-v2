//! Pixels that come from outside the toolkit: a canvas made from a buffer and rows written
//! into a canvas (the frames of a web page, a video, a camera). Every input is checked;
//! nothing is read or written outside the buffers.

use super::{Canvas, CanvasError, PixelFormat, BPP, MAX_DIMENSION};
use crate::geom::PixelRect;

impl Canvas {
    /// A BGRX canvas that takes over `data` (`height` rows of `width` 4-byte pixels, no
    /// padding). The fourth byte of every pixel is set to 255.
    pub fn from_bgrx(width: u32, height: u32, mut data: Vec<u8>) -> Result<Canvas, CanvasError> {
        let valid = |v: u32| v > 0 && v <= MAX_DIMENSION;
        if !valid(width) || !valid(height) {
            return Err(CanvasError::InvalidSize { width, height });
        }
        let expected = width as usize * height as usize * BPP;
        if data.len() != expected {
            return Err(CanvasError::LengthMismatch {
                expected,
                actual: data.len(),
            });
        }
        for pixel in data.chunks_exact_mut(BPP) {
            pixel[3] = u8::MAX;
        }
        let mut canvas = Canvas::new(1, 1, PixelFormat::Bgrx8)?;
        canvas.width = width;
        canvas.height = height;
        canvas.data = data;
        canvas.reset_clip();
        Ok(canvas)
    }

    /// Writes `area` from `rows` (BGRX or BGRA pixels, `stride` bytes per row, the first row
    /// of `rows` is the top row of `area`) into a BGRX canvas; alpha is ignored. The area is
    /// clipped to the surface (not to the clip rectangle) and recorded as damage.
    pub fn write_bgrx(
        &mut self,
        area: PixelRect,
        rows: &[u8],
        stride: usize,
    ) -> Result<(), CanvasError> {
        if self.format != PixelFormat::Bgrx8 {
            return Err(CanvasError::InvalidSize {
                width: self.width,
                height: self.height,
            });
        }
        let target = area.intersect(&self.bounds());
        if target.is_empty() {
            return Ok(());
        }
        let columns = area.width().max(0) as usize;
        let lines = area.height().max(0) as usize;
        let needed = match lines {
            0 => 0,
            n => stride.saturating_mul(n - 1).saturating_add(columns * BPP),
        };
        if stride < columns * BPP || rows.len() < needed {
            return Err(CanvasError::LengthMismatch {
                expected: needed.max(columns * BPP),
                actual: rows.len(),
            });
        }
        let (skip_x, skip_y) = (
            (target.x0 - area.x0) as usize,
            (target.y0 - area.y0) as usize,
        );
        let width = target.width() as usize * BPP;
        let row_bytes = self.width as usize * BPP;
        for line in 0..target.height() as usize {
            let src = (skip_y + line) * stride + skip_x * BPP;
            let dst = (target.y0 as usize + line) * row_bytes + target.x0 as usize * BPP;
            let (Some(from), Some(to)) = (
                rows.get(src..src + width),
                self.data.get_mut(dst..dst + width),
            ) else {
                continue;
            };
            to.copy_from_slice(from);
            for pixel in to.chunks_exact_mut(BPP) {
                pixel[3] = u8::MAX;
            }
        }
        self.damage.add(target);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;

    /// A colour from its bytes in BGR order (as in the buffer).
    fn bgr(b: u8, g: u8, r: u8) -> Color {
        Color::rgb(r, g, b)
    }

    #[test]
    fn a_canvas_from_a_buffer_checks_size_and_length() {
        assert!(Canvas::from_bgrx(0, 1, Vec::new()).is_err());
        assert!(matches!(
            Canvas::from_bgrx(2, 2, vec![0; 15]),
            Err(CanvasError::LengthMismatch { expected: 16, .. })
        ));
        let canvas = Canvas::from_bgrx(2, 1, vec![1, 2, 3, 0, 4, 5, 6, 7]).unwrap();
        assert_eq!(canvas.data(), &[1, 2, 3, 255, 4, 5, 6, 255]);
        assert_eq!(canvas.pixel(1, 0), Some(bgr(4, 5, 6)));
    }

    #[test]
    fn rows_are_written_clipped_and_damaged() {
        let mut canvas = Canvas::new(4, 4, PixelFormat::Bgrx8).unwrap();
        canvas.take_damage();
        // 3 × 2 pixels with a padded stride, placed so that one column falls outside.
        let stride = 16;
        let mut rows = vec![0u8; stride * 2];
        for (i, b) in rows.iter_mut().enumerate() {
            *b = i as u8;
        }
        canvas
            .write_bgrx(PixelRect::new(2, 1, 5, 3), &rows, stride)
            .unwrap();
        assert_eq!(canvas.pixel(2, 1), Some(bgr(0, 1, 2)));
        assert_eq!(canvas.pixel(3, 2), Some(bgr(20, 21, 22)));
        assert_eq!(canvas.pixel(1, 1), Some(bgr(0, 0, 0)));
        assert_eq!(canvas.damage().bounds(), PixelRect::new(2, 1, 4, 3));
        // Too short a buffer and too small a stride are refused, nothing written.
        assert!(canvas
            .write_bgrx(PixelRect::new(0, 0, 2, 2), &rows[..10], 8)
            .is_err());
        assert!(canvas
            .write_bgrx(PixelRect::new(0, 0, 2, 2), &rows, 4)
            .is_err());
        // An area outside the surface writes nothing and is fine.
        assert!(canvas
            .write_bgrx(PixelRect::new(9, 9, 12, 12), &rows, stride)
            .is_ok());
        let mut rgba = Canvas::new(1, 1, PixelFormat::Rgba8).unwrap();
        assert!(rgba
            .write_bgrx(PixelRect::new(0, 0, 1, 1), &rows, 4)
            .is_err());
    }
}
