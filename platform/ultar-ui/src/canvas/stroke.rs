//! Anti-aliased strokes with round caps and joins, computed as coverage of the distance to
//! line segments. Shared by canvas lines and the icon rasteriser.

use crate::geom::Point;

/// Squared distance from `p` to the segment `a`–`b`.
#[inline]
fn dist_sq(p: Point, a: Point, b: Point) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let (wx, wy) = (p.x - a.x, p.y - a.y);
    let len_sq = vx * vx + vy * vy;
    let t = if len_sq > 0.0 {
        ((wx * vx + wy * vy) / len_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let (dx, dy) = (wx - t * vx, wy - t * vy);
    dx * dx + dy * dy
}

/// Adds the coverage of a stroked segment to an 8-bit mask of `width` × `height` pixels
/// (maximum with what is there, so overlapping segments of one path do not double up).
/// Coordinates are in mask pixels; `half` is half the stroke width.
pub(crate) fn stroke_segment(
    mask: &mut [u8],
    width: usize,
    height: usize,
    a: Point,
    b: Point,
    half: f32,
) {
    if width == 0 || height == 0 || mask.len() < width * height {
        return;
    }
    let reach = half + 1.0;
    let x0 = (a.x.min(b.x) - reach).floor().max(0.0) as usize;
    let y0 = (a.y.min(b.y) - reach).floor().max(0.0) as usize;
    let x1 = ((a.x.max(b.x) + reach).ceil().max(0.0) as usize).min(width);
    let y1 = ((a.y.max(b.y) + reach).ceil().max(0.0) as usize).min(height);
    let outer = (half + 0.5) * (half + 0.5);
    for y in y0..y1 {
        let row = &mut mask[y * width..(y + 1) * width];
        for (x, cell) in row.iter_mut().enumerate().take(x1).skip(x0) {
            let d2 = dist_sq(Point::new(x as f32 + 0.5, y as f32 + 0.5), a, b);
            if d2 >= outer {
                continue;
            }
            let cov = (half + 0.5 - d2.sqrt()).clamp(0.0, 1.0);
            let value = (cov * f32::from(u8::MAX) + 0.5) as u8;
            if value > *cell {
                *cell = value;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_stroke_covers_its_width() {
        let (w, h) = (20usize, 9usize);
        let mut mask = vec![0u8; w * h];
        stroke_segment(
            &mut mask,
            w,
            h,
            Point::new(4.0, 4.5),
            Point::new(16.0, 4.5),
            1.5,
        );
        assert_eq!(mask[4 * w + 10], 255, "centre row is fully covered");
        assert_eq!(mask[3 * w + 10], 255);
        assert_eq!(mask[5 * w + 10], 255);
        assert_eq!(mask[w + 10], 0, "three pixels away is untouched");
        assert!(mask[4 * w + 2] > 0, "round cap extends past the end point");
        assert_eq!(mask[4 * w], 0);
    }

    #[test]
    fn dot_and_out_of_range_are_safe() {
        let mut mask = vec![0u8; 16];
        stroke_segment(
            &mut mask,
            4,
            4,
            Point::new(2.0, 2.0),
            Point::new(2.0, 2.0),
            1.0,
        );
        assert!(mask[5] > 0 && mask[10] > 0);
        let before = mask.clone();
        stroke_segment(
            &mut mask,
            4,
            4,
            Point::new(-50.0, -50.0),
            Point::new(-40.0, -40.0),
            1.0,
        );
        assert_eq!(mask, before);
        stroke_segment(
            &mut mask,
            8,
            8,
            Point::new(1.0, 1.0),
            Point::new(2.0, 2.0),
            1.0,
        );
        assert_eq!(
            mask, before,
            "a mask shorter than width*height is left alone"
        );
    }
}
