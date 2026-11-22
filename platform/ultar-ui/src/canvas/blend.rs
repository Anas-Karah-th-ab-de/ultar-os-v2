//! Pixel blending primitives (straight alpha, source-over). No allocation, row slices only.

/// Bytes per pixel of every supported format.
pub(crate) const BPP: usize = 4;
const FULL: u32 = 255;

/// Exact `v / 255` with rounding for `v` in `0..=255*255`.
#[inline]
fn div255(v: u32) -> u32 {
    let t = v + 128;
    (t + (t >> 8)) >> 8
}

/// Blends one pixel. `src` is in buffer byte order (colour bytes 0..3); `a` is 0..=255.
#[inline]
pub(crate) fn blend_px(dst: &mut [u8], src: [u8; BPP], a: u32) {
    let inv = FULL - a;
    for i in 0..3 {
        dst[i] = div255(u32::from(src[i]) * a + u32::from(dst[i]) * inv) as u8;
    }
    let da = u32::from(dst[3]);
    dst[3] = (da + div255((FULL - da) * a)) as u8;
}

/// Blends a constant colour with constant coverage over a row slice.
#[inline]
pub(crate) fn blend_span(dst: &mut [u8], src: [u8; BPP], a: u32) {
    if a == 0 {
        return;
    }
    if a >= FULL {
        let opaque = [src[0], src[1], src[2], u8::MAX];
        for px in dst.chunks_exact_mut(BPP) {
            px.copy_from_slice(&opaque);
        }
        return;
    }
    for px in dst.chunks_exact_mut(BPP) {
        blend_px(px, src, a);
    }
}

/// Blends a constant colour through an 8-bit coverage mask row; `alpha` scales the mask.
#[inline]
pub(crate) fn blend_mask_row(dst: &mut [u8], mask: &[u8], src: [u8; BPP], alpha: u32) {
    for (px, &m) in dst.chunks_exact_mut(BPP).zip(mask) {
        if m != 0 {
            blend_px(px, src, div255(u32::from(m) * alpha));
        }
    }
}

/// Converts coverage (0..=1) and colour opacity into a blend factor.
#[inline]
pub(crate) fn alpha_of(coverage: f32, color_alpha: u8) -> u32 {
    (coverage.clamp(0.0, 1.0) * f32::from(color_alpha) + 0.5) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn div255_is_exact() {
        for v in 0..=(255u32 * 255) {
            assert_eq!(div255(v), (v + 127) / 255, "{v}");
        }
    }

    #[test]
    fn blend_extremes_and_half() {
        let mut px = [10, 20, 30, 255];
        blend_px(&mut px, [200, 100, 0, 0], 0);
        assert_eq!(px, [10, 20, 30, 255]);
        blend_px(&mut px, [200, 100, 0, 0], 255);
        assert_eq!(px, [200, 100, 0, 255]);
        let mut px = [0, 0, 0, 0];
        blend_px(&mut px, [255, 255, 255, 0], 128);
        assert_eq!(px, [128, 128, 128, 128]);
    }

    #[test]
    fn spans_and_masks() {
        let mut row = [0u8; 12];
        blend_span(&mut row, [1, 2, 3, 0], 255);
        assert_eq!(row, [1, 2, 3, 255, 1, 2, 3, 255, 1, 2, 3, 255]);
        let mut row = [0u8; 8];
        blend_span(&mut row, [9, 9, 9, 0], 0);
        assert_eq!(row, [0u8; 8]);
        blend_mask_row(&mut row, &[0, 255], [50, 60, 70, 0], 255);
        assert_eq!(row, [0, 0, 0, 0, 50, 60, 70, 255]);
        assert_eq!(alpha_of(2.0, 255), 255);
        assert_eq!(alpha_of(0.5, 255), 128);
        assert_eq!(alpha_of(-1.0, 255), 0);
    }
}
