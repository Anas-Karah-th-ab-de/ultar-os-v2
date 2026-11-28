//! Paints the page area (the visible pages with search hits and the selection) and the
//! thumbnails of the panel into their pictures. The pages are white whatever the theme; the
//! ground around them follows it.

use crate::app::App;
use crate::build::PANEL_WIDTH;
use crate::doc::DocState;
use crate::raster::{Buf, Rgba};
use crate::state::rect_to_px;
use ultar_ui::canvas::Canvas;
use ultar_ui::color::Color;
use ultar_ui::tokens::ColorRole;
use ultar_ui::ui::UiError;

const PAGE_WHITE: Rgba = Rgba(255, 255, 255, 255);

fn rgba(c: Color, a: u8) -> Rgba {
    Rgba(c.r, c.g, c.b, a)
}

impl App {
    /// Paints the page area and the thumbnails.
    pub fn draw(&mut self) -> Result<(), UiError> {
        self.draw_pages()?;
        self.draw_thumbs()
    }

    fn draw_pages(&mut self) -> Result<(), UiError> {
        let Some(node) = self.nodes.page else {
            return Ok(());
        };
        let (vw, vh) = self.view_px();
        if vw < 1.0 || vh < 1.0 {
            return Ok(());
        }
        let palette = self.ui.env().palette();
        let mut buf = Buf::new(vw.round() as u32, vh.round() as u32, rgba(palette.get(ColorRole::Ground), 255));
        if self.doc.state == DocState::Open {
            for page in self.layout.visible(self.scroll, vh) {
                self.paint_page(&mut buf, page);
            }
        }
        let canvas = Canvas::from_bgrx(buf.w as u32, buf.h as u32, buf.data)
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }

    fn paint_page(&mut self, buf: &mut Buf, page: usize) {
        let Some(row) = self.layout.rows.get(page).copied() else {
            return;
        };
        let ox = self.layout.origin_x(page, buf.w as f32, self.scroll_x).round() as i32;
        let oy = (row.y - self.scroll).round() as i32;
        let (w, h) = (row.w.round() as i32, row.h.round() as i32);
        buf.blend(ox, oy, ox + w, oy + h, PAGE_WHITE);
        let img = self.doc.image(page, self.scale).or_else(|| self.doc.stale_image(page));
        if let Some(img) = img {
            buf.picture(ox, oy, w, h, &img, self.rot);
        } else if self.doc.page_failed(page) {
            let muted = self.ui.env().palette().get(ColorRole::TextMuted);
            buf.cross(ox, oy, w, h, rgba(muted, 120));
        }
        self.paint_marks(buf, page, (ox, oy));
    }

    /// The search hits and the selection over one page.
    fn paint_marks(&self, buf: &mut Buf, page: usize, (ox, oy): (i32, i32)) {
        let Some(size) = self.doc.sizes.get(page).copied() else {
            return;
        };
        let accent = self.ui.env().palette().get(ColorRole::Accent);
        let to_px = |r| {
            let (l, t, rr, b) = rect_to_px(r, size, self.scale, self.rot);
            (
                ox + l.floor() as i32,
                oy + t.floor() as i32,
                ox + rr.ceil() as i32,
                oy + b.ceil() as i32,
            )
        };
        let hits = self.search.hits();
        let (from, to) = (
            hits.partition_point(|h| h.page < page),
            hits.partition_point(|h| h.page <= page),
        );
        let current = self.search.position();
        for (i, hit) in hits.iter().enumerate().take(to).skip(from) {
            let alpha = if Some(i) == current { 200 } else { 90 };
            for r in &hit.rects {
                let (l, t, rr, b) = to_px(*r);
                buf.blend(l, t, rr, b, rgba(accent, alpha));
            }
        }
        let Some(sel) = self.selection.filter(|s| s.page == page) else {
            return;
        };
        let Some(text) = self.doc.text(page) else {
            return;
        };
        let last = sel.last.min(text.words.len().saturating_sub(1));
        for w in text.words.get(sel.first..=last).unwrap_or(&[]) {
            let (l, t, rr, b) = to_px((w.x0, w.y0, w.x1, w.y1));
            buf.blend(l, t, rr, b, rgba(accent, 110));
        }
    }

    // ---- the panel's thumbnails ---------------------------------------------------------

    /// The size of the thumbnail area in device pixels.
    fn thumbs_px(&mut self) -> Option<(f32, f32)> {
        self.ui.layout();
        let s = self.ui.env().scale;
        let r = self.nodes.thumbs.and_then(|n| self.ui.rect(n))?;
        Some((r.w * s, r.h * s))
    }

    /// Lays the thumbnails out for the panel's width (after a resize or a new document).
    pub(crate) fn build_thumbs(&mut self) {
        let s = self.ui.env().scale;
        let width = (PANEL_WIDTH - 24.0) * s;
        self.thumbs = crate::layout::Layout::thumbs(&self.doc.sizes, width, 12.0 * s, 18.0 * s);
        let height = self.thumbs_px().map_or(0.0, |p| p.1);
        self.panel_scroll = self.panel_scroll.clamp(0.0, self.thumbs.max_scroll(height));
    }

    /// Scrolls the thumbnails so that the current page's one is in view.
    pub(crate) fn follow_thumbs(&mut self) {
        let Some((_, h)) = self.thumbs_px() else {
            return;
        };
        let Some(row) = self.thumbs.rows.get(self.page).copied() else {
            return;
        };
        if row.y < self.panel_scroll {
            self.panel_scroll = row.y;
        } else if row.y + row.h > self.panel_scroll + h {
            self.panel_scroll = row.y + row.h - h;
        }
        self.panel_scroll = self.panel_scroll.clamp(0.0, self.thumbs.max_scroll(h));
    }

    /// The thumbnails to ask for: the visible ones and two on each side.
    pub(crate) fn wanted_thumbs(&mut self) -> Vec<(usize, f32)> {
        let Some((_, h)) = self.thumbs_px() else {
            return Vec::new();
        };
        let vis = self.thumbs.visible(self.panel_scroll, h);
        let (from, to) = (vis.start.saturating_sub(2), (vis.end + 2).min(self.thumbs.rows.len()));
        (from..to)
            .filter_map(|p| {
                let w = self.doc.sizes.get(p)?.0;
                Some((p, self.thumbs.rows.get(p)?.w / w.max(1.0)))
            })
            .collect()
    }

    fn draw_thumbs(&mut self) -> Result<(), UiError> {
        let (Some(node), Some((w, h))) = (self.nodes.thumbs, self.thumbs_px()) else {
            return Ok(());
        };
        if w < 1.0 || h < 1.0 {
            return Ok(());
        }
        let palette = self.ui.env().palette();
        let (text, border, accent) = (
            palette.get(ColorRole::TextMuted),
            palette.get(ColorRole::ControlBorder),
            palette.get(ColorRole::Accent),
        );
        let mut buf = Buf::new(w.round() as u32, h.round() as u32, rgba(palette.get(ColorRole::Surface), 255));
        let s = self.ui.env().scale;
        let label = (18.0 * s) as i32;
        for page in self.thumbs.visible(self.panel_scroll, h) {
            let Some(row) = self.thumbs.rows.get(page).copied() else {
                continue;
            };
            let x = ((w - row.w) / 2.0).round() as i32;
            let y = (row.y - self.panel_scroll).round() as i32;
            let (tw, th) = (row.w.round() as i32, row.h.round() as i32 - label);
            buf.blend(x, y, x + tw, y + th, PAGE_WHITE);
            if let Some(img) = self.doc.thumb(page) {
                buf.picture(x, y, tw, th, &img, crate::state::Rot::None);
            }
            let (c, t) = if page == self.page { (accent, 3) } else { (border, 1) };
            buf.frame(x, y, x + tw, y + th, t * (s.round() as i32).max(1), rgba(c, 255));
            let k = (2.0 * s).round().max(1.0) as i32;
            buf.number(page + 1, x + tw / 2, y + th + (label - 5 * k) / 2, k, rgba(text, 255));
        }
        let canvas = Canvas::from_bgrx(buf.w as u32, buf.h as u32, buf.data)
            .map_err(|_| UiError::InvalidSize)?;
        self.ui.set_picture(node, canvas)
    }
}
