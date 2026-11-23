//! One frame: which regions are painted and how the part under the scrim of an open
//! dialog is produced.
//!
//! Without a scrim a region is background, tree and floating layers. Under a scrim:
//! - The frame that opens the dialog dims the pixels already in the canvas in one pass
//!   (only the regions that changed are painted first), then draws the dialog on top.
//! - Later regions are painted and dimmed directly (a spinner behind the dialog costs its
//!   own rectangle), or copied from the backdrop buffer once a full repaint filled it.
//! - The buffer stays allocated between dialogs and follows changes behind the dialog by
//!   damage rectangles; it is filled completely only for a full repaint.

use super::layers::FrameMark;
use super::render::snap_out;
use super::Ui;
use crate::canvas::Canvas;
use crate::geom::Rect;
use crate::paint::Painter;
use crate::widgets::{shadow_reach, Dialog, PaintCx, ToastView};

/// How the part under the scrim is produced in a region.
#[derive(Clone, Copy)]
enum Under<'a> {
    /// No scrim: the whole tree.
    Tree,
    /// Painted and dimmed in place.
    Dim(usize),
    /// Copied from the dimmed buffer.
    Copy(&'a Canvas, usize),
}

impl Ui {
    /// Paints what changed since the last call into `canvas` (everything on the first call
    /// or after a layout change). The canvas records the touched pixels in its damage list.
    ///
    /// The canvas must be the one painted by the previous call (the toolkit paints only
    /// what changed); with another buffer call [`Ui::paint_all`].
    pub fn paint(&mut self, canvas: &mut Canvas) {
        self.layout();
        let surface = Rect::new(0.0, 0.0, self.size().w, self.size().h);
        let full = std::mem::take(&mut self.dirty.full);
        let mut regions = std::mem::take(&mut self.dirty.rects);
        if full {
            regions = vec![surface];
        }
        let fresh = std::mem::take(&mut self.layers.backdrop.fresh);
        let end = self.layers.scrim_step;
        if regions.is_empty() && !(fresh && end.is_some()) {
            return;
        }
        let mark = FrameMark {
            width: canvas.width(),
            height: canvas.height(),
            format: canvas.format(),
            scrim: end.is_some(),
        };
        let previous = self.layers.backdrop.frame.take();
        let toasts = self.toast_views();
        let painted = match end {
            Some(end) if fresh => {
                let in_place = !full
                    && previous
                        == Some(FrameMark {
                            scrim: false,
                            ..mark
                        });
                self.open_scrim(canvas, (end, in_place), regions, &toasts)
            }
            Some(end) => self.paint_under_scrim(canvas, end, (full, regions), &toasts),
            None => self.paint_regions(canvas, Under::Tree, &regions, &toasts),
        };
        self.layers.backdrop.frame = painted.then_some(mark);
        if !painted {
            self.dirty.full = true;
        }
    }

    /// Paints the whole surface regardless of what changed.
    pub fn paint_all(&mut self, canvas: &mut Canvas) {
        // The dimmed backdrop under a dialog holds the same pixels: it is kept.
        self.mark_full_repaint();
        self.paint(canvas);
    }

    /// Paints `regions` with the part under the scrim produced as `under`. `false` when
    /// the resources were busy (nothing painted).
    fn paint_regions(
        &mut self,
        canvas: &mut Canvas,
        under: Under<'_>,
        regions: &[Rect],
        toasts: &[(usize, ToastView)],
    ) -> bool {
        let surface = Rect::new(0.0, 0.0, self.size().w, self.size().h);
        let Ok(mut resources) = self.res.try_borrow_mut() else {
            return false;
        };
        let mut p = Painter::new(canvas, &mut resources, self.env);
        let cx = self.paint_cx();
        let all = self.layers.steps.len();
        for region in regions {
            let region = snap_out(region.intersect(&surface), self.env.scale);
            if region.is_empty() {
                continue;
            }
            let saved = p.clip(region);
            let from = match under {
                Under::Tree => {
                    self.paint_under(&mut p, &cx, region, all);
                    all
                }
                Under::Dim(end) => {
                    self.paint_under(&mut p, &cx, region, end);
                    let scrim = Dialog::scrim(&p);
                    p.dim(region, scrim);
                    end
                }
                Under::Copy(buffer, end) => {
                    // The opaque panel of the dialog is painted over whatever lies there.
                    for part in outside(region, self.opaque_panel()) {
                        p.copy_from(buffer, part);
                    }
                    end
                }
            };
            self.paint_top(&mut p, &cx, region, (from, toasts));
            p.restore_clip(saved);
        }
        true
    }

    fn paint_cx(&self) -> PaintCx<'_> {
        PaintCx {
            tr: &self.tr,
            mode: self.mode,
            spin: self.spin(),
        }
    }

    /// The frame in which a dialog with a scrim appears. With `in_place` the canvas holds
    /// the previous frame: only `regions` (and the toasts, which are drawn above the scrim)
    /// are painted again without the dialog, then the whole canvas is dimmed in one pass.
    /// Otherwise everything under the scrim is painted first.
    fn open_scrim(
        &mut self,
        canvas: &mut Canvas,
        (end, in_place): (usize, bool),
        mut regions: Vec<Rect>,
        toasts: &[(usize, ToastView)],
    ) -> bool {
        let surface = Rect::new(0.0, 0.0, self.size().w, self.size().h);
        if in_place {
            let reach = shadow_reach();
            regions.extend(toasts.iter().map(|(_, view)| {
                let r = view.rect.inflate(reach.x);
                Rect::new(r.x, r.y, r.w, r.h + reach.y)
            }));
        } else {
            regions = vec![surface];
        }
        self.layers.backdrop.ready = false;
        self.layers.backdrop.damage.clear();
        let Ok(mut resources) = self.res.try_borrow_mut() else {
            return false;
        };
        let mut p = Painter::new(canvas, &mut resources, self.env);
        let cx = self.paint_cx();
        for region in regions {
            let region = snap_out(region.intersect(&surface), self.env.scale);
            if !region.is_empty() {
                let saved = p.clip(region);
                self.paint_under(&mut p, &cx, region, end);
                p.restore_clip(saved);
            }
        }
        // Snapped like every region, so that a partly covered last pixel is dimmed the same
        // way as by a later region (scales that are not whole numbers).
        let whole = snap_out(surface, self.env.scale);
        let saved = p.clip(whole);
        let scrim = Dialog::scrim(&p);
        // The opaque panel of the dialog covers its part: it is not dimmed first.
        for part in outside(whole, self.opaque_panel()) {
            p.dim(part, scrim);
        }
        self.paint_top(&mut p, &cx, whole, (end, toasts));
        p.restore_clip(saved);
        true
    }

    /// A frame while a dialog with a scrim is open. A full repaint fills the backdrop
    /// buffer (once) and copies it; other regions copy from the buffer when it is ready
    /// (bringing its damaged rectangles up to date first) or are painted and dimmed in place.
    fn paint_under_scrim(
        &mut self,
        canvas: &mut Canvas,
        end: usize,
        (full, regions): (bool, Vec<Rect>),
        toasts: &[(usize, ToastView)],
    ) -> bool {
        let size = (canvas.width(), canvas.height(), canvas.format());
        if full || self.layers.backdrop.ready {
            let mut buffer = self.layers.backdrop.buffer.take();
            let fits = buffer
                .as_ref()
                .is_some_and(|b| (b.width(), b.height(), b.format()) == size);
            if !fits {
                buffer = Canvas::new(size.0, size.1, size.2).ok();
                self.layers.backdrop.ready = false;
            }
            if let Some(mut buffer) = buffer {
                let painted = self.refresh_backdrop(&mut buffer, end)
                    && self.paint_regions(canvas, Under::Copy(&buffer, end), &regions, toasts);
                self.layers.backdrop.buffer = Some(buffer);
                return painted;
            }
        }
        self.paint_regions(canvas, Under::Dim(end), &regions, toasts)
    }

    /// Brings the backdrop buffer up to date: everything when it is not ready, else the
    /// damaged rectangles only.
    fn refresh_backdrop(&mut self, buffer: &mut Canvas, end: usize) -> bool {
        let surface = Rect::new(0.0, 0.0, self.size().w, self.size().h);
        let rects = if self.layers.backdrop.ready {
            std::mem::take(&mut self.layers.backdrop.damage)
        } else {
            self.layers.backdrop.damage.clear();
            vec![surface]
        };
        if rects.is_empty() {
            return true;
        }
        let Ok(mut resources) = self.res.try_borrow_mut() else {
            return false;
        };
        buffer.reset_clip();
        let mut p = Painter::new(buffer, &mut resources, self.env);
        let cx = self.paint_cx();
        for rect in rects {
            let rect = snap_out(rect.intersect(&surface), self.env.scale);
            if rect.is_empty() {
                continue;
            }
            let saved = p.clip(rect);
            self.paint_under(&mut p, &cx, rect, end);
            let scrim = Dialog::scrim(&p);
            p.dim(rect, scrim);
            p.restore_clip(saved);
        }
        drop(resources);
        buffer.take_damage();
        self.layers.backdrop.ready = true;
        true
    }
}

impl Ui {
    /// The part of the topmost dialog with a scrim that its opaque panel covers for sure
    /// (inside the rounded corners), if any.
    fn opaque_panel(&self) -> Option<Rect> {
        let id = self.layers.open.iter().rev().find(|d| {
            matches!(&self.nodes[d.index()].widget, crate::widgets::Widget::Dialog(w) if w.has_scrim())
        })?;
        let rect = self.rect(*id)?;
        let inset = crate::tokens::Radius::Card.points() + 1.0;
        let inner = Rect::new(
            rect.x + inset,
            rect.y + inset,
            rect.w - 2.0 * inset,
            rect.h - 2.0 * inset,
        );
        let snapped = snap_in(inner, self.env.scale);
        (!snapped.is_empty()).then_some(snapped)
    }
}

/// `region` without `hole`: up to four rectangles (above, below, beside).
fn outside(region: Rect, hole: Option<Rect>) -> Vec<Rect> {
    let Some(hole) = hole.map(|h| h.intersect(&region)).filter(|h| !h.is_empty()) else {
        return vec![region];
    };
    let parts = [
        Rect::new(region.x, region.y, region.w, hole.y - region.y),
        Rect::new(
            region.x,
            hole.bottom(),
            region.w,
            region.bottom() - hole.bottom(),
        ),
        Rect::new(region.x, hole.y, hole.x - region.x, hole.h),
        Rect::new(hole.right(), hole.y, region.right() - hole.right(), hole.h),
    ];
    parts
        .into_iter()
        .filter(|r| r.w > 0.0 && r.h > 0.0)
        .collect()
}

/// Shrinks a logical rectangle so that its edges fall on device pixels.
fn snap_in(rect: Rect, scale: f32) -> Rect {
    let x0 = (rect.x * scale).ceil() / scale;
    let y0 = (rect.y * scale).ceil() / scale;
    let x1 = (rect.right() * scale).floor() / scale;
    let y1 = (rect.bottom() * scale).floor() / scale;
    Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}
