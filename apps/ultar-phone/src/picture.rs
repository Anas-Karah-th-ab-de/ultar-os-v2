//! The other side's picture of a video call (`call.picture`): a JPEG from the call service,
//! decoded into a canvas for the picture node of the call screen.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::PixelRect;

/// Largest picture shown (a call sends 320×240; more is a broken or hostile sender).
pub const MAX_SIDE: usize = 1280;

/// The picture in `jpeg` as a canvas, made smaller to fit into `fit` (width, height) when given
/// (`None`: not a JPEG, or too large).
pub fn canvas(jpeg: &[u8], fit: Option<(usize, usize)>) -> Option<Canvas> {
    let options = zune_jpeg::zune_core::options::DecoderOptions::default()
        .set_max_width(MAX_SIDE)
        .set_max_height(MAX_SIDE)
        .jpeg_set_out_colorspace(zune_jpeg::zune_core::colorspace::ColorSpace::RGB);
    let mut d = zune_jpeg::JpegDecoder::new_with_options(
        zune_jpeg::zune_core::bytestream::ZCursor::new(jpeg),
        options,
    );
    let rgb = d.decode().ok()?;
    let (sw, sh) = d.dimensions()?;
    if sw == 0 || sh == 0 || rgb.len() != sw * sh * 3 {
        return None;
    }
    let (w, h) = match fit {
        // the whole picture in the box, sides in the picture's ratio
        Some((bw, bh)) if sw > bw || sh > bh => {
            let s = (bw as f64 / sw as f64).min(bh as f64 / sh as f64);
            (
                ((sw as f64 * s) as usize).max(1),
                ((sh as f64 * s) as usize).max(1),
            )
        }
        _ => (sw, sh),
    };
    let mut rows = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let row = (y * sh / h) * sw;
        for x in 0..w {
            let p = &rgb[(row + x * sw / w) * 3..][..3];
            rows.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    let mut c = Canvas::new(
        u32::try_from(w).ok()?,
        u32::try_from(h).ok()?,
        PixelFormat::Bgrx8,
    )
    .ok()?;
    let area = PixelRect::new(0, 0, i32::try_from(w).ok()?, i32::try_from(h).ok()?);
    c.write_bgrx(area, &rows, w * 4).ok()?;
    c.take_damage();
    Some(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 16×8 JPEG: red left half, blue right half.
    fn sample() -> Vec<u8> {
        let mut rgb = Vec::new();
        for _ in 0..8 {
            for x in 0..16 {
                rgb.extend_from_slice(if x < 8 {
                    &[220, 20, 20]
                } else {
                    &[20, 20, 220]
                });
            }
        }
        let mut out = Vec::new();
        jpeg_encoder::Encoder::new(&mut out, 90)
            .encode(&rgb, 16, 8, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        out
    }

    #[test]
    fn a_jpeg_becomes_a_canvas_of_its_size() {
        let c = canvas(&sample(), None).unwrap();
        assert_eq!((c.width(), c.height()), (16, 8));
        assert!(canvas(b"no picture", None).is_none());
        assert!(canvas(&[], None).is_none());
        // made smaller to fit, in its ratio
        let c = canvas(&sample(), Some((8, 8))).unwrap();
        assert_eq!((c.width(), c.height()), (8, 4));
        let c = canvas(&sample(), Some((64, 64))).unwrap();
        assert_eq!((c.width(), c.height()), (16, 8));
    }
}
