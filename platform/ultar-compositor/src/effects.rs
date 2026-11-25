//! The accessibility view effects on the composed frame: the magnifier (zoom
//! around the pointer), the colour filter (a final colour matrix) and the
//! visual-alert flash. They are applied to a *copy* of the composed frame
//! (`transformed`), so the incremental composition keeps the clean image and the
//! effects never accumulate frame over frame.

use crate::geom::Rect;
use crate::pixels::{self, Image};
use crate::state::Visual;
use ultar_surface::proto::ColourFilter;

/// The colour of the visual-alert flash: white at half opacity (premultiplied,
/// like the scrim: alpha 0x80 and the channels 0x80).
pub const FLASH_COLOR: u32 = 0x8080_8080;

/// The colour matrix of a filter in 1/1024 units (rows R, G, B out; columns R, G,
/// B in). The colour-blind modes are the usual Viénot–Brettel simulation matrices;
/// greyscale is the Rec. 709 luminance weights. `None` and `Invert` have no matrix.
fn matrix(filter: ColourFilter) -> Option<[[i32; 3]; 3]> {
    Some(match filter {
        ColourFilter::Greyscale => [[218, 732, 74], [218, 732, 74], [218, 732, 74]],
        ColourFilter::Protanopia => [[581, 443, 0], [571, 453, 0], [0, 248, 776]],
        ColourFilter::Deuteranopia => [[640, 384, 0], [717, 307, 0], [0, 307, 717]],
        ColourFilter::Tritanopia => [[973, 51, 0], [0, 443, 581], [0, 486, 538]],
        _ => return None,
    })
}

/// Inverts the colour of a premultiplied pixel (unpremultiply, invert, premultiply
/// again); a fully transparent pixel stays as it is.
fn invert(px: u32) -> u32 {
    let a = px >> 24;
    if a == 0 {
        return px;
    }
    let straight = |c: u32| c * 255 / a;
    let premultiply = |c: u32| c * a / 255;
    let r = premultiply(255 - straight((px >> 16) & 0xff));
    let g = premultiply(255 - straight((px >> 8) & 0xff));
    let b = premultiply(255 - straight(px & 0xff));
    (a << 24) | (r << 16) | (g << 8) | b
}

/// One pixel through the colour filter. The matrix filters are linear, so they
/// apply to the premultiplied channels directly and keep the alpha; invert is the
/// one non-linear mode.
pub fn filter_pixel(px: u32, filter: ColourFilter) -> u32 {
    let Some(m) = matrix(filter) else {
        return if filter == ColourFilter::Invert {
            invert(px)
        } else {
            px
        };
    };
    let (r, g, b) = (
        ((px >> 16) & 0xff) as i32,
        ((px >> 8) & 0xff) as i32,
        (px & 0xff) as i32,
    );
    let out = |row: [i32; 3]| ((row[0] * r + row[1] * g + row[2] * b) >> 10).clamp(0, 255) as u32;
    (px & 0xff00_0000) | (out(m[0]) << 16) | (out(m[1]) << 8) | out(m[2])
}

/// The magnifier: `dst` (the same size as `src`) shows `src` scaled by
/// `percent / 100` around `center`, nearest neighbour, clamped to the edges so the
/// view never reaches outside the frame. The point under `center` stays fixed.
pub fn magnify(src: &Image, dst: &mut Image, center: (i32, i32), percent: u16) {
    let (w, h) = (src.width() as i32, src.height() as i32);
    if w == 0 || h == 0 || dst.width() != src.width() || dst.height() != src.height() {
        return;
    }
    // 16.16 fixed point: the source step per destination pixel (100 / percent).
    let inv = (100i64 << 16) / i64::from(percent.max(100));
    let (cx, cy) = (center.0.clamp(0, w - 1), center.1.clamp(0, h - 1));
    let cols: Vec<usize> = (0..w)
        .map(|x| {
            (i64::from(cx) + (((x - cx) as i64 * inv) >> 16)).clamp(0, i64::from(w - 1)) as usize
        })
        .collect();
    let from = src.data();
    let to = dst.data_mut();
    for y in 0..h {
        let sy =
            (i64::from(cy) + (((y - cy) as i64 * inv) >> 16)).clamp(0, i64::from(h - 1)) as usize;
        let row = &from[sy * w as usize..(sy + 1) * w as usize];
        let out = &mut to[y as usize * w as usize..(y as usize + 1) * w as usize];
        for (x, &sx) in cols.iter().enumerate() {
            out[x] = row[sx];
        }
    }
}

/// The visual-alert flash: a translucent white veil over `clip` of `dst`.
pub fn flash(dst: &mut Image, clip: &Rect) {
    if let Some(tile) = Image::new(1, 1, FLASH_COLOR) {
        let bounds = dst.bounds();
        pixels::scale(dst, bounds, clip, &tile, false);
    }
}

/// The frame to present: `frame` with the magnifier, the colour filter and the
/// flash applied, or `None` when nothing changes it (then `frame` is presented as
/// it is). `center` is the pointer in device pixels of the output.
pub fn transformed(v: &Visual, now: u64, center: (i32, i32), frame: &Image) -> Option<Image> {
    if !v.active(now) {
        return None;
    }
    let mut shown = Image::new(frame.width(), frame.height(), 0)?;
    if v.zoom_active() {
        magnify(frame, &mut shown, center, v.zoom_percent);
    } else {
        shown.data_mut().copy_from_slice(frame.data());
    }
    if v.filter != ColourFilter::None {
        for px in shown.data_mut() {
            *px = filter_pixel(*px, v.filter);
        }
    }
    if now < v.flash_until {
        let bounds = shown.bounds();
        flash(&mut shown, &bounds);
    }
    Some(shown)
}

#[cfg(test)]
#[path = "effects_tests.rs"]
mod tests;
