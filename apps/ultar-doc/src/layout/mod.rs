//! The layout of a document: every paragraph shaped and broken into lines ([`para`]), the
//! lines placed on pages (A4/Letter with margins) or in one flowing column (phone), tables as
//! grids and pictures as boxes. The layout answers where the caret is, which character lies
//! under a point and which rectangles a selection covers ([`query`]). Paragraph layouts are
//! cached by content, so typing re-shapes one paragraph and only walks the rest.

pub mod para;
pub mod query;

use crate::state::Cell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use ultar_office::doc::{Block, Document, Para, Picture, Style};
use ultar_ui::text::{TextEngine, TextLayout};

/// How a piece of text looks (sizes in pixels).
#[derive(Clone, Debug, PartialEq)]
pub struct PieceStyle {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub color: Option<[u8; 3]>,
    pub highlight: Option<[u8; 3]>,
    pub mono: bool,
    pub title: bool,
}

/// A shaped piece of a line: a word, a run of spaces or a tab, in visual order.
#[derive(Clone)]
pub struct Piece {
    /// Characters of the paragraph this piece stands for (`start == end` for a list marker).
    pub start: usize,
    pub end: usize,
    /// Left edge from the left edge of the column, pixels.
    pub x: f32,
    pub w: f32,
    pub rtl: bool,
    pub space: bool,
    pub style: PieceStyle,
    pub text: String,
    pub layout: Option<Rc<TextLayout>>,
}

/// One line of a paragraph (positions relative to the paragraph).
#[derive(Clone)]
pub struct LineBox {
    /// Characters `start..end` of the item's text (trailing spaces included).
    pub start: usize,
    pub end: usize,
    pub y: f32,
    pub h: f32,
    /// Distance from the top to the baseline (before the line spacing).
    pub base: f32,
    pub pieces: Vec<Piece>,
    pub marker: Option<Piece>,
    pub rtl: bool,
    /// The line ends with a line feed.
    pub hard_end: bool,
    /// Where the line starts (its start edge) from the left edge of the column: where the
    /// caret of an empty line stands.
    pub start_x: f32,
}

/// A laid out paragraph.
pub struct ParaLayout {
    pub lines: Vec<LineBox>,
    pub h: f32,
    pub before: f32,
    pub after: f32,
    pub style: Style,
}

/// One line on its page.
pub struct Placed {
    pub para: Rc<ParaLayout>,
    pub line: usize,
    pub block: usize,
    pub cell: Option<Cell>,
    /// Left edge of the column from the left edge of the paper.
    pub x: f32,
    /// Top of the line from the top of the whole document view.
    pub y: f32,
    pub page: usize,
}

impl Placed {
    pub fn line(&self) -> &LineBox {
        &self.para.lines[self.line]
    }
}

/// The grid of a table: per row its top, height and page; per column its left edge.
pub struct TableBox {
    pub block: usize,
    pub x: f32,
    pub col_x: Vec<f32>,
    pub col_w: f32,
    pub rows: Vec<(f32, f32, usize)>,
    pub rtl: bool,
}

/// A picture on the page.
pub struct PicBox {
    pub block: usize,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub page: usize,
}

/// A sheet of paper.
pub struct PageBox {
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Settings of one layout pass.
#[derive(Clone, Copy, PartialEq)]
pub struct Geo {
    /// A paged view (otherwise one flowing column as wide as the view).
    pub page_view: bool,
    /// Width of the view in pixels.
    pub view_w: f32,
    /// Pixels per point at zoom 1 (the display scale).
    pub scale: f32,
    pub zoom: f32,
}

impl Geo {
    /// Pixels per point.
    pub fn k(&self) -> f32 {
        self.scale * self.zoom
    }
}

/// Where each line, table and picture of a document is.
pub struct Layout {
    pub placed: Vec<Placed>,
    pub tables: Vec<TableBox>,
    pub pics: Vec<PicBox>,
    pub pages: Vec<PageBox>,
    /// Width and height of the whole document view in pixels.
    pub width: f32,
    pub height: f32,
    /// Paper width in pixels (the view width in the flowing view).
    pub paper_w: f32,
    /// Width of the text column in pixels.
    pub col_w: f32,
    pub k: f32,
    pub geo: Geo,
    pub(crate) items: HashMap<(usize, Option<Cell>), (usize, usize)>,
}

/// Cache of laid out paragraphs.
#[derive(Default)]
pub struct ParaCache {
    map: HashMap<u64, (u64, Rc<ParaLayout>)>,
    gen: u64,
    used: usize,
}

fn key_of(p: &Para, label: Option<&str>, width: f32, k: f32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (
        p.style as u8,
        p.align as u8,
        p.dir as u8,
        p.indent_pt.map(f32::to_bits),
        p.line_spacing.map(f32::to_bits),
    )
        .hash(&mut h);
    label.hash(&mut h);
    (width.to_bits(), k.to_bits()).hash(&mut h);
    for r in &p.runs {
        (
            &r.text,
            r.bold,
            r.italic,
            r.underline,
            r.strike,
            r.size_pt.map(f32::to_bits),
        )
            .hash(&mut h);
        (&r.color, &r.highlight).hash(&mut h);
    }
    h.finish()
}

impl ParaCache {
    fn begin(&mut self) {
        self.gen += 1;
        self.used = 0;
    }

    fn get(
        &mut self,
        eng: &mut TextEngine,
        p: &Para,
        label: Option<&str>,
        width: f32,
        k: f32,
    ) -> Rc<ParaLayout> {
        let key = key_of(p, label, width, k);
        self.used += 1;
        if let Some(e) = self.map.get_mut(&key) {
            e.0 = self.gen;
            return Rc::clone(&e.1);
        }
        let l = Rc::new(para::layout_para(eng, p, label, width, k));
        self.map.insert(key, (self.gen, Rc::clone(&l)));
        l
    }

    fn end(&mut self) {
        if self.map.len() > self.used * 2 + 64 {
            let gen = self.gen;
            self.map.retain(|_, e| e.0 == gen);
        }
    }
}

/// Space around the paper in the paged view, pixels at scale 1.
const GAP: f32 = 16.0;
/// Padding inside a table cell, points.
const CELL_PAD_PT: f32 = 5.0;
/// Margin of the flowing view, points.
const FLOW_MARGIN_PT: f32 = 10.0;

struct Pager {
    page: usize,
    y: f32,
    body_h: f32,
    origin: f32,
    top: f32,
    step: f32,
}

impl Pager {
    fn new(geo: &Geo, paper_h: f32, top: f32, bottom: f32) -> Pager {
        let gap = GAP * geo.scale;
        let (origin, step, body_h) = if geo.page_view {
            (gap, paper_h + gap, paper_h - top - bottom)
        } else {
            (0.0, 0.0, f32::INFINITY)
        };
        Pager {
            page: 0,
            y: 0.0,
            body_h,
            origin,
            top,
            step,
        }
    }

    /// Starts a new page unless this one is empty.
    fn next_page(&mut self) {
        if self.y > 0.0 && self.step > 0.0 {
            self.page += 1;
            self.y = 0.0;
        }
    }

    /// Makes room for `h`; returns whether a page was started.
    fn room(&mut self, h: f32) {
        if self.y > 0.0 && self.y + h > self.body_h {
            self.next_page();
        }
    }

    /// Absolute top of the current line position.
    fn abs(&self) -> f32 {
        self.origin + self.page as f32 * self.step + self.top + self.y
    }
}

/// The column of the text on the paper (or in the flowing view), pixels.
struct Frame {
    /// Start of the text (the physical left edge of the column).
    x0: f32,
    text_w: f32,
    k: f32,
    /// The document is right to left (pictures are placed at the end).
    rtl: bool,
}

/// Paper and margins in pixels: (paper width, paper height, left, right, top, bottom).
fn page_metrics(doc: &Document, geo: &Geo, k: f32) -> (f32, f32, f32, f32, f32, f32) {
    let page = &doc.page;
    if geo.page_view {
        (
            page.width_pt * k,
            page.height_pt * k,
            page.margin_left * k,  // physical
            page.margin_right * k, // physical
            page.margin_top * k,
            page.margin_bottom * k,
        )
    } else {
        let m = FLOW_MARGIN_PT * k;
        (geo.view_w.max(120.0), 0.0, m, m, m, m)
    }
}

/// Lays `doc` out for `geo`.
pub fn build(doc: &Document, geo: &Geo, eng: &mut TextEngine, cache: &mut ParaCache) -> Layout {
    cache.begin();
    let k = geo.k();
    let (paper_w, paper_h, ml, mr, mt, mb) = page_metrics(doc, geo, k);
    let text_w = (paper_w - ml - mr).max(48.0);
    let mut pager = Pager::new(geo, paper_h, mt, mb);
    let mut out = Layout {
        placed: Vec::new(),
        tables: Vec::new(),
        pics: Vec::new(),
        pages: Vec::new(),
        width: paper_w,
        height: 0.0,
        paper_w,
        col_w: text_w,
        k,
        geo: *geo,
        items: HashMap::new(),
    };
    let labels = doc.list_labels();
    let rtl = doc
        .paras()
        .find(|p| !p.text().is_empty())
        .is_some_and(Para::rtl);
    let fr = Frame {
        x0: ml,
        text_w,
        k,
        rtl,
    };
    for (bi, block) in doc.blocks.iter().enumerate() {
        match block {
            Block::Para(p) => {
                let label = labels.get(bi).and_then(|l| l.as_deref());
                let l = cache.get(eng, p, label, text_w, k);
                place_para(&mut out, &mut pager, l, p.page_break, (bi, ml));
            }
            Block::Picture(pic) => place_picture(&mut out, &mut pager, pic, bi, &fr),
            Block::Table(rows) => place_table(&mut out, &mut pager, cache, eng, (bi, rows), &fr),
        }
    }
    finish(&mut out, &pager, (paper_w, paper_h, mb));
    cache.end();
    out
}

/// Places the lines of a paragraph; `(block, x)` says which block and where it starts.
fn place_para(
    out: &mut Layout,
    pager: &mut Pager,
    l: Rc<ParaLayout>,
    page_break: bool,
    (bi, x): (usize, f32),
) {
    if page_break {
        pager.next_page();
    }
    if pager.y > 0.0 {
        pager.y += l.before;
    }
    let start = out.placed.len();
    for (li, line) in l.lines.iter().enumerate() {
        pager.room(line.h);
        out.placed.push(Placed {
            para: Rc::clone(&l),
            line: li,
            block: bi,
            cell: None,
            x,
            y: pager.abs(),
            page: pager.page,
        });
        pager.y += line.h;
    }
    out.items.insert((bi, None), (start, out.placed.len()));
    pager.y += l.after;
}

/// Places a picture, scaled down to fit the column and the page.
fn place_picture(out: &mut Layout, pager: &mut Pager, pic: &Picture, bi: usize, fr: &Frame) {
    let (w0, h0) = (
        pic.width_pt.unwrap_or(200.0) * fr.k,
        pic.height_pt.unwrap_or(150.0) * fr.k,
    );
    let fit = (fr.text_w / w0.max(1.0))
        .min(1.0)
        .min(pager.body_h.min(1.0e6) / h0.max(1.0));
    let (w, h) = ((w0 * fit).max(8.0), (h0 * fit).max(8.0));
    pager.room(h);
    let x = if fr.rtl { fr.x0 + fr.text_w - w } else { fr.x0 };
    out.pics.push(PicBox {
        block: bi,
        x,
        y: pager.abs(),
        w,
        h,
        page: pager.page,
    });
    pager.y += h + 8.0 * fr.k;
}

/// Adds the page boxes and the height of the whole layout.
fn finish(out: &mut Layout, pager: &Pager, (paper_w, paper_h, mb): (f32, f32, f32)) {
    if out.geo.page_view {
        let pages = pager.page + 1;
        for i in 0..pages {
            out.pages.push(PageBox {
                y: pager.origin + i as f32 * pager.step,
                w: paper_w,
                h: paper_h,
            });
        }
        out.height = pager.origin + pages as f32 * pager.step;
    } else {
        let h = pager.top + pager.y + mb;
        out.pages.push(PageBox {
            y: 0.0,
            w: paper_w,
            h,
        });
        out.height = h;
    }
}

/// The paragraphs of the cells of one row, laid out for the column width.
fn row_cells(
    cache: &mut ParaCache,
    eng: &mut TextEngine,
    row: &[String],
    width: f32,
    k: f32,
) -> Vec<Rc<ParaLayout>> {
    row.iter()
        .map(|t| cache.get(eng, &Para::new(Style::Normal, t), None, width, k))
        .collect()
}

/// Cells of a table as a grid of equal columns; a row stays on one page.
fn place_table(
    out: &mut Layout,
    pager: &mut Pager,
    cache: &mut ParaCache,
    eng: &mut TextEngine,
    (bi, rows): (usize, &[Vec<String>]),
    fr: &Frame,
) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
    let col_w = fr.text_w / cols as f32;
    let pad = CELL_PAD_PT * fr.k;
    let rtl = {
        let all: String = rows.iter().flatten().map(String::as_str).collect();
        ultar_office::doc::is_rtl(&all)
    };
    let col_x: Vec<f32> = (0..cols)
        .map(|c| {
            if rtl {
                fr.x0 + fr.text_w - (c + 1) as f32 * col_w
            } else {
                fr.x0 + c as f32 * col_w
            }
        })
        .collect();
    let mut tb = TableBox {
        block: bi,
        x: fr.x0,
        col_x: col_x.clone(),
        col_w,
        rows: Vec::new(),
        rtl,
    };
    for (ri, row) in rows.iter().enumerate() {
        let cells = row_cells(cache, eng, row, (col_w - 2.0 * pad).max(16.0), fr.k);
        let inner = cells.iter().map(|c| c.h).fold(0.0_f32, f32::max);
        let row_h = inner + 2.0 * pad;
        pager.room(row_h);
        let top = pager.abs();
        place_row(out, pager.page, (bi, ri), cells, &col_x, (top, pad));
        tb.rows.push((top, row_h, pager.page));
        pager.y += row_h;
    }
    pager.y += 8.0 * fr.k;
    out.tables.push(tb);
}

/// Places the lines of the cells of row `ri`, each below `top` and inside `pad`.
fn place_row(
    out: &mut Layout,
    page: usize,
    (bi, ri): (usize, usize),
    cells: Vec<Rc<ParaLayout>>,
    col_x: &[f32],
    (top, pad): (f32, f32),
) {
    for (ci, l) in cells.into_iter().enumerate() {
        let cell = Cell { row: ri, col: ci };
        let start = out.placed.len();
        for (li, line) in l.lines.iter().enumerate() {
            out.placed.push(Placed {
                para: Rc::clone(&l),
                line: li,
                block: bi,
                cell: Some(cell),
                x: col_x[ci] + pad,
                y: top + pad + line.y,
                page,
            });
        }
        out.items
            .insert((bi, Some(cell)), (start, out.placed.len()));
    }
}

#[cfg(test)]
mod tests;
