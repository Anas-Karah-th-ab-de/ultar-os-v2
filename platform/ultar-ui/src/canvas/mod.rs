//! Software renderer into an RGBA or BGRX pixel buffer.
//!
//! Everything is clipped to the current clip rectangle and recorded in the damage list, so a
//! compositor can present only what changed. Coordinates are device pixels (`f32`, pixel
//! centres at `.5`). No allocation happens per pixel or per shape; the only growing buffer is
//! one scratch coverage mask that is reused for strokes.

mod blend;
mod damage;
mod shapes;
pub(crate) mod stroke;

pub use damage::{Damage, MAX_DAMAGE_RECTS};

use crate::color::Color;
use crate::geom::{PixelRect, Point};
use blend::BPP;
use std::fmt;

/// Largest supported width or height in pixels.
pub const MAX_DIMENSION: u32 = 16_384;

/// Byte order of a pixel in the buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PixelFormat {
    /// Red, green, blue, alpha (PNG order; alpha is maintained).
    Rgba8,
    /// Blue, green, red, unused (the usual scan-out order; the fourth byte stays 255).
    Bgrx8,
}

/// Why a canvas operation was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CanvasError {
    /// Width or height is zero or above [`MAX_DIMENSION`].
    InvalidSize {
        /// Requested width.
        width: u32,
        /// Requested height.
        height: u32,
    },
    /// A mask or buffer does not have `width * height` entries.
    LengthMismatch {
        /// Entries required.
        expected: usize,
        /// Entries supplied.
        actual: usize,
    },
}

impl fmt::Display for CanvasError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CanvasError::InvalidSize { width, height } => {
                write!(f, "invalid canvas size {width}x{height}")
            }
            CanvasError::LengthMismatch { expected, actual } => {
                write!(f, "buffer length {actual}, expected {expected}")
            }
        }
    }
}

impl std::error::Error for CanvasError {}

/// A pixel buffer with clip rectangle and damage list.
#[derive(Debug, Clone)]
pub struct Canvas {
    width: u32,
    height: u32,
    format: PixelFormat,
    data: Vec<u8>,
    clip: PixelRect,
    damage: Damage,
    scratch: Vec<u8>,
}

impl Canvas {
    /// A canvas of `width` × `height` pixels, transparent (RGBA) or black (BGRX).
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Result<Canvas, CanvasError> {
        let valid = |v: u32| v > 0 && v <= MAX_DIMENSION;
        if !valid(width) || !valid(height) {
            return Err(CanvasError::InvalidSize { width, height });
        }
        let mut data = vec![0u8; width as usize * height as usize * BPP];
        if format == PixelFormat::Bgrx8 {
            for px in data.chunks_exact_mut(BPP) {
                px[3] = u8::MAX;
            }
        }
        Ok(Canvas {
            width,
            height,
            format,
            data,
            clip: PixelRect::new(0, 0, width as i32, height as i32),
            damage: Damage::new(),
            scratch: Vec::new(),
        })
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Byte order of the buffer.
    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// The raw buffer: `height` rows of `width` 4-byte pixels, no padding.
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The whole surface as a pixel rectangle.
    pub fn bounds(&self) -> PixelRect {
        PixelRect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// The current clip rectangle.
    pub fn clip(&self) -> PixelRect {
        self.clip
    }

    /// Restricts drawing to `clip` (intersected with the surface) and returns the previous clip.
    pub fn set_clip(&mut self, clip: PixelRect) -> PixelRect {
        let previous = self.clip;
        self.clip = clip.intersect(&self.bounds());
        previous
    }

    /// Removes any clip restriction.
    pub fn reset_clip(&mut self) {
        self.clip = self.bounds();
    }

    /// Regions drawn to since the damage was last taken.
    pub fn damage(&self) -> &Damage {
        &self.damage
    }

    /// Returns and clears the damage list (call once per presented frame).
    pub fn take_damage(&mut self) -> Damage {
        self.damage.take()
    }

    /// The colour of one pixel, `None` outside the surface.
    pub fn pixel(&self, x: i32, y: i32) -> Option<Color> {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * BPP;
        let p = self.data.get(i..i + BPP)?;
        Some(match self.format {
            PixelFormat::Rgba8 => Color::rgba(p[0], p[1], p[2], p[3]),
            PixelFormat::Bgrx8 => Color::rgba(p[2], p[1], p[0], u8::MAX),
        })
    }

    /// A copy of the surface in RGBA byte order (for PNG encoding).
    pub fn to_rgba(&self) -> Vec<u8> {
        let mut out = self.data.clone();
        if self.format == PixelFormat::Bgrx8 {
            for px in out.chunks_exact_mut(BPP) {
                px.swap(0, 2);
                px[3] = u8::MAX;
            }
        }
        out
    }

    /// Colour bytes in buffer order.
    fn pack(&self, color: Color) -> [u8; BPP] {
        match self.format {
            PixelFormat::Rgba8 => [color.r, color.g, color.b, color.a],
            PixelFormat::Bgrx8 => [color.b, color.g, color.r, color.a],
        }
    }

    /// The bytes of row `y` from column `x0` to `x1` (caller guarantees they are inside).
    fn row_mut(&mut self, y: i32, x0: i32, x1: i32) -> &mut [u8] {
        let start = (y.max(0) as usize * self.width as usize + x0.max(0) as usize) * BPP;
        let end = start + (x1 - x0).max(0) as usize * BPP;
        // Callers clip first; an area outside the buffer draws nothing instead of panicking.
        self.data.get_mut(start..end).unwrap_or_default()
    }

    /// Fills the clip area with `color`, replacing what is there (no blending).
    pub fn clear(&mut self, color: Color) {
        let clip = self.clip;
        if clip.is_empty() {
            return;
        }
        let px = self.pack(color);
        let alpha = if self.format == PixelFormat::Bgrx8 {
            u8::MAX
        } else {
            color.a
        };
        let value = [px[0], px[1], px[2], alpha];
        for y in clip.y0..clip.y1 {
            for dst in self.row_mut(y, clip.x0, clip.x1).chunks_exact_mut(BPP) {
                dst.copy_from_slice(&value);
            }
        }
        self.damage.add(clip);
    }

    /// Blends `color` through an 8-bit coverage mask of `width` × `height` placed at `(x, y)`.
    pub fn fill_mask(
        &mut self,
        x: i32,
        y: i32,
        width: usize,
        height: usize,
        mask: &[u8],
        color: Color,
    ) -> Result<(), CanvasError> {
        let expected = width.saturating_mul(height);
        if mask.len() != expected {
            return Err(CanvasError::LengthMismatch {
                expected,
                actual: mask.len(),
            });
        }
        let (x64, y64) = (i64::from(x), i64::from(y));
        let target = PixelRect::new(
            x,
            y,
            saturate(x64 + width.min(SPAN_MAX) as i64),
            saturate(y64 + height.min(SPAN_MAX) as i64),
        )
        .intersect(&self.clip);
        if target.is_empty() || color.a == 0 {
            return Ok(());
        }
        let px = self.pack(color);
        let alpha = u32::from(color.a);
        let columns = target.width() as usize;
        for row in target.y0..target.y1 {
            // The target lies inside the mask, so both differences are small and positive.
            let m0 =
                (i64::from(row) - y64) as usize * width + (i64::from(target.x0) - x64) as usize;
            let Some(line) = mask.get(m0..m0 + columns) else {
                continue;
            };
            blend::blend_mask_row(self.row_mut(row, target.x0, target.x1), line, px, alpha);
        }
        self.damage.add(target);
        Ok(())
    }

    /// Copies `source_area` of another canvas to `(x, y)`, converting the byte order if needed.
    /// Pixels are replaced, not blended; the copy is clipped on both sides.
    pub fn blit(&mut self, source: &Canvas, source_area: PixelRect, x: i32, y: i32) {
        let src = source_area.intersect(&source.bounds());
        if src.is_empty() {
            return;
        }
        // Offsets from source to target coordinates; 64 bits because both operands may be
        // anywhere in the 32-bit range.
        let dx = i64::from(x) - i64::from(source_area.x0);
        let dy = i64::from(y) - i64::from(source_area.y0);
        let moved = |v: i32, d: i64| saturate(i64::from(v) + d);
        let target = PixelRect::new(
            moved(src.x0, dx),
            moved(src.y0, dy),
            moved(src.x1, dx),
            moved(src.y1, dy),
        )
        .intersect(&self.clip);
        if target.is_empty() {
            return;
        }
        let swap = self.format != source.format;
        // A BGRX target needs opaque pixels; a BGRX source has them already, so a copy
        // between two BGRX canvases is a plain memory copy.
        let opaque = self.format == PixelFormat::Bgrx8 && source.format != PixelFormat::Bgrx8;
        if !(swap || opaque) && dx == 0 && dy == 0 && self.copy_large(source, target) {
            return;
        }
        let columns = target.width() as usize;
        for row in target.y0..target.y1 {
            let (sy, sx) = (i64::from(row) - dy, i64::from(target.x0) - dx);
            if sy < 0 || sx < 0 {
                continue;
            }
            let s0 = (sy as usize * source.width as usize + sx as usize) * BPP;
            let Some(from) = source.data.get(s0..s0 + columns * BPP) else {
                continue;
            };
            let to = self.row_mut(row, target.x0, target.x1);
            if to.len() != from.len() {
                continue;
            }
            to.copy_from_slice(from);
            if swap || opaque {
                for px in to.chunks_exact_mut(BPP) {
                    if swap {
                        px.swap(0, 2);
                    }
                    if opaque {
                        px[3] = u8::MAX;
                    }
                }
            }
        }
        self.damage.add(target);
    }

    /// A plain copy of a large area between two canvases of the same size and format,
    /// rows shared among the cores (the backdrop under a dialog at 4K). `false` when the
    /// area is small or the canvases differ (the caller copies row by row).
    fn copy_large(&mut self, source: &Canvas, area: PixelRect) -> bool {
        let cores = shapes::cores();
        let same = (self.width, self.height) == (source.width, source.height);
        if !same || cores < 2 || area.area() < shapes::PARALLEL_AREA {
            return false;
        }
        let stride = self.width as usize * BPP;
        let rows = area.y0 as usize * stride..area.y1 as usize * stride;
        let columns = area.x0 as usize * BPP..area.x1 as usize * BPP;
        let (Some(to), Some(from)) = (self.data.get_mut(rows.clone()), source.data.get(rows))
        else {
            return false;
        };
        let per = (area.height() as usize).div_ceil(cores).max(1) * stride;
        let work = |(to, from): (&mut [u8], &[u8])| {
            for (t, f) in to.chunks_exact_mut(stride).zip(from.chunks_exact(stride)) {
                if let (Some(t), Some(f)) = (t.get_mut(columns.clone()), f.get(columns.clone())) {
                    t.copy_from_slice(f);
                }
            }
        };
        shapes::shared(
            to.chunks_mut(per).zip(from.chunks(per)).collect(),
            cores,
            &work,
        );
        self.damage.add(area);
        true
    }

    /// Strokes a line with round caps.
    pub fn line(&mut self, from: Point, to: Point, width: f32, color: Color) {
        self.stroke_polyline(&[from, to], false, width, color);
    }

    /// Strokes connected segments with round caps and joins; `closed` joins the last point to
    /// the first. Overlaps within the path are not blended twice.
    pub fn stroke_polyline(&mut self, points: &[Point], closed: bool, width: f32, color: Color) {
        if points.is_empty() || width <= 0.0 || color.a == 0 {
            return;
        }
        let half = width / 2.0;
        let reach = half + 1.0;
        let fold = |acc: (f32, f32, f32, f32), p: &Point| {
            (
                acc.0.min(p.x),
                acc.1.min(p.y),
                acc.2.max(p.x),
                acc.3.max(p.y),
            )
        };
        let start = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        let (min_x, min_y, max_x, max_y) = points.iter().fold(start, fold);
        let bbox = PixelRect::new(
            (min_x - reach).floor() as i32,
            (min_y - reach).floor() as i32,
            (max_x + reach).ceil() as i32,
            (max_y + reach).ceil() as i32,
        )
        .intersect(&self.clip);
        if bbox.is_empty() {
            return;
        }
        let (w, h) = (bbox.width() as usize, bbox.height() as usize);
        let mut mask = std::mem::take(&mut self.scratch);
        mask.clear();
        mask.resize(w * h, 0);
        let local = |p: Point| Point::new(p.x - bbox.x0 as f32, p.y - bbox.y0 as f32);
        let count = if closed {
            points.len()
        } else {
            points.len() - 1
        };
        for i in 0..count.max(1) {
            let a = local(points[i]);
            let b = local(points[(i + 1) % points.len()]);
            stroke::stroke_segment(&mut mask, w, h, a, b, half);
        }
        // The mask has exactly w*h entries, so this cannot fail.
        let _ = self.fill_mask(bbox.x0, bbox.y0, w, h, &mask, color);
        self.scratch = mask;
    }
}

/// Largest mask edge that still fits the 32-bit pixel coordinates.
const SPAN_MAX: usize = i32::MAX as usize;

/// A 64-bit coordinate clamped into the 32-bit pixel range.
fn saturate(value: i64) -> i32 {
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}
