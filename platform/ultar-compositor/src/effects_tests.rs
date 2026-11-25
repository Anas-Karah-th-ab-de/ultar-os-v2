//! Tests of the accessibility view effects (see `effects.rs`).

use super::*;

/// A 4×4 image whose red channel is the pixel index (x + 4·y).
fn gradient() -> Image {
    let mut img = Image::new(4, 4, 0xff00_0000).unwrap();
    for y in 0..4 {
        for x in 0..4 {
            img.data_mut()[(y * 4 + x) as usize] = 0xff00_0000 | ((x + 4 * y) << 16);
        }
    }
    img
}

#[test]
fn the_colour_filters_keep_alpha_and_transform_colour() {
    // None is the identity.
    assert_eq!(filter_pixel(0xff12_3456, ColourFilter::None), 0xff12_3456);
    // Greyscale: pure red becomes its luminance (218/1024 · 255 = 54).
    assert_eq!(
        filter_pixel(0xffff_0000, ColourFilter::Greyscale),
        0xff36_3636
    );
    // White stays white (the rows sum to 1024).
    assert_eq!(
        filter_pixel(0xffff_ffff, ColourFilter::Greyscale),
        0xffff_ffff
    );
    // Invert of an opaque colour.
    assert_eq!(filter_pixel(0xff10_2030, ColourFilter::Invert), 0xffef_dfcf);
    // Invert unpremultiplies and premultiplies again: half-opacity red becomes
    // half-opacity cyan, alpha kept.
    assert_eq!(filter_pixel(0x8080_0000, ColourFilter::Invert), 0x8000_8080);
    // A matrix filter keeps the alpha too.
    assert_eq!(
        filter_pixel(0x8080_0000, ColourFilter::Greyscale),
        0x801b_1b1b
    );
    // A fully transparent pixel is left alone by invert.
    assert_eq!(filter_pixel(0x0000_0000, ColourFilter::Invert), 0x0000_0000);
    // The colour-blind modes change a saturated colour and keep alpha.
    for filter in [
        ColourFilter::Protanopia,
        ColourFilter::Deuteranopia,
        ColourFilter::Tritanopia,
    ] {
        let p = filter_pixel(0xff00_ff00, filter);
        assert_eq!(p >> 24, 0xff, "{filter:?} keeps alpha");
        assert_ne!(p, 0xff00_ff00, "{filter:?} changes the colour");
    }
}

#[test]
fn the_magnifier_keeps_the_centre_fixed_and_doubles_the_rest() {
    let src = gradient();
    let mut dst = Image::new(4, 4, 0).unwrap();
    magnify(&src, &mut dst, (1, 1), 200);
    // The centre pixel stays; its neighbours sample half as far from the centre.
    assert_eq!(dst.pixel(1, 1), src.pixel(1, 1), "centre fixed");
    assert_eq!(dst.pixel(0, 1), src.pixel(0, 1));
    assert_eq!(
        dst.pixel(3, 1),
        src.pixel(2, 1),
        "200 %: two dest pixels per source"
    );
    assert_eq!(dst.pixel(1, 3), src.pixel(1, 2));
    // 100 % is the identity.
    let mut same = Image::new(4, 4, 0).unwrap();
    magnify(&src, &mut same, (1, 1), 100);
    assert_eq!(same.data(), src.data());
    // The centre is clamped into the frame (a pointer on another output).
    let mut edge = Image::new(4, 4, 0).unwrap();
    magnify(&src, &mut edge, (99, 99), 200);
    assert_eq!(
        edge.pixel(3, 3),
        src.pixel(3, 3),
        "clamped centre stays fixed"
    );
}

#[test]
fn the_flash_lightens_the_frame() {
    let mut img = Image::new(2, 2, 0xff00_0000).unwrap();
    let bounds = img.bounds();
    flash(&mut img, &bounds);
    let p = img.pixel(0, 0).unwrap();
    assert!(p > 0xff00_0000, "black becomes lighter: {p:#010x}");
    assert!((p & 0xff) > 100 && (p & 0xff) < 200, "about half white");
}

#[test]
fn transformed_applies_only_what_is_active() {
    let frame = gradient();
    let mut v = Visual::default();
    assert!(
        transformed(&v, 0, (1, 1), &frame).is_none(),
        "defaults: nothing"
    );
    // A filter alone.
    v.filter = ColourFilter::Greyscale;
    let shown = transformed(&v, 0, (1, 1), &frame).unwrap();
    assert_eq!(
        shown.pixel(0, 0),
        Some(filter_pixel(frame.pixel(0, 0).unwrap(), v.filter))
    );
    // The magnifier alone keeps the centre.
    v.filter = ColourFilter::None;
    v.zoom = true;
    v.zoom_percent = 200;
    let shown = transformed(&v, 0, (1, 1), &frame).unwrap();
    assert_eq!(shown.pixel(1, 1), frame.pixel(1, 1));
    // The flash alone shows until its deadline, then nothing.
    v.zoom = false;
    v.flash_until = 100;
    assert!(transformed(&v, 99, (1, 1), &frame).is_some());
    assert!(transformed(&v, 100, (1, 1), &frame).is_none());
}
