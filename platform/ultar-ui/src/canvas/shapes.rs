//! Rounded rectangles (filled, stroked) and the shadow, as signed-distance coverage.
//!
//! Per row only the two edge zones are evaluated per pixel; the centre is one constant span.

use super::blend::{alpha_of, blend_alphas, blend_span};
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
    #[inline]
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

    /// Row centres between which the edge zones (`zone` pixels wide) see only the straight
    /// sides: there the distance depends on `x` alone (`max(qx, qy) = qx`, `qy <= 0`).
    #[inline]
    fn straight_rows(&self, zone: i32) -> (f32, f32) {
        let half = self.hy - self.r - zone as f32 - 2.0;
        if half > 0.0 {
            (self.cy - half, self.cy + half)
        } else {
            (0.0, 0.0)
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

/// Pixels from which `Canvas::blend_rect` shares the rows among the cores (below, starting
/// the threads costs more than they save).
pub(crate) const PARALLEL_AREA: i64 = 1 << 21;

/// Cores to share large blends and copies among (at most four), asked once.
pub(crate) fn cores() -> usize {
    static CORES: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    *CORES.get_or_init(|| std::thread::available_parallelism().map_or(1, |n| n.get().min(4)))
}

/// Runs `work` on every item on up to `cores` threads. The items wait in a queue that the
/// calling thread works through too: when the system refuses a thread, the calling thread
/// simply does more (never a panic in a frame).
pub(crate) fn shared<T: Send>(items: Vec<T>, cores: usize, work: &(dyn Fn(T) + Sync)) {
    use std::sync::Mutex;
    let queue = Mutex::new(items);
    let next = || {
        queue
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop()
    };
    let drain = || {
        while let Some(item) = next() {
            work(item);
        }
    };
    std::thread::scope(|scope| {
        for _ in 1..cores {
            // A refused thread leaves its share to the others.
            let _ = std::thread::Builder::new().spawn_scoped(scope, drain);
        }
        drain();
    });
}

/// Width of the edge zones for a shape whose coverage varies within `reach` pixels of its
/// left and right border (float-to-integer conversion saturates, the sum must too).
#[inline]
fn zone_width(reach: f32) -> i32 {
    (reach.ceil() as i32).saturating_add(2)
}

impl Canvas {
    /// Paints coverage `cov(x, y)` over `bbox`. `edge_zone` is the distance from the left and
    /// right border of `bbox` beyond which the coverage no longer depends on `x`; rows whose
    /// centre lies inside `alike` (open interval) have the same coverage in every row (the
    /// straight sides of the shape), so it is computed once for them – the nine-slice of a
    /// large panel or shadow.
    #[inline]
    fn paint_coverage(
        &mut self,
        (bbox, edge_zone, alike): (PixelRect, i32, (f32, f32)),
        color: Color,
        cov: impl Fn(f32, f32) -> f32,
    ) {
        let area = bbox.intersect(&self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        let px = self.pack(color);
        let left_end = bbox.x0.saturating_add(edge_zone).min(bbox.x1);
        let right_start = bbox.x1.saturating_sub(edge_zone).max(left_end);
        let zones = [
            (area.x0, left_end.min(area.x1)),
            (right_start.max(area.x0), area.x1),
        ];
        let span = (left_end.max(area.x0), right_start.min(area.x1));
        let mut alphas: Vec<u32> = Vec::new();
        let (mut span_alpha, mut cached) = (0, false);
        for y in area.y0..area.y1 {
            let fy = y as f32 + 0.5;
            let straight = fy > alike.0 && fy < alike.1;
            if !(straight && cached) {
                alphas.clear();
                for (x0, x1) in zones {
                    for x in x0..x1.max(x0) {
                        alphas.push(alpha_of(cov(x as f32 + 0.5, fy), color.a));
                    }
                }
                span_alpha = alpha_of(cov(left_end as f32 + 0.5, fy), color.a);
                cached = straight;
            }
            let mut next = alphas.as_slice();
            for (x0, x1) in zones {
                if x1 <= x0 {
                    continue;
                }
                let n = (x1 - x0) as usize;
                blend_alphas(self.row_mut(y, x0, x1), next.get(..n).unwrap_or(&[]), px);
                next = next.get(n..).unwrap_or(&[]);
            }
            if span.1 > span.0 {
                blend_span(self.row_mut(y, span.0, span.1), px, span_alpha);
            }
        }
        self.damage.add(area);
    }

    /// Blends `color` at full coverage over the pixel rectangle `area` (within the clip):
    /// the pixels `fill_rrect` of that rectangle gives, with the rows of a large area shared
    /// among up to four cores (the scrim over a 4K surface).
    pub fn blend_rect(&mut self, area: PixelRect, color: Color) {
        let area = area.intersect(&self.clip);
        if area.is_empty() || color.a == 0 {
            return;
        }
        let px = self.pack(color);
        let alpha = u32::from(color.a);
        let stride = self.width as usize * super::blend::BPP;
        let columns = area.x0 as usize * super::blend::BPP..area.x1 as usize * super::blend::BPP;
        let Some(rows) = self
            .data
            .get_mut(area.y0 as usize * stride..area.y1 as usize * stride)
        else {
            return;
        };
        let blend = |chunk: &mut [u8]| {
            for row in chunk.chunks_exact_mut(stride) {
                if let Some(span) = row.get_mut(columns.clone()) {
                    blend_span(span, px, alpha);
                }
            }
        };
        let cores = cores();
        if area.area() < PARALLEL_AREA || cores < 2 {
            blend(rows);
        } else {
            let per = (area.height() as usize).div_ceil(cores) * stride;
            shared(rows.chunks_mut(per.max(stride)).collect(), cores, &blend);
        }
        self.damage.add(area);
    }

    /// Fills a rectangle with corner radius `radius` (0 for square corners), anti-aliased.
    pub fn fill_rrect(&mut self, rect: Rect, radius: f32, color: Color) {
        if rect.is_empty() {
            return;
        }
        let shape = RoundBox::new(&rect, radius);
        let zone = zone_width(shape.r);
        let slices = (PixelRect::covering(&rect), zone, shape.straight_rows(zone));
        self.paint_coverage(slices, color, |x, y| edge(shape.dist(x, y)));
    }

    /// Strokes the inside border of a rounded rectangle with `width` pixels.
    pub fn stroke_rrect(&mut self, rect: Rect, radius: f32, width: f32, color: Color) {
        if rect.is_empty() || width <= 0.0 {
            return;
        }
        let shape = RoundBox::new(&rect, radius);
        let zone = zone_width(shape.r + width);
        let slices = (PixelRect::covering(&rect), zone, shape.straight_rows(zone));
        self.paint_coverage(slices, color, |x, y| {
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
        let zone = zone_width(shape.r + blur);
        let bbox = PixelRect::covering(&rect.inflate(blur / 2.0));
        self.paint_coverage((bbox, zone, shape.straight_rows(zone)), color, |x, y| {
            1.0 - smooth(shape.dist(x, y) / blur + 0.5)
        });
    }

    /// [`Canvas::shadow`] without the pixels inside `hole`, which the caller covers with an
    /// opaque panel afterwards: a large sheet does not blend its shadow under itself.
    /// The result after the panel is painted is the same, pixel for pixel.
    pub fn shadow_around(
        &mut self,
        rect: Rect,
        radius: f32,
        blur: f32,
        color: Color,
        hole: PixelRect,
    ) {
        let saved = self.clip;
        let b = self.bounds();
        let bands = [
            PixelRect::new(b.x0, b.y0, b.x1, hole.y0),
            PixelRect::new(b.x0, hole.y1, b.x1, b.y1),
            PixelRect::new(b.x0, hole.y0, hole.x0, hole.y1),
            PixelRect::new(hole.x1, hole.y0, b.x1, hole.y1),
        ];
        let damage_before = self.damage.clone();
        for band in bands {
            let band = band.intersect(&saved);
            if !band.is_empty() {
                self.clip = band;
                self.shadow(rect, radius, blur, color);
            }
        }
        self.clip = saved;
        // The damage is that of the whole shadow, as if it were painted in one piece.
        self.damage = damage_before;
        let bbox = PixelRect::covering(&rect.inflate(blur.max(1.0) / 2.0)).intersect(&saved);
        if !bbox.is_empty() && color.a != 0 {
            self.damage.add(bbox);
        }
    }
}
