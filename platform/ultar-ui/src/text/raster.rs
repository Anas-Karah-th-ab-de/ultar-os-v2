//! Glyph rasterisation: ttf-parser outlines filled by `ab_glyph_rasterizer`.

use super::fonts::{FontId, Fonts};
use ab_glyph_rasterizer::{point, Point as RPoint, Rasterizer};
use rustybuzz::ttf_parser::{GlyphId, OutlineBuilder};

/// Horizontal sub-pixel positions a glyph is rasterised at.
pub(crate) const SUBPIXEL_STEPS: u8 = 4;

/// An 8-bit coverage image of one glyph and where to put it relative to the pen position.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct GlyphMask {
    pub(crate) width: usize,
    pub(crate) height: usize,
    /// Offset of the mask's left column from the pen position (whole pixels).
    pub(crate) left: i32,
    /// Offset of the mask's top row from the baseline (negative = above).
    pub(crate) top: i32,
    pub(crate) data: Vec<u8>,
}

/// Cache key of a rasterised glyph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub(crate) font: FontId,
    pub(crate) id: u16,
    pub(crate) size_bits: u32,
    pub(crate) subpixel: u8,
}

struct Outline {
    raster: Rasterizer,
    scale: f32,
    shift_x: f32,
    shift_y: f32,
    start: RPoint,
    last: RPoint,
}

impl Outline {
    fn map(&self, x: f32, y: f32) -> RPoint {
        point(
            x * self.scale + self.shift_x,
            -y * self.scale + self.shift_y,
        )
    }
}

impl OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = self.map(x, y);
        self.last = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.raster.draw_line(self.last, p);
        self.last = p;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (c, p) = (self.map(x1, y1), self.map(x, y));
        self.raster.draw_quad(self.last, c, p);
        self.last = p;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (c1, c2, p) = (self.map(x1, y1), self.map(x2, y2), self.map(x, y));
        self.raster.draw_cubic(self.last, c1, c2, p);
        self.last = p;
    }

    fn close(&mut self) {
        if self.last != self.start {
            self.raster.draw_line(self.last, self.start);
        }
        self.last = self.start;
    }
}

/// Largest glyph bitmap edge; bigger glyphs (absurd sizes) are not drawn.
const MAX_GLYPH_EDGE: f32 = 2048.0;

/// Rasterises one glyph. Glyphs without outline (spaces) give an empty mask.
pub(crate) fn rasterise(fonts: &Fonts, key: GlyphKey) -> GlyphMask {
    let face = fonts.face(key.font);
    let size = f32::from_bits(key.size_bits);
    let scale = fonts.scale(key.font, size);
    let Some(bbox) = face.glyph_bounding_box(GlyphId(key.id)) else {
        return GlyphMask::default();
    };
    let sub = f32::from(key.subpixel) / f32::from(SUBPIXEL_STEPS);
    let left = (f32::from(bbox.x_min) * scale + sub).floor() - 1.0;
    let top = (-f32::from(bbox.y_max) * scale).floor() - 1.0;
    let right = (f32::from(bbox.x_max) * scale + sub).ceil() + 1.0;
    let bottom = (-f32::from(bbox.y_min) * scale).ceil() + 1.0;
    let (w, h) = (right - left, bottom - top);
    if !(w >= 1.0 && h >= 1.0 && w <= MAX_GLYPH_EDGE && h <= MAX_GLYPH_EDGE) {
        return GlyphMask::default();
    }
    let (width, height) = (w as usize, h as usize);
    let mut outline = Outline {
        raster: Rasterizer::new(width, height),
        scale,
        shift_x: sub - left,
        shift_y: -top,
        start: point(0.0, 0.0),
        last: point(0.0, 0.0),
    };
    if face.outline_glyph(GlyphId(key.id), &mut outline).is_none() {
        return GlyphMask::default();
    }
    let mut data = vec![0u8; width * height];
    outline.raster.for_each_pixel(|index, alpha| {
        if let Some(cell) = data.get_mut(index) {
            *cell = (alpha.clamp(0.0, 1.0) * f32::from(u8::MAX) + 0.5) as u8;
        }
    });
    GlyphMask {
        width,
        height,
        left: left as i32,
        top: top as i32,
        data,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(fonts: &Fonts, font: FontId, ch: char, size: f32) -> GlyphKey {
        GlyphKey {
            font,
            id: fonts.face(font).glyph_index(ch).unwrap().0,
            size_bits: size.to_bits(),
            subpixel: 0,
        }
    }

    #[test]
    fn letters_have_ink_and_spaces_do_not() {
        let fonts = Fonts::new().unwrap();
        let mask = rasterise(&fonts, key(&fonts, FontId::Latin, 'H', 20.0));
        assert!(mask.width > 5 && mask.height > 10);
        assert!(mask.top < 0, "an H sits above the baseline");
        let ink: u32 = mask.data.iter().map(|v| u32::from(*v)).sum();
        assert!(ink > 255 * 20);
        assert!(mask.data.contains(&255));
        let space = rasterise(&fonts, key(&fonts, FontId::Latin, ' ', 20.0));
        assert_eq!(space, GlyphMask::default());
        let arabic = rasterise(&fonts, key(&fonts, FontId::Arabic, 'س', 20.0));
        assert!(arabic.data.iter().any(|v| *v > 0));
    }

    #[test]
    fn absurd_sizes_and_unknown_glyphs_are_empty() {
        let fonts = Fonts::new().unwrap();
        let huge = rasterise(&fonts, key(&fonts, FontId::Latin, 'H', 1.0e6));
        assert_eq!(huge, GlyphMask::default());
        let missing = GlyphKey {
            font: FontId::Latin,
            id: u16::MAX,
            size_bits: 16f32.to_bits(),
            subpixel: 0,
        };
        assert_eq!(rasterise(&fonts, missing), GlyphMask::default());
    }

    #[test]
    fn subpixel_shift_changes_the_mask() {
        let fonts = Fonts::new().unwrap();
        let mut k = key(&fonts, FontId::Latin, 'l', 14.0);
        let a = rasterise(&fonts, k);
        k.subpixel = 2;
        let b = rasterise(&fonts, k);
        assert_ne!(a.data, b.data);
    }
}
