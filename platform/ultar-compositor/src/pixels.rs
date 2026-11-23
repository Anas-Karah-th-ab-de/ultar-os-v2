//! Pixel images and the software composition primitives.
//!
//! Pixels are `u32` words `0xAARRGGBB` with premultiplied alpha (the
//! little-endian byte order is B, G, R, A – the scan-out order of DRM and
//! the Ultar framebuffer). The inner loops are branch-free over whole rows
//! so the compiler vectorises them (SSE2/AVX2/NEON); no `unsafe`.

use crate::geom::Rect;

/// An image in compositor memory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    width: u32,
    height: u32,
    data: Vec<u32>,
}

/// Largest side of an image.
pub const MAX_IMAGE_SIDE: u32 = 16_384;

impl Image {
    /// An image filled with `color`; `None` if a side is 0 or too large.
    pub fn new(width: u32, height: u32, color: u32) -> Option<Image> {
        if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
            return None;
        }
        Some(Image {
            width,
            height,
            data: vec![color; width as usize * height as usize],
        })
    }

    /// Wraps BGRX/BGRA bytes (4 per pixel, rows without padding).
    pub fn from_bytes(width: u32, height: u32, bytes: &[u8], opaque: bool) -> Option<Image> {
        let mut image = Image::new(width, height, 0)?;
        if bytes.len() != image.data.len() * 4 {
            return None;
        }
        let force = if opaque { 0xff00_0000 } else { 0 };
        for (px, b) in image.data.iter_mut().zip(bytes.chunks_exact(4)) {
            *px = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) | force;
        }
        Some(image)
    }

    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// The whole image as a rectangle at the origin.
    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.width as i32, self.height as i32)
    }

    /// All pixels, row by row.
    pub fn data(&self) -> &[u32] {
        &self.data
    }

    /// All pixels for writing.
    pub fn data_mut(&mut self) -> &mut [u32] {
        &mut self.data
    }

    /// One pixel (None outside).
    pub fn pixel(&self, x: i32, y: i32) -> Option<u32> {
        if !self.bounds().contains(x, y) {
            return None;
        }
        self.data
            .get(y as usize * self.width as usize + x as usize)
            .copied()
    }

    /// Pixels of row `y` from `x` for `len` pixels (caller clips).
    fn span(&self, x: i32, y: i32, len: usize) -> &[u32] {
        let start = y as usize * self.width as usize + x as usize;
        &self.data[start..start + len]
    }

    fn span_mut(&mut self, x: i32, y: i32, len: usize) -> &mut [u32] {
        let start = y as usize * self.width as usize + x as usize;
        &mut self.data[start..start + len]
    }

    /// The pixels as little-endian bytes (B, G, R, A), for snapshots and
    /// capture.
    pub fn to_bytes(&self, rect: Rect) -> Vec<u8> {
        let Some(r) = rect.intersect(&self.bounds()) else {
            return Vec::new();
        };
        let mut out = Vec::with_capacity(r.area() as usize * 4);
        for y in r.y..r.max_y() {
            for px in self.span(r.x, y, r.w as usize) {
                out.extend_from_slice(&px.to_le_bytes());
            }
        }
        out
    }
}

/// Source-over for one premultiplied pixel.
#[inline(always)]
pub fn over(dst: u32, src: u32) -> u32 {
    let inv = 255 - (src >> 24);
    let rb = (dst & 0x00ff_00ff) * inv;
    let ag = ((dst >> 8) & 0x00ff_00ff) * inv;
    let rb = ((rb + 0x0080_0080 + ((rb >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff;
    let ag = (ag + 0x0080_0080 + ((ag >> 8) & 0x00ff_00ff)) & 0xff00_ff00;
    src.wrapping_add(rb | ag)
}

/// `dst` scaled by `keep / 255` (darkening by a shadow of opacity
/// `255 - keep`).
#[inline(always)]
fn attenuate(dst: u32, keep: u32) -> u32 {
    let rb = (dst & 0x00ff_00ff) * keep;
    let ag = ((dst >> 8) & 0x00ff_00ff) * keep;
    let rb = ((rb + 0x0080_0080 + ((rb >> 8) & 0x00ff_00ff)) >> 8) & 0x00ff_00ff;
    let ag = (ag + 0x0080_0080 + ((ag >> 8) & 0x00ff_00ff)) & 0xff00_ff00;
    rb | ag
}

/// Linear mix: `coverage` 255 = `src`, 0 = `dst`.
#[inline(always)]
fn mix(dst: u32, src: u32, coverage: u32) -> u32 {
    over(dst, attenuate(src, coverage))
}

/// Clips a copy of `src_rect` of `src` to position (`dx`,`dy`) in `dst`
/// and to `clip`; returns (destination rect, source x, source y).
fn clip_copy(
    dst: &Image,
    dx: i32,
    dy: i32,
    src: &Image,
    src_rect: Rect,
    clip: &Rect,
) -> Option<(Rect, i32, i32)> {
    let s = src_rect.intersect(&src.bounds())?;
    let d = Rect::new(dx + (s.x - src_rect.x), dy + (s.y - src_rect.y), s.w, s.h);
    let d = d.intersect(&dst.bounds())?.intersect(clip)?;
    Some((d, src_rect.x + (d.x - dx), src_rect.y + (d.y - dy)))
}

/// Fills `rect ∩ clip` with an opaque or premultiplied colour (replace).
pub fn fill(dst: &mut Image, rect: Rect, clip: &Rect, color: u32) {
    let Some(r) = rect
        .intersect(clip)
        .and_then(|r| r.intersect(&dst.bounds()))
    else {
        return;
    };
    for y in r.y..r.max_y() {
        dst.span_mut(r.x, y, r.w as usize).fill(color);
    }
}

/// Copies `src_rect` of `src` to (`dx`,`dy`) in `dst`, clipped (opaque).
pub fn copy(dst: &mut Image, dx: i32, dy: i32, src: &Image, src_rect: Rect, clip: &Rect) {
    let Some((d, sx, sy)) = clip_copy(dst, dx, dy, src, src_rect, clip) else {
        return;
    };
    for row in 0..d.h {
        let from = src.span(sx, sy + row, d.w as usize);
        dst.span_mut(d.x, d.y + row, d.w as usize)
            .copy_from_slice(from);
    }
}

/// Blends `src_rect` of `src` over `dst` at (`dx`,`dy`), clipped.
pub fn blend(dst: &mut Image, dx: i32, dy: i32, src: &Image, src_rect: Rect, clip: &Rect) {
    let Some((d, sx, sy)) = clip_copy(dst, dx, dy, src, src_rect, clip) else {
        return;
    };
    for row in 0..d.h {
        let from = src.span(sx, sy + row, d.w as usize);
        let to = dst.span_mut(d.x, d.y + row, d.w as usize);
        for (t, f) in to.iter_mut().zip(from) {
            *t = over(*t, *f);
        }
    }
}

/// Draws `src` stretched (nearest neighbour) onto `dest` in `dst`,
/// clipped; used when a window shows on an output whose scale differs from
/// the one it drew for.
pub fn scale(dst: &mut Image, dest: Rect, clip: &Rect, src: &Image, opaque: bool) {
    let Some(r) = dest
        .intersect(clip)
        .and_then(|r| r.intersect(&dst.bounds()))
    else {
        return;
    };
    let step_x = (u64::from(src.width) << 16) / dest.w.max(1) as u64;
    let step_y = (u64::from(src.height) << 16) / dest.h.max(1) as u64;
    let max_x = src.width as u64 - 1;
    for y in r.y..r.max_y() {
        let sy = (((y - dest.y) as u64 * step_y) >> 16).min(u64::from(src.height) - 1) as i32;
        let row = src.span(0, sy, src.width as usize);
        let base = (r.x - dest.x) as u64 * step_x;
        let to = dst.span_mut(r.x, y, r.w as usize);
        for (i, t) in to.iter_mut().enumerate() {
            let sx = ((base + i as u64 * step_x) >> 16).min(max_x) as usize;
            *t = if opaque { row[sx] } else { over(*t, row[sx]) };
        }
    }
}

/// Coverage of the quarter circle of a rounded corner with radius `r`
/// pixels (`r × r` entries, row 0 = outermost row of the top-left corner).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CornerMask {
    radius: u32,
    coverage: Vec<u8>,
}

impl CornerMask {
    /// The mask of a radius (at least 1 pixel).
    pub fn new(radius: u32) -> CornerMask {
        let r = radius.max(1);
        let n = r as usize;
        let mut coverage = vec![255u8; n * n];
        let rf = r as f32;
        for y in 0..n {
            for x in 0..n {
                // Centre of the circle at (r, r); 4×4 supersampling.
                let mut inside = 0u32;
                for sy in 0..4 {
                    for sx in 0..4 {
                        let px = x as f32 + (sx as f32 + 0.5) / 4.0 - rf;
                        let py = y as f32 + (sy as f32 + 0.5) / 4.0 - rf;
                        inside += u32::from(px * px + py * py <= rf * rf);
                    }
                }
                coverage[y * n + x] = (inside * 255 / 16) as u8;
            }
        }
        CornerMask {
            radius: r,
            coverage,
        }
    }

    /// Radius in pixels.
    pub fn radius(&self) -> u32 {
        self.radius
    }

    /// The coverage values.
    pub fn coverage(&self) -> &[u8] {
        &self.coverage
    }

    /// Re-draws the corner pixels of `frame` (destination of a rounded
    /// window part) with coverage: `src` is the part's image placed at the
    /// frame's origin; the top corners are round, or the bottom ones with
    /// `bottom`. The pixels must already hold what lies *below* the part.
    pub fn apply(&self, dst: &mut Image, frame: Rect, clip: &Rect, src: &Image, bottom: bool) {
        let n = self.radius as i32;
        let y0 = if bottom { frame.max_y() - n } else { frame.y };
        for (cx, flip_x) in [(frame.x, false), (frame.max_x() - n, true)] {
            let Some(area) = Rect::new(cx, y0, n, n)
                .intersect(clip)
                .and_then(|a| a.intersect(&dst.bounds()))
            else {
                continue;
            };
            for y in area.y..area.max_y() {
                let my = if bottom { n - 1 - (y - y0) } else { y - y0 } as usize;
                for x in area.x..area.max_x() {
                    let mx = if flip_x { n - 1 - (x - cx) } else { x - cx } as usize;
                    let coverage = u32::from(self.coverage[my * n as usize + mx]);
                    if let (Some(s), Some(d)) =
                        (src.pixel(x - frame.x, y - frame.y), dst.pixel(x, y))
                    {
                        let i = y as usize * dst.width as usize + x as usize;
                        dst.data[i] = mix(d, s, coverage);
                    }
                }
            }
        }
    }
}

/// The soft shadow of a rectangle: two 1-D profiles whose product is the
/// opacity (a Gaussian-blurred box is separable).
#[derive(Clone, Debug, PartialEq)]
pub struct Shadow {
    /// The shadow's own rectangle (frame grown by the blur, moved down).
    pub area: Rect,
    xs: Vec<u16>,
    ys: Vec<u16>,
}

fn erf(x: f32) -> f32 {
    // Abramowitz–Stegun 7.1.26, |error| < 1.5e-7.
    let s = x.signum();
    let x = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * x);
    let y = 1.0
        - (((((1.061_405_4 * t - 1.453_152_1) * t) + 1.421_413_7) * t - 0.284_496_74) * t
            + 0.254_829_6)
            * t
            * (-x * x).exp();
    s * y
}

fn profile(len: i32, blur: i32, strength: f32) -> Vec<u16> {
    let sigma = (blur as f32 / 2.0).max(0.5);
    let total = len + 2 * blur;
    (0..total)
        .map(|i| {
            let c = i as f32 + 0.5 - blur as f32;
            let k = std::f32::consts::SQRT_2 * sigma;
            let v = 0.5 * (erf(c / k) - erf((c - len as f32) / k));
            (v.clamp(0.0, 1.0) * strength * 65535.0) as u16
        })
        .collect()
}

impl Shadow {
    /// Shadow of `frame` (device pixels) with blur and downward offset in
    /// pixels and opacity `alpha` (0–1).
    pub fn new(frame: Rect, blur: i32, offset_y: i32, alpha: f32) -> Shadow {
        let blur = blur.max(1);
        Shadow {
            area: Rect::new(
                frame.x - blur,
                frame.y - blur + offset_y,
                frame.w + 2 * blur,
                frame.h + 2 * blur,
            ),
            xs: profile(frame.w, blur, 1.0),
            ys: profile(frame.h, blur, alpha.clamp(0.0, 1.0)),
        }
    }

    /// Darkens `dst` inside `clip` by the shadow, except inside `hole` (the
    /// frame, which the window covers anyway).
    pub fn paint(&self, dst: &mut Image, clip: &Rect, hole: &Rect) {
        let Some(r) = self
            .area
            .intersect(clip)
            .and_then(|r| r.intersect(&dst.bounds()))
        else {
            return;
        };
        for y in r.y..r.max_y() {
            let py = u64::from(self.ys[(y - self.area.y) as usize]);
            if py == 0 {
                continue;
            }
            // Columns outside the hole only: the frame covers the rest.
            let inside_rows = y >= hole.y && y < hole.max_y();
            let spans = if inside_rows {
                [
                    (r.x, r.max_x().min(hole.x)),
                    (r.x.max(hole.max_x()), r.max_x()),
                ]
            } else {
                [(r.x, r.max_x()), (0, 0)]
            };
            let row = y as usize * dst.width as usize;
            for (x0, x1) in spans {
                for x in x0..x1 {
                    let px = u64::from(self.xs[(x - self.area.x) as usize]);
                    let opacity = ((px * py) >> 24).min(255) as u32;
                    let i = row + x as usize;
                    dst.data[i] = attenuate(dst.data[i], 255 - opacity) | 0xff00_0000;
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "pixels_tests.rs"]
mod tests;
