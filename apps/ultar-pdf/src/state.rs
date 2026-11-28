//! The pure logic of the reader: zoom steps, fit scales, the user's rotation and the maths
//! between a point of the page (PDF points, y up) and a pixel of the shown page picture.
//! Everything here is deterministic and tested without a screen.

/// Zoom steps offered by the tool bar, from small to large.
pub const ZOOM_STEPS: [u16; 9] = [50, 65, 80, 100, 125, 150, 200, 300, 400];
/// Smallest and largest zoom (percent).
pub const MIN_ZOOM: u16 = 50;
pub const MAX_ZOOM: u16 = 400;
/// Gap between two pages in points.
pub const PAGE_GAP: f32 = 16.0;
/// Margin left and right of a page in points.
pub const PAGE_MARGIN: f32 = 20.0;

/// What the tool bar shows as the zoom mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// The page fills the width.
    Width,
    /// The whole page is visible.
    Page,
    /// A fixed percent.
    Zoom(u16),
}

/// Clamps a percent to the allowed range.
pub fn clamp_zoom(p: i32) -> u16 {
    p.clamp(MIN_ZOOM as i32, MAX_ZOOM as i32) as u16
}

/// The next (or previous, with `dir < 0`) step of [`ZOOM_STEPS`] around `p`.
pub fn zoom_step(p: u16, dir: i8) -> u16 {
    let up = dir >= 0;
    let step = if up {
        ZOOM_STEPS.iter().copied().find(|s| *s > p)
    } else {
        ZOOM_STEPS.iter().rev().copied().find(|s| *s < p)
    };
    step.unwrap_or(if up { MAX_ZOOM } else { MIN_ZOOM })
}

/// The scale (points → device pixels) that fits the page width.
pub fn fit_width_scale(view_w: f32, page_w: f32) -> f32 {
    ((view_w - 2.0 * PAGE_MARGIN) / page_w.max(1.0)).max(0.02)
}

/// The scale that fits the whole page in the view.
pub fn fit_page_scale(view_w: f32, view_h: f32, page_w: f32, page_h: f32) -> f32 {
    let sx = (view_w - 2.0 * PAGE_MARGIN) / page_w.max(1.0);
    let sy = (view_h - 2.0 * PAGE_MARGIN) / page_h.max(1.0);
    sx.min(sy).max(0.02)
}

/// The zoom percent a pinch of two fingers gives: the percent at the start times the ratio of
/// the finger distances now and at the start, rounded to 5 % (fewer distinct renders) and
/// clamped to the allowed range.
pub fn pinch_percent(start_percent: u16, start_dist: f32, dist: f32) -> u16 {
    if start_dist < 1.0 {
        return start_percent;
    }
    let raw = start_percent as f32 * dist / start_dist;
    clamp_zoom(((raw / 5.0).round() * 5.0) as i32)
}

/// The user's rotation of the page content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rot {
    None,
    /// 90° clockwise.
    Cw,
    /// Upside down.
    Half,
    /// 90° counter-clockwise.
    Ccw,
}

impl Rot {
    /// The next rotation (clockwise).
    pub fn next(self) -> Rot {
        match self {
            Rot::None => Rot::Cw,
            Rot::Cw => Rot::Half,
            Rot::Half => Rot::Ccw,
            Rot::Ccw => Rot::None,
        }
    }

    /// Quarter turns clockwise.
    pub fn turns(self) -> u8 {
        match self {
            Rot::None => 0,
            Rot::Cw => 1,
            Rot::Half => 2,
            Rot::Ccw => 3,
        }
    }
}

/// A page's display size in points after the user's rotation.
pub fn page_display(w: f32, h: f32, rot: Rot) -> (f32, f32) {
    match rot {
        Rot::None | Rot::Half => (w, h),
        Rot::Cw | Rot::Ccw => (h, w),
    }
}

/// Where the shown picture's point `(u, v)` (0..1 across and down the rotated page) lies in
/// the unrotated rendered picture (0..1 as well). The pages are rendered once and turned
/// while they are drawn, so a rotation costs no new render.
pub fn source_uv(u: f32, v: f32, rot: Rot) -> (f32, f32) {
    match rot {
        Rot::None => (u, v),
        Rot::Cw => (v, 1.0 - u),
        Rot::Half => (1.0 - u, 1.0 - v),
        Rot::Ccw => (1.0 - v, u),
    }
}

/// A point of the page (PDF points, y up, page `w`×`h` unrotated) as a pixel of the shown,
/// rotated picture at `scale` (origin top start of the picture).
pub fn page_to_px(p: (f32, f32), size: (f32, f32), scale: f32, rot: Rot) -> (f32, f32) {
    let (w, h) = (size.0 * scale, size.1 * scale);
    let (ux, uy) = (p.0 * scale, (size.1 - p.1) * scale);
    match rot {
        Rot::None => (ux, uy),
        Rot::Cw => (h - uy, ux),
        Rot::Half => (w - ux, h - uy),
        Rot::Ccw => (uy, w - ux),
    }
}

/// The inverse of [`page_to_px`]: a pixel of the shown picture as a point of the page.
pub fn px_to_page(px: (f32, f32), size: (f32, f32), scale: f32, rot: Rot) -> (f32, f32) {
    let (w, h) = (size.0 * scale, size.1 * scale);
    let (ux, uy) = match rot {
        Rot::None => px,
        Rot::Cw => (px.1, h - px.0),
        Rot::Half => (w - px.0, h - px.1),
        Rot::Ccw => (w - px.1, px.0),
    };
    let s = scale.max(0.0001);
    (ux / s, size.1 - uy / s)
}

/// The box of a page rectangle (x0, y0, x1, y1 in points, y up) in picture pixels, as
/// (left, top, right, bottom).
pub fn rect_to_px(
    r: (f32, f32, f32, f32),
    size: (f32, f32),
    scale: f32,
    rot: Rot,
) -> (f32, f32, f32, f32) {
    let a = page_to_px((r.0, r.1), size, scale, rot);
    let b = page_to_px((r.2, r.3), size, scale, rot);
    (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_steps_stay_inside_the_range() {
        assert_eq!(zoom_step(100, 1), 125);
        assert_eq!(zoom_step(100, -1), 80);
        assert_eq!(zoom_step(400, 1), 400);
        assert_eq!(zoom_step(50, -1), 50);
        assert_eq!(zoom_step(90, 1), 100);
        assert_eq!(zoom_step(90, -1), 80);
        assert_eq!(zoom_step(399, 1), 400);
        assert_eq!(zoom_step(51, -1), 50);
        assert_eq!(clamp_zoom(800), 400);
        assert_eq!(clamp_zoom(0), 50);
    }

    #[test]
    fn fit_keeps_the_page_fully_visible() {
        let sw = fit_width_scale(800.0, 595.0);
        assert!((595.0 * sw) <= 800.0 - 2.0 * PAGE_MARGIN + 0.01);
        let sp = fit_page_scale(800.0, 500.0, 595.0, 842.0);
        assert!(842.0 * sp <= 500.0 - 2.0 * PAGE_MARGIN + 0.01);
        assert!(sp < sw);
    }

    #[test]
    fn a_pinch_scales_and_clamps_the_zoom() {
        assert_eq!(pinch_percent(100, 100.0, 200.0), 200);
        assert_eq!(pinch_percent(100, 100.0, 50.0), 50);
        assert_eq!(pinch_percent(100, 100.0, 1000.0), 400);
        assert_eq!(pinch_percent(100, 100.0, 103.0), 105);
        assert_eq!(pinch_percent(120, 0.0, 10.0), 120);
    }

    #[test]
    fn rotation_swaps_the_page_size_and_maps_the_picture() {
        assert_eq!(page_display(595.0, 842.0, Rot::Cw), (842.0, 595.0));
        assert_eq!(page_display(595.0, 842.0, Rot::Half), (595.0, 842.0));
        assert_eq!(Rot::None.next().next().next().next(), Rot::None);
        // The top-start corner of the shown page, per rotation, in the rendered picture:
        // turned clockwise it is the rendered picture's bottom-start corner, and so on.
        assert_eq!(source_uv(0.0, 0.0, Rot::None), (0.0, 0.0));
        assert_eq!(source_uv(0.0, 0.0, Rot::Cw), (0.0, 1.0));
        assert_eq!(source_uv(0.0, 0.0, Rot::Half), (1.0, 1.0));
        assert_eq!(source_uv(0.0, 0.0, Rot::Ccw), (1.0, 0.0));
        // The mapping and the point maths agree: a page point drawn at pixel p in the
        // rotated picture is sampled from the same spot of the rendered one.
        let (size, scale) = ((200.0, 100.0), 2.0);
        for rot in [Rot::None, Rot::Cw, Rot::Half, Rot::Ccw] {
            let (dw, dh) = page_display(size.0, size.1, rot);
            let (px, py) = page_to_px((50.0, 80.0), size, scale, rot);
            let (u, v) = source_uv(px / (dw * scale), py / (dh * scale), rot);
            let (ux, uy) = (50.0 * scale / (size.0 * scale), (size.1 - 80.0) / size.1);
            assert!((u - ux).abs() < 1e-4 && (v - uy).abs() < 1e-4, "{rot:?}");
        }
    }

    #[test]
    fn page_points_follow_the_rotation_and_come_back() {
        let size = (200.0, 100.0);
        // The top-start corner of the page is (0, 100) in PDF points.
        assert_eq!(page_to_px((0.0, 100.0), size, 2.0, Rot::None), (0.0, 0.0));
        // Turned clockwise the old top-start corner is at the top end of a 200×400 picture.
        assert_eq!(page_to_px((0.0, 100.0), size, 2.0, Rot::Cw), (200.0, 0.0));
        assert_eq!(
            page_to_px((0.0, 100.0), size, 2.0, Rot::Half),
            (400.0, 200.0)
        );
        assert_eq!(page_to_px((0.0, 100.0), size, 2.0, Rot::Ccw), (0.0, 400.0));
        for rot in [Rot::None, Rot::Cw, Rot::Half, Rot::Ccw] {
            let p = (37.0, 61.0);
            let back = px_to_page(page_to_px(p, size, 1.5, rot), size, 1.5, rot);
            assert!(
                (back.0 - p.0).abs() < 0.01 && (back.1 - p.1).abs() < 0.01,
                "{rot:?}"
            );
        }
        let r = rect_to_px((10.0, 10.0, 30.0, 20.0), size, 1.0, Rot::None);
        assert_eq!(r, (10.0, 80.0, 30.0, 90.0));
    }
}
