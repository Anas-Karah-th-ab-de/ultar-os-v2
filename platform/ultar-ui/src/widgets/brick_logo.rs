//! The company logo of KM Code Craft as a picture brick (docs/BRAND.md §3): the bitmap of
//! `ultar-brand` (transparent background), scaled down with an area filter to exactly the
//! device pixels it covers – never up, never recoloured, cropped or mirrored. It stands
//! directly on the surface in both themes, with its clear space around it.

use super::{MeasureCx, Visual};
use crate::geom::{Rect, Size};
use crate::paint::Painter;
use crate::tokens::{Space, ThemeKind};
use std::cell::RefCell;
use std::rc::Rc;
use ultar_brand::logo;

/// Scaled copies kept per thread (screens draw the logo at one or two sizes).
const CACHE: usize = 4;

/// A scaled copy: its size in pixels and its RGBA.
type Scaled = ((u32, u32), Rc<[u8]>);

thread_local! {
    static SCALED: RefCell<Vec<Scaled>> = const { RefCell::new(Vec::new()) };
}

/// Clear space around the logo.
fn clear() -> f32 {
    Space::S12.points()
}

/// Size of a logo `width` points wide at `scale` device pixels per point: the proportions of
/// the bitmap, never wider than the bitmap's own pixels, never narrower than
/// [`logo::MIN_WIDTH`] pixels; the clear space around it.
pub(super) fn measure(cx: &MeasureCx<'_>, width: u16) -> Size {
    let (w, h) = logo_points(f32::from(width), cx.env.scale);
    let pad = 2.0 * clear();
    Size::new(w + pad, h + pad)
}

/// Width and height of the logo itself in points.
fn logo_points(width: f32, scale: f32) -> (f32, f32) {
    let scale = scale.max(0.25);
    let max = logo::WIDTH as f32 / scale;
    let min = logo::MIN_WIDTH as f32 / scale;
    let w = width.clamp(min.min(max), max);
    (w, w * logo::HEIGHT as f32 / logo::WIDTH as f32)
}

/// Strength of a quiet logo (the desktop background) on the dark ground, and on the light
/// ground (its grey name line needs more there to stay readable).
const QUIET: f32 = 0.5;
const QUIET_LIGHT: f32 = 0.75;

/// Paints the logo centred in `rect`, without a plate: the bitmap's background is
/// transparent.
pub(super) fn paint(p: &mut Painter<'_>, rect: Rect, width: u16, quiet: bool, v: &Visual) {
    let _ = v;
    let env = *p.env();
    let pad = clear();
    let inner = Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - 2.0 * pad).max(0.0),
        (rect.h - 2.0 * pad).max(0.0),
    );
    let (w, h) = logo_points(f32::from(width).min(inner.w), env.scale);
    let w = w.min(inner.w);
    let h = h.min(inner.h);
    let at = Rect::new(
        inner.x + (inner.w - w) / 2.0,
        inner.y + (inner.h - h) / 2.0,
        w,
        h,
    );
    // High contrast never dims it.
    let quiet = quiet && !env.high_contrast;
    let strength = if env.theme == ThemeKind::Light {
        QUIET_LIGHT
    } else {
        QUIET
    };
    let (px_w, px_h) = p.pixel_size(at);
    if let Some(rgba) = scaled(px_w, px_h) {
        let previous = quiet.then(|| p.set_opacity(strength));
        p.rgba(&rgba, px_w, px_h, at);
        if let Some(previous) = previous {
            p.set_opacity(previous);
        }
    }
}

/// The logo at `width` × `height` pixels (cached); `None` for an empty size or when the
/// bitmap is missing.
fn scaled(width: u32, height: u32) -> Option<Rc<[u8]>> {
    if width == 0 || height == 0 {
        return None;
    }
    let key = (width, height);
    if let Some(hit) = SCALED.with(|c| {
        c.borrow()
            .iter()
            .find(|(k, _)| *k == key)
            .map(|e| e.1.clone())
    }) {
        return Some(hit);
    }
    let source = logo::bitmap().ok()?;
    let pixels: Rc<[u8]> =
        area_scale(source.rgba, source.width, source.height, width, height).into();
    SCALED.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() >= CACHE {
            c.remove(0);
        }
        c.push((key, pixels.clone()));
    });
    Some(pixels)
}

/// Scales straight-alpha RGBA from `sw` × `sh` to `dw` × `dh` by averaging the covered area
/// (alpha-weighted, so transparent pixels do not darken the edges). Larger targets than the
/// source are sampled, not invented: each target pixel still averages what it covers.
pub(crate) fn area_scale(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let (sw_u, sh_u) = (sw as usize, sh as usize);
    let mut out = vec![0u8; dw as usize * dh as usize * 4];
    if sw == 0 || sh == 0 || src.len() != sw_u * sh_u * 4 {
        return out;
    }
    let fx = sw as f32 / dw as f32;
    let fy = sh as f32 / dh as f32;
    for ty in 0..dh as usize {
        let y0 = ty as f32 * fy;
        let y1 = y0 + fy.max(1.0).min(sh as f32 - y0).max(f32::EPSILON);
        for tx in 0..dw as usize {
            let x0 = tx as f32 * fx;
            let x1 = x0 + fx.max(1.0).min(sw as f32 - x0).max(f32::EPSILON);
            let mut acc = [0f32; 4];
            let mut weight = 0f32;
            let mut sy = y0.floor() as usize;
            while (sy as f32) < y1 && sy < sh_u {
                let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                let mut sx = x0.floor() as usize;
                while (sx as f32) < x1 && sx < sw_u {
                    let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let wgt = wx * wy;
                    let at = (sy * sw_u + sx) * 4;
                    let a = f32::from(src[at + 3]) / 255.0;
                    acc[0] += f32::from(src[at]) * a * wgt;
                    acc[1] += f32::from(src[at + 1]) * a * wgt;
                    acc[2] += f32::from(src[at + 2]) * a * wgt;
                    acc[3] += a * wgt;
                    weight += wgt;
                    sx += 1;
                }
                sy += 1;
            }
            let at = (ty * dw as usize + tx) * 4;
            if weight > 0.0 && acc[3] > 0.0 {
                out[at] = (acc[0] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[at + 1] = (acc[1] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[at + 2] = (acc[2] / acc[3]).round().clamp(0.0, 255.0) as u8;
                out[at + 3] = (acc[3] / weight * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn area_scale_keeps_colour_and_coverage() {
        // 2 × 1: opaque red | transparent → 1 × 1: red at half coverage (no dark fringe)
        let src = [255, 0, 0, 255, 0, 0, 0, 0];
        let out = area_scale(&src, 2, 1, 1, 1);
        assert_eq!(&out[..], &[255, 0, 0, 128]);
        // wrong length: transparent
        assert_eq!(area_scale(&src[..4], 2, 1, 1, 1), vec![0; 4]);
    }

    #[test]
    fn logo_size_stays_inside_the_bitmap() {
        let (w, h) = logo_points(10_000.0, 1.0);
        assert_eq!(w, logo::WIDTH as f32);
        assert!((h - logo::HEIGHT as f32).abs() < 0.01);
        // never below the readable minimum
        assert_eq!(logo_points(10.0, 1.0).0, logo::MIN_WIDTH as f32);
        // a 2× screen: half as many points
        assert_eq!(logo_points(10_000.0, 2.0).0, logo::WIDTH as f32 / 2.0);
    }

    #[test]
    fn the_embedded_logo_scales() {
        let s = scaled(260, 109).unwrap();
        assert_eq!(s.len(), 260 * 109 * 4);
        // the red K of "KM" is in the right half and stays red
        assert!(s
            .chunks_exact(4)
            .any(|p| p[3] > 200 && p[0] > 180 && p[1] < 60));
        // cached: the same pixels again
        assert!(Rc::ptr_eq(&s, &scaled(260, 109).unwrap()));
    }
}
