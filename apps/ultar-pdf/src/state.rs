//! The pure logic of the reader: zoom, page geometry, rotation, search and selection.
//! Everything here is deterministic and tested without a screen.

use ultar_i18n::arabic::search_matches;
use ultar_office::pdf::Word;

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
    let step = if dir >= 0 { 1isize } else { -1isize };
    match ZOOM_STEPS.binary_search(&p) {
        Ok(i) => {
            let j = (i as isize + step).clamp(0, ZOOM_STEPS.len() as isize - 1) as usize;
            ZOOM_STEPS[j]
        }
        Err(insert) => {
            let j = if step > 0 { insert } else { insert.saturating_sub(1) };
            ZOOM_STEPS[j.min(ZOOM_STEPS.len() - 1)]
        }
    }
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
    /// The next rotation.
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

/// Rotates an RGBA buffer by `turns` quarter turns clockwise.
pub fn rotate_rgba(w: u32, h: u32, rgba: &[u8], turns: u8) -> (u32, u32, Vec<u8>) {
    let turns = turns % 4;
    if turns == 0 {
        return (w, h, rgba.to_vec());
    }
    let (nw, nh) = match turns {
        1 | 3 => (h, w),
        _ => (w, h),
    };
    let mut out = vec![0u8; (nw as usize * nh as usize * 4) as usize];
    let src = |x: u32, y: u32| ((y * w + x) * 4) as usize;
    for y2 in 0..nh {
        for x2 in 0..nw {
            let (x, y) = match turns {
                // 90° clockwise: new(x,y) = old(y, w-1-x).
                1 => (y2, w - 1 - x2),
                // 180°.
                2 => (w - 1 - x2, h - 1 - y2),
                // 270° clockwise.
                _ => (h - 1 - y2, x2),
            };
            let s = src(x, y);
            let d = ((y2 * nw + x2) * 4) as usize;
            out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    (nw, nh, out)
}

/// One page laid out in the continuous scroll (top offset and height in device pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageRow {
    pub page: usize,
    /// Top offset in the content, device pixels.
    pub y: f32,
    /// Height in device pixels.
    pub h: f32,
}

/// Lays the pages out top-to-bottom; returns the rows and the total content height.
pub fn layout_pages(sizes: &[(f32, f32)], scale: f32, rot: Rot) -> (Vec<PageRow>, f32) {
    let gap = PAGE_GAP * scale;
    let mut rows = Vec::with_capacity(sizes.len());
    let mut y = 0.0f32;
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let (_, dh) = page_display(w, h, rot);
        let dh = dh * scale;
        rows.push(PageRow {
            page: i,
            y,
            h: dh,
        });
        y += dh + gap;
    }
    let total = if sizes.is_empty() {
        0.0
    } else {
        y - gap
    };
    (rows, total)
}

/// The pages (indices) that intersect the viewport `[scroll, scroll + view_h)`.
pub fn visible_pages(rows: &[PageRow], scroll: f32, view_h: f32) -> std::ops::Range<usize> {
    let mut first = rows.len();
    let mut last = 0usize;
    for row in rows {
        if row.y + row.h > scroll && row.y < scroll + view_h {
            first = first.min(row.page);
            last = last.max(row.page);
        }
    }
    if first > last {
        return 0..0;
    }
    first..last + 1
}

/// The page under the middle of the viewport (for the status line).
pub fn page_at(rows: &[PageRow], scroll: f32, view_h: f32) -> Option<usize> {
    let mid = scroll + view_h / 2.0;
    rows.iter()
        .find(|r| r.y <= mid && mid < r.y + r.h)
        .map(|r| r.page)
        .or_else(|| rows.last().map(|r| r.page))
}

/// The scroll offset that puts `page` at the top of the viewport.
pub fn scroll_to_page(rows: &[PageRow], page: usize) -> f32 {
    rows.get(page).map(|r| r.y).unwrap_or(0.0)
}

/// One search hit: a run of matching words on a page (inclusive indices).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hit {
    pub page: usize,
    pub first: usize,
    pub last: usize,
}

/// Finds every word matching `query` (Arabic normalisation, the same on both sides).
/// Empty and blank queries give no hits. Consecutive matches become one hit.
pub fn find_hits(pages: &[Vec<Word>], query: &str) -> Vec<Hit> {
    let q = query.trim();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (page, words) in pages.iter().enumerate() {
        let mut i = 0;
        while i < words.len() {
            if search_matches(&words[i].text, q) {
                let first = i;
                let mut last = i;
                while last + 1 < words.len() && search_matches(&words[last + 1].text, q) {
                    last += 1;
                }
                out.push(Hit { page, first, last });
                i = last + 1;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// The indices of words whose center lies inside `rect` (points, y up from the bottom).
pub fn words_in_rect(words: &[Word], rect: (f32, f32, f32, f32)) -> Vec<usize> {
    let (rx0, ry0, rx1, ry1) = rect;
    let (x0, y0, x1, y1) = (rx0.min(rx1), ry0.min(ry1), rx0.max(rx1), ry0.max(ry1));
    words
        .iter()
        .enumerate()
        .filter(|(_, w)| {
            let cx = (w.x0 + w.x1) / 2.0;
            let cy = (w.y0 + w.y1) / 2.0;
            cx >= x0 && cx <= x1 && cy >= y0 && cy <= y1
        })
        .map(|(i, _)| i)
        .collect()
}

/// The words of a selection, joined in reading order.
pub fn selection_text(words: &[Word], mut indices: Vec<usize>) -> String {
    indices.sort_unstable();
    indices.dedup();
    indices
        .into_iter()
        .filter_map(|i| words.get(i).map(|w| w.text.clone()))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sizes() -> Vec<(f32, f32)> {
        vec![(595.0, 842.0), (595.0, 842.0), (300.0, 400.0)]
    }

    #[test]
    fn zoom_steps_stay_inside_the_range() {
        assert_eq!(zoom_step(100, 1), 125);
        assert_eq!(zoom_step(100, -1), 80);
        assert_eq!(zoom_step(400, 1), 400);
        assert_eq!(zoom_step(50, -1), 50);
        assert_eq!(zoom_step(90, 1), 100);
        assert_eq!(clamp_zoom(800), 400);
        assert_eq!(clamp_zoom(0), 50);
    }

    #[test]
    fn fit_keeps_the_page_fully_visible() {
        // 595 pt page in an 800 pt view → width fit at ~1.27.
        let sw = fit_width_scale(800.0, 595.0);
        assert!((595.0 * sw) <= 800.0 - 2.0 * PAGE_MARGIN);
        let sp = fit_page_scale(800.0, 500.0, 595.0, 842.0);
        assert!(842.0 * sp <= 500.0 - 2.0 * PAGE_MARGIN);
        assert!(sp < sw);
    }

    #[test]
    fn layout_and_visibility_follow_each_other() {
        let (rows, total) = layout_pages(&sizes(), 1.0, Rot::None);
        assert_eq!(rows.len(), 3);
        // 842 + 842 + 400 + two gaps.
        assert!((total - (842.0 + 842.0 + 400.0 + 2.0 * PAGE_GAP)).abs() < 0.01);
        let visible = visible_pages(&rows, 0.0, 900.0);
        assert_eq!(visible, 0..2);
        let visible = visible_pages(&rows, 900.0, 900.0);
        assert_eq!(visible, 1..3);
        assert_eq!(page_at(&rows, 0.0, 900.0), Some(0));
        assert_eq!(page_at(&rows, 860.0, 900.0), Some(1));
    }

    #[test]
    fn rotation_swaps_the_page_size_and_rotates_pixels() {
        assert_eq!(page_display(595.0, 842.0, Rot::Cw), (842.0, 595.0));
        assert_eq!(page_display(595.0, 842.0, Rot::Half), (595.0, 842.0));
        // 2×3 RGBA, one quarter turn clockwise → 3×2.
        let rgba: Vec<u8> = vec![
            1, 0, 0, 255, 2, 0, 0, 255, 3, 0, 0, 255, 4, 0, 0, 255, 5, 0, 0, 255, 6, 0, 0, 255,
        ];
        let (w, h, out) = rotate_rgba(3, 2, &rgba, 1);
        assert_eq!((w, h), (2, 3));
        // The top-left of the rotated image is the old bottom-left (value 4).
        assert_eq!(out[0], 4);
    }

    fn words() -> Vec<Word> {
        fn w(text: &str, x: f32, y: f32) -> Word {
            Word {
                text: text.into(),
                x0: x,
                y0: y,
                x1: x + 20.0,
                y1: y + 12.0,
            }
        }
        vec![
            w("مرحبا", 50.0, 700.0),
            w("بكم", 110.0, 700.0),
            w("hello", 50.0, 680.0),
        ]
    }

    #[test]
    fn search_finds_with_arabic_normalisation() {
        // Presentation forms and tashkeel fold to the same base.
        let mut pages = vec![words()];
        pages[0][0].text = "\u{FEE3}\u{FEAE}\u{FEA3}\u{FE92}\u{FE8E}".into();
        let hits = find_hits(&pages, "مرحبا");
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0], Hit { page: 0, first: 0, last: 0 });
        // Latin is case-insensitive through the same normalisation.
        let hits = find_hits(&pages, "HELLO");
        assert_eq!(hits.len(), 1);
        assert_eq!(find_hits(&pages, "").len(), 0);
        assert_eq!(find_hits(&pages, "   ").len(), 0);
    }

    #[test]
    fn selection_collects_words_in_reading_order() {
        let ws = words();
        let idx = words_in_rect(&ws, (0.0, 675.0, 150.0, 720.0));
        assert_eq!(idx, vec![0, 1, 2]);
        assert_eq!(selection_text(&ws, idx), "مرحبا بكم hello");
        assert_eq!(selection_text(&ws, vec![2, 0]), "مرحبا hello");
    }
}