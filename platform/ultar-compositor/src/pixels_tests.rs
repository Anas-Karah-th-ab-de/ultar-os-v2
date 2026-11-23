use super::*;

const RED: u32 = 0xffff_0000;
const BLUE: u32 = 0xff00_00ff;
const ALL: Rect = Rect::new(-1000, -1000, 100_000, 100_000);

#[test]
fn images_are_bounded_and_convert_bytes() {
    assert!(Image::new(0, 1, 0).is_none());
    assert!(Image::new(MAX_IMAGE_SIDE + 1, 1, 0).is_none());
    let img = Image::from_bytes(2, 1, &[1, 2, 3, 0, 4, 5, 6, 0], true).unwrap();
    assert_eq!(img.data(), &[0xff03_0201, 0xff06_0504]);
    assert!(Image::from_bytes(2, 1, &[0; 7], true).is_none());
    assert_eq!(img.to_bytes(Rect::new(1, 0, 5, 5)), vec![4, 5, 6, 255]);
    assert!(img.to_bytes(Rect::new(9, 9, 1, 1)).is_empty());
    assert_eq!(img.pixel(2, 0), None);
    assert_eq!(img.bounds(), Rect::new(0, 0, 2, 1));
}

#[test]
fn over_matches_the_formula() {
    assert_eq!(over(RED, BLUE), BLUE);
    assert_eq!(over(RED, 0), RED);
    // 50 % black over white.
    let half_black = 0x8000_0000;
    let out = over(0xffff_ffff, half_black);
    for shift in [0, 8, 16] {
        let c = (out >> shift) & 0xff;
        assert!((126..=128).contains(&c), "{out:08x}");
    }
    assert_eq!(out >> 24, 0xff);
    // Exhaustive check against the per-channel formula for a sample.
    for a in (0..=255u32).step_by(17) {
        for d in (0..=255u32).step_by(15) {
            let src = a << 24 | (a / 2) << 16;
            let dst = 0xff00_0000 | d << 16 | d;
            let got = over(dst, src);
            let want_r = (a / 2) + (d * (255 - a) + 127) / 255;
            assert!(((got >> 16) & 0xff).abs_diff(want_r) <= 1);
        }
    }
}

#[test]
fn copy_fill_and_blend_are_clipped() {
    let mut dst = Image::new(4, 4, RED).unwrap();
    let src = Image::new(3, 3, BLUE).unwrap();
    copy(&mut dst, 2, 2, &src, src.bounds(), &ALL);
    assert_eq!(dst.pixel(1, 1), Some(RED));
    assert_eq!(dst.pixel(3, 3), Some(BLUE));
    copy(&mut dst, -2, -2, &src, src.bounds(), &Rect::new(0, 0, 1, 1));
    assert_eq!(dst.pixel(0, 0), Some(BLUE));
    assert_eq!(dst.pixel(1, 0), Some(RED));
    fill(&mut dst, Rect::new(0, 0, 10, 10), &Rect::new(1, 0, 1, 1), 7);
    assert_eq!(dst.pixel(1, 0), Some(7));
    let half = Image::new(1, 1, 0x8000_0000).unwrap();
    blend(&mut dst, 0, 3, &half, half.bounds(), &ALL);
    assert!(dst.pixel(0, 3).unwrap() & 0x00ff_0000 < 0x0090_0000);
    // Source rectangle partly outside the source.
    copy(&mut dst, 0, 0, &src, Rect::new(2, 2, 5, 5), &ALL);
    assert_eq!(dst.pixel(0, 0), Some(BLUE));
    assert_eq!(
        dst.pixel(1, 1),
        Some(RED),
        "only the one source pixel inside exists"
    );
    // Completely outside: nothing happens.
    copy(&mut dst, 100, 100, &src, src.bounds(), &ALL);
    blend(&mut dst, -100, 0, &src, src.bounds(), &ALL);
}

#[test]
fn nearest_neighbour_scaling() {
    let mut src = Image::new(2, 1, RED).unwrap();
    src.data_mut()[1] = BLUE;
    let mut dst = Image::new(4, 2, 0).unwrap();
    scale(&mut dst, Rect::new(0, 0, 4, 2), &ALL, &src, true);
    assert_eq!(dst.data(), &[RED, RED, BLUE, BLUE, RED, RED, BLUE, BLUE]);
    let mut small = Image::new(1, 1, 0).unwrap();
    scale(&mut small, Rect::new(0, 0, 1, 1), &ALL, &src, false);
    assert_eq!(small.data(), &[RED]);
}

#[test]
fn rounded_corners_blend_only_the_corner() {
    let mask = CornerMask::new(4);
    assert_eq!(mask.radius(), 4);
    assert_eq!(mask.coverage().len(), 16);
    assert!(
        mask.coverage()[0] < 40,
        "outermost corner pixel is mostly outside"
    );
    assert_eq!(mask.coverage()[15], 255);
    assert_eq!(CornerMask::new(0).radius(), 1);
    let mut dst = Image::new(8, 8, RED).unwrap();
    let src = Image::new(8, 8, BLUE).unwrap();
    mask.apply(&mut dst, Rect::new(0, 0, 8, 8), &ALL, &src, false);
    let mut low = Image::new(8, 8, RED).unwrap();
    mask.apply(&mut low, Rect::new(0, 0, 8, 8), &ALL, &src, true);
    assert!(low.pixel(0, 7).unwrap() & 0xff0000 > 0x800000);
    assert_eq!(low.pixel(0, 0), Some(RED));
    assert!(
        dst.pixel(0, 0).unwrap() & 0xff0000 > 0x800000,
        "corner stays mostly red"
    );
    assert_eq!(dst.pixel(3, 3), Some(BLUE));
    assert_eq!(
        dst.pixel(7, 7),
        Some(RED),
        "bottom corners untouched when not round"
    );
    assert!(dst.pixel(7, 0).unwrap() & 0xff0000 > 0x800000);
    let mut low = Image::new(8, 8, RED).unwrap();
    mask.apply(&mut low, Rect::new(0, 0, 8, 8), &ALL, &src, true);
    assert!(low.pixel(0, 7).unwrap() & 0xff0000 > 0x800000);
    assert_eq!(low.pixel(0, 0), Some(RED));
}

#[test]
fn shadow_darkens_around_the_frame_only() {
    let frame = Rect::new(20, 20, 20, 20);
    let shadow = Shadow::new(frame, 8, 4, 0.4);
    assert_eq!(shadow.area, Rect::new(12, 16, 36, 36));
    let mut dst = Image::new(60, 60, 0xffff_ffff).unwrap();
    shadow.paint(&mut dst, &ALL, &frame);
    assert_eq!(dst.pixel(30, 30), Some(0xffff_ffff), "inside the hole");
    let below = dst.pixel(30, 42).unwrap() & 0xff;
    assert!(below < 0xf0 && below > 0x90, "{below:x}");
    assert_eq!(dst.pixel(0, 0), Some(0xffff_ffff), "far away");
    let far_below = dst.pixel(30, 51).unwrap() & 0xff;
    assert!(far_below > below);
    assert!(
        (erf(0.0)).abs() < 1e-6 && (erf(3.0) - 1.0).abs() < 1e-4 && (erf(-3.0) + 1.0).abs() < 1e-4
    );
}
