//! The continuous page layout: where each page sits in the scrolling content (device pixels)
//! and what lies under a point of the view. Pure geometry, tested without a screen.

use crate::state::{page_display, Rot, PAGE_GAP, PAGE_MARGIN};

/// One page laid out in the continuous scroll.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRow {
    pub page: usize,
    /// Top offset in the content, device pixels.
    pub y: f32,
    /// Width in device pixels.
    pub w: f32,
    /// Height in device pixels.
    pub h: f32,
}

/// Where a point of the view falls on a page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Located {
    pub page: usize,
    /// Position inside the shown page picture, device pixels.
    pub x: f32,
    pub y: f32,
}

/// All pages top to bottom at one scale.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Layout {
    pub rows: Vec<PageRow>,
    /// Content height in device pixels.
    pub total_h: f32,
    /// Content width: the widest page plus the margins, device pixels.
    pub content_w: f32,
}

impl Layout {
    /// Lays out pages of `sizes` (points, unrotated) at `scale` turned by `rot`.
    pub fn new(sizes: &[(f32, f32)], scale: f32, rot: Rot) -> Layout {
        let gap = PAGE_GAP * scale;
        let mut rows = Vec::with_capacity(sizes.len());
        let mut y = 0.0f32;
        let mut widest = 0.0f32;
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let (dw, dh) = page_display(w, h, rot);
            let (dw, dh) = (dw * scale, dh * scale);
            widest = widest.max(dw);
            rows.push(PageRow { page: i, y, w: dw, h: dh });
            y += dh + gap;
        }
        let total_h = if rows.is_empty() { 0.0 } else { y - gap };
        Layout {
            rows,
            total_h,
            content_w: widest + 2.0 * PAGE_MARGIN,
        }
    }

    /// The panel's thumbnails top to bottom: every page `width` wide, as high as its shape
    /// asks plus `label` for its number, `gap` apart (all device pixels).
    pub fn thumbs(sizes: &[(f32, f32)], width: f32, gap: f32, label: f32) -> Layout {
        let mut rows = Vec::with_capacity(sizes.len());
        let mut y = 0.0f32;
        for (i, &(w, h)) in sizes.iter().enumerate() {
            let tall = (width * h / w.max(1.0)).clamp(width * 0.25, width * 4.0) + label;
            rows.push(PageRow {
                page: i,
                y,
                w: width,
                h: tall,
            });
            y += tall + gap;
        }
        Layout {
            total_h: if rows.is_empty() { 0.0 } else { y - gap },
            content_w: width,
            rows,
        }
    }

    /// The largest vertical scroll for a view of height `view_h`.
    pub fn max_scroll(&self, view_h: f32) -> f32 {
        (self.total_h - view_h).max(0.0)
    }

    /// The largest horizontal scroll for a view of width `view_w`.
    pub fn max_scroll_x(&self, view_w: f32) -> f32 {
        (self.content_w - view_w).max(0.0)
    }

    /// The left edge of `page` in the view (device pixels): centred while the content fits,
    /// shifted by the horizontal scroll when it is wider than the view.
    pub fn origin_x(&self, page: usize, view_w: f32, scroll_x: f32) -> f32 {
        let w = self.rows.get(page).map_or(0.0, |r| r.w);
        (self.content_w.max(view_w) - w) / 2.0 - scroll_x
    }

    /// The pages (indices) that intersect the viewport `[scroll, scroll + view_h)`.
    pub fn visible(&self, scroll: f32, view_h: f32) -> std::ops::Range<usize> {
        let first = self.rows.partition_point(|r| r.y + r.h <= scroll);
        let last = self.rows.partition_point(|r| r.y < scroll + view_h);
        if first >= last {
            return 0..0;
        }
        first..last
    }

    /// The page under the middle of the viewport (the page the status shows).
    pub fn page_at(&self, scroll: f32, view_h: f32) -> Option<usize> {
        let mid = scroll + view_h / 2.0;
        let i = self.rows.partition_point(|r| r.y + r.h <= mid);
        self.rows
            .get(i)
            .map(|r| r.page)
            .or_else(|| self.rows.last().map(|r| r.page))
    }

    /// The scroll offset that puts `page` at the top of the viewport.
    pub fn scroll_to(&self, page: usize) -> f32 {
        self.rows.get(page).map_or(0.0, |r| r.y)
    }

    /// The page and the fraction (0..1) of its height at the top of the viewport; keeps the
    /// reading position while the zoom changes.
    pub fn anchor(&self, scroll: f32) -> (usize, f32) {
        let i = self.rows.partition_point(|r| r.y + r.h <= scroll);
        match self.rows.get(i) {
            Some(r) => (r.page, ((scroll - r.y) / r.h.max(1.0)).clamp(0.0, 1.0)),
            None => (self.rows.len().saturating_sub(1), 0.0),
        }
    }

    /// The scroll offset of an [`Layout::anchor`] in this layout.
    pub fn scroll_of(&self, anchor: (usize, f32)) -> f32 {
        self.rows
            .get(anchor.0)
            .map_or(0.0, |r| r.y + r.h * anchor.1)
    }

    /// The page under the view point `(x, y)` (device pixels from the view's top start corner).
    pub fn locate(&self, x: f32, y: f32, view_w: f32, scroll: f32, scroll_x: f32) -> Option<Located> {
        let cy = scroll + y;
        let i = self.rows.partition_point(|r| r.y + r.h <= cy);
        let r = self.rows.get(i).filter(|r| r.y <= cy)?;
        let left = self.origin_x(r.page, view_w, scroll_x);
        let (px, py) = (x - left, cy - r.y);
        (px >= 0.0 && px < r.w).then_some(Located {
            page: r.page,
            x: px,
            y: py,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> Vec<(f32, f32)> {
        vec![(595.0, 842.0), (595.0, 842.0), (300.0, 400.0)]
    }

    #[test]
    fn layout_and_visibility_follow_each_other() {
        let l = Layout::new(&sizes(), 1.0, Rot::None);
        assert_eq!(l.rows.len(), 3);
        assert!((l.total_h - (842.0 + 842.0 + 400.0 + 2.0 * PAGE_GAP)).abs() < 0.01);
        assert_eq!(l.visible(0.0, 900.0), 0..2);
        assert_eq!(l.visible(900.0, 900.0), 1..3);
        assert_eq!(l.visible(0.0, 10.0), 0..1);
        assert_eq!(l.page_at(0.0, 900.0), Some(0));
        assert_eq!(l.page_at(860.0, 900.0), Some(1));
        assert_eq!(l.page_at(5000.0, 900.0), Some(2));
        assert_eq!(Layout::default().page_at(0.0, 10.0), None);
        // A gap between two pages shows no page at its top edge.
        assert_eq!(l.visible(845.0, 5.0), 0..0);
    }

    #[test]
    fn thumbnails_keep_their_shape_and_leave_room_for_the_number() {
        let t = Layout::thumbs(&sizes(), 100.0, 10.0, 20.0);
        assert_eq!(t.rows.len(), 3);
        assert!((t.rows[0].h - (100.0 * 842.0 / 595.0 + 20.0)).abs() < 0.01);
        assert!((t.rows[2].h - (100.0 * 400.0 / 300.0 + 20.0)).abs() < 0.01);
        assert!((t.rows[1].y - (t.rows[0].h + 10.0)).abs() < 0.01);
        assert_eq!(t.visible(0.0, 10.0), 0..1);
        // a very long page does not take over the whole panel
        let long = Layout::thumbs(&[(10.0, 5000.0)], 100.0, 10.0, 20.0);
        assert_eq!(long.rows[0].h, 420.0);
    }

    #[test]
    fn rotation_turns_the_rows() {
        let l = Layout::new(&sizes(), 1.0, Rot::Cw);
        assert_eq!(l.rows[0].w, 842.0);
        assert_eq!(l.rows[0].h, 595.0);
        assert!((l.content_w - (842.0 + 2.0 * PAGE_MARGIN)).abs() < 0.01);
    }

    #[test]
    fn the_anchor_keeps_the_reading_position_over_a_zoom() {
        let a = Layout::new(&sizes(), 1.0, Rot::None);
        let anchor = a.anchor(a.rows[1].y + 421.0);
        assert_eq!(anchor.0, 1);
        assert!((anchor.1 - 0.5).abs() < 0.01);
        let b = Layout::new(&sizes(), 2.0, Rot::None);
        assert!((b.scroll_of(anchor) - (b.rows[1].y + 842.0)).abs() < 1.0);
        assert_eq!(a.scroll_to(2), a.rows[2].y);
    }

    #[test]
    fn pages_centre_and_scroll_sideways_when_wider_than_the_view() {
        let l = Layout::new(&sizes(), 1.0, Rot::None);
        // 595 wide page in an 800 wide view: centred.
        assert!((l.origin_x(0, 800.0, 0.0) - 102.5).abs() < 0.01);
        // The small page is centred in the content width.
        assert!((l.origin_x(2, 800.0, 0.0) - 250.0).abs() < 0.01);
        // Zoomed in: the content is wider than the view and moves with the scroll.
        let big = Layout::new(&sizes(), 2.0, Rot::None);
        assert!(big.max_scroll_x(800.0) > 0.0);
        let left = big.origin_x(0, 800.0, 0.0);
        assert!((left - PAGE_MARGIN).abs() < 0.01);
        assert!((big.origin_x(0, 800.0, 100.0) - (left - 100.0)).abs() < 0.01);
    }

    #[test]
    fn a_view_point_finds_its_page_and_the_spot_on_it() {
        let l = Layout::new(&sizes(), 1.0, Rot::None);
        let hit = l.locate(150.0, 20.0, 800.0, 0.0, 0.0);
        assert_eq!(hit.map(|h| h.page), Some(0));
        let h = hit.unwrap_or(Located { page: 9, x: 0.0, y: 0.0 });
        assert!((h.x - 47.5).abs() < 0.01 && (h.y - 20.0).abs() < 0.01);
        // Left of the page, and in the gap: nothing.
        assert_eq!(l.locate(10.0, 20.0, 800.0, 0.0, 0.0), None);
        assert_eq!(l.locate(150.0, 850.0, 800.0, 0.0, 0.0), None);
        // Scrolled to the second page.
        let s = l.rows[1].y + 5.0;
        assert_eq!(l.locate(150.0, 0.0, 800.0, s, 0.0).map(|h| h.page), Some(1));
    }
}
