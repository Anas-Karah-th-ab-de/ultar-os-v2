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
    // `div255` in 16-bit arithmetic (every intermediate stays below 2^16: 255 · 255 + 128 +
    // 254), one constant per channel; the compiler turns this loop into vector code. The
    // alpha byte blends with 255, which is `da + (255 − da) · a / 255` of `blend_px`.
    let inv = (FULL - a) as u16;
    let bias = [src[0], src[1], src[2], u8::MAX].map(|c| u16::from(c) * a as u16 + 128);
    let wide: [u16; BLOCK] = std::array::from_fn(|i| bias[i % BPP]);
    let mut blocks = dst.chunks_exact_mut(BLOCK);
    for block in &mut blocks {
        for (byte, add) in block.iter_mut().zip(wide) {
            *byte = mix(*byte, inv, add);
        }
    }
    for px in blocks.into_remainder().chunks_exact_mut(BPP) {
        for (byte, add) in px.iter_mut().zip(bias) {
            *byte = mix(*byte, inv, add);
        }
    }
}

/// One channel of a constant blend: `(byte · inv + add) / 255` with `add` holding the
/// colour term and the rounding, all in 16 bits.
#[inline(always)]
fn mix(byte: u8, inv: u16, add: u16) -> u8 {
    let t = u16::from(byte) * inv + add;
    ((t + (t >> 8)) >> 8) as u8
}

/// Bytes blended per step of the vector loop (eight pixels).
const BLOCK: usize = 8 * BPP;

/// Blends a constant colour with a coverage per pixel (`alphas`, 0..=255) over a row slice;
/// exactly `blend_px` for every pixel, in 16-bit arithmetic the compiler vectorises.
#[inline]
pub(crate) fn blend_alphas(dst: &mut [u8], alphas: &[u32], src: [u8; BPP]) {
    // Runs of equal kind: transparent pixels are skipped, opaque ones written, only the
    // partly covered ones blended (most of a shape's edge zone is one of the first two).
    let kind = |a: u32| (a > 0) as u8 + (a >= FULL) as u8;
    let n = alphas.len().min(dst.len() / BPP);
    let mut start = 0;
    while start < n {
        let first = kind(alphas[start]);
        let end = alphas[start..n]
            .iter()
            .position(|a| kind(*a) != first)
            .map_or(n, |len| start + len);
        let run = &mut dst[start * BPP..end * BPP];
        match first {
            0 => {}
            2 => {
                let opaque = [src[0], src[1], src[2], u8::MAX];
                for px in run.chunks_exact_mut(BPP) {
                    px.copy_from_slice(&opaque);
                }
            }
            _ => blend_varying(run, &alphas[start..end], src),
        }
        start = end;
    }
}

/// The partly covered run of [`blend_alphas`].
fn blend_varying(dst: &mut [u8], alphas: &[u32], src: [u8; BPP]) {
    let colour = [src[0], src[1], src[2], u8::MAX].map(u16::from);
    for (px, &a) in dst.chunks_exact_mut(BPP).zip(alphas) {
        let a = a.min(FULL) as u16;
        let inv = FULL as u16 - a;
        for (byte, c) in px.iter_mut().zip(colour) {
            *byte = mix(*byte, inv, c * a + 128);
        }
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

    #[test]
    fn per_pixel_alphas_blend_exactly_like_single_pixels() {
        let mut base = vec![0u8; 256 * BPP];
        for (i, b) in base.iter_mut().enumerate() {
            *b = ((i / BPP * 7 + i % BPP * 61) % 256) as u8;
        }
        let alphas: Vec<u32> = (0..256).collect();
        for src in [[12, 200, 77, 0], [255, 255, 255, 9], [0, 0, 0, 0]] {
            let (mut row, mut each) = (base.clone(), base.clone());
            blend_alphas(&mut row, &alphas, src);
            for (px, &a) in each.chunks_exact_mut(BPP).zip(&alphas) {
                blend_px(px, src, a);
            }
            assert_eq!(row, each, "{src:?}");
        }
    }

    #[test]
    fn long_spans_blend_exactly_like_single_pixels() {
        // Every byte value under every coverage and several colours, against `blend_px`.
        let mut base = vec![0u8; 256 * BPP];
        for (i, b) in base.iter_mut().enumerate() {
            *b = ((i / BPP + i % BPP * 61) % 256) as u8;
        }
        for src in [[12, 200, 77, 0], [0, 0, 0, 0], [255, 255, 255, 9]] {
            for a in 1..255 {
                let (mut long, mut each) = (base.clone(), base.clone());
                blend_span(&mut long, src, a);
                for px in each.chunks_exact_mut(BPP) {
                    blend_px(px, src, a);
                }
                assert_eq!(long, each, "{src:?} {a}");
            }
        }
    }
}
