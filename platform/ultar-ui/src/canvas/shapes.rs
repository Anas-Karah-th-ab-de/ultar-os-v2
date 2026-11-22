//! Rounded rectangles (filled, stroked) and the shadow, as signed-distance coverage.
//!
//! Per row only the two edge zones are evaluated per pixel; the centre is one constant span.

use super::blend::{alpha_of, blend_px, blend_span};
use super::Canvas;
use crate::color::Color;
use crate::geom::{PixelRect, Rect};

/// Signed distance to a rounded box (negative inside), in pixels.
#[derive(Clone, Copy)]
struct RoundBox {
    cx: f32,
    cy: f32,
    hx: f32,
    hy: f32,
    r: f32,
}

impl RoundBox {
    fn new(rect: &Rect, radius: f32) -> RoundBox {
        let (hx, hy) = (rect.w / 2.0, rect.h / 2.0);
        RoundBox {
            cx: rect.x + hx,
            cy: rect.y + hy,
            hx,
            hy,
            r: radius.max(0.0).min(hx).min(hy),
        }
    }

    #[inline]
    fn dist(&self, px: f32, py: f32) -> f32 {
        let qx = (px - self.cx).abs() - (self.hx - self.r);
        let qy = (py - self.cy).abs() - (self.hy - self.r);
        let (ox, oy) = (qx.max(0.0), qy.max(0.0));
        (ox * ox + oy * oy).sqrt() + qx.max(qy).min(0.0) - self.r
    }
}

#[inline]
fn edge(d: f32) -> f32 {
    (0.5 - d).clamp(0.0, 1.0)
}

#[inline]
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Canvas {
    /// Paints coverage `cov(x, y)` over `bbox`. `edge_zone` is the distance from the left and
    /// right border of `bbox` beyond which the coverage no longer depends on `x`.
    fn paint_coverage(
        &mut self,
        bbox: PixelRect,
        edge_zone: i32,
        color: Color,
        cov: impl Fn(f32, f32) -> f32,
    ) {
        let area = bbox.intersect(&self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        let px = self.pack(color);
        let left_end = (bbox.x0 + edge_zone).min(bbox.x1);
        let right_start = (bbox.x1 - edge_zone).max(left_end);
        let zones = [
            (area.x0, left_end.min(area.x1)),
            (right_start.max(area.x0), area.x1),
        ];
        let span = (left_end.max(area.x0), right_start.min(area.x1));
        for y in area.y0..area.y1 {
            let fy = y as f32 + 0.5;
            for (x0, x1) in zones {
                if x1 <= x0 {
                    continue;
                }
                let row = self.row_mut(y, x0, x1);
                for (i, dst) in row.chunks_exact_mut(super::blend::BPP).enumerate() {
                    let a = alpha_of(cov((x0 + i as i32) as f32 + 0.5, fy), color.a);
                    if a > 0 {
                        blend_px(dst, px, a);
                    }
                }
            }
            if span.1 > span.0 {
                let a = alpha_of(cov(left_end as f32 + 0.5, fy), color.a);
                blend_span(self.row_mut(y, span.0, span.1), px, a);
            }
        }
        self.damage.add(area);
    }

    /// Fills a rectangle with corner radius `radius` (0 for square corners), anti-aliased.
    pub fn fill_rrect(&mut self, rect: Rect, radius: f32, color: Color) {
        if rect.is_empty() {
            return;
        }
        let shape = RoundBox::new(&rect, radius);
        let zone = shape.r.ceil() as i32 + 2;
        self.paint_coverage(PixelRect::covering(&rect), zone, color, |x, y| {
            edge(shape.dist(x, y))
        });
    }

    /// Strokes the inside border of a rounded rectangle with `width` pixels.
    pub fn stroke_rrect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        if rect.is_empty() || width <= 0.0 {
            return;
        }
        let shape = RoundBox::new(&rect, radius);
        let zone = (shape.r + width).ceil() as i32 + 2;
        self.paint_coverage(PixelRect::covering(&rect), zone, color, |x, y| {
            let d = shape.dist(x, y);
            edge(d) - edge(d + width)
        });
    }

    /// Paints a soft shadow of a rounded rectangle: full `color` inside, fading to nothing
    /// over `blur` pixels centred on the outline. Offset the rectangle before calling.
    pub fn shadow(&mut self, rect: Rect, radius: f32, blur: f32, color: Color) {
        if rect.is_empty() {
            return;
        }
        let blur = blur.max(1.0);
        let shape = RoundBox::new(&rect, radius);
        let zone = (shape.r + blur).ceil() as i32 + 2;
        let bbox = PixelRect::covering(&rect.inflate(blur / 2.0));
        self.paint_coverage(bbox, zone, color, |x, y| {
            1.0 - smooth(shape.dist(x, y) / blur + 0.5)
        });
    }
}
