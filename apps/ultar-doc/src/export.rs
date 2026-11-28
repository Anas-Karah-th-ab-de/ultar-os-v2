//! Export to PDF. The document is laid out as paged paper at 1 pixel = 1 point (the same
//! layout the window shows, so the pages break where they do on screen), then every shaped
//! glyph is written with its glyph number into the PDF of `ultar_office::pdfout`; the fonts are
//! embedded, Arabic keeps its joining and order, italic is a slant, underline/strike/highlight,
//! list markers, tables and PNG/JPEG pictures are drawn. Text in a PDF viewer copies in the
//! order it is drawn (visual order for right-to-left lines).

use crate::layout::{build, Geo, Layout, ParaCache, Piece};
use crate::picture::{decode_png, is_png};
use crate::state::Doc;
use std::collections::HashMap;
use ultar_office::doc::{Block, Style};
use ultar_office::pdfout::Pdf;
use ultar_ui::text::{FontId, TextEngine};

/// Slant of italic text (as on screen).
const SLANT: f32 = 0.21;
const CODE_BAND: [u8; 3] = [0xF3, 0xF4, 0xF6];
const GRID: [u8; 3] = [0x99, 0x99, 0x99];

/// Why the export failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportError {
    /// A font or the text engine could not be set up.
    Fonts,
}

macro_rules! font_files {
    ($($v:ident => $f:literal,)*) => {
        fn font_bytes(id: FontId) -> &'static [u8] {
            match id { $( FontId::$v => include_bytes!(concat!("../../../assets/fonts/", $f)), )* }
        }
    };
}

font_files! {
    Arabic => "NotoNaskhArabic-Regular.ttf",
    ArabicMedium => "NotoNaskhArabic-Medium.ttf",
    ArabicSemibold => "NotoNaskhArabic-SemiBold.ttf",
    ArabicBold => "NotoNaskhArabic-Bold.ttf",
    ArabicTitle => "NotoKufiArabic-Regular.ttf",
    ArabicTitleMedium => "NotoKufiArabic-Medium.ttf",
    ArabicTitleSemibold => "NotoKufiArabic-SemiBold.ttf",
    ArabicTitleBold => "NotoKufiArabic-Bold.ttf",
    ArabicSans => "NotoSansArabic-Regular.ttf",
    ArabicSansBold => "NotoSansArabic-Bold.ttf",
    Latin => "NotoSans-Regular.ttf",
    LatinMedium => "NotoSans-Medium.ttf",
    LatinSemibold => "NotoSans-SemiBold.ttf",
    LatinBold => "NotoSans-Bold.ttf",
    Mono => "NotoSansMono-Regular.ttf",
    MonoBold => "NotoSansMono-Bold.ttf",
}

fn rgb(c: Option<[u8; 3]>) -> [u8; 3] {
    c.unwrap_or([0x1B, 0x1B, 0x1B])
}

struct Writer {
    pdf: Pdf,
    fonts: HashMap<FontId, usize>,
}

impl Writer {
    fn font(&mut self, id: FontId) -> Option<usize> {
        if let Some(&i) = self.fonts.get(&id) {
            return Some(i);
        }
        let i = self.pdf.add_font(id.file_name(), font_bytes(id)).ok()?;
        self.fonts.insert(id, i);
        Some(i)
    }

    fn piece(&mut self, page: usize, p: &Piece, line_base: f32, origin: (f32, f32), line_h: f32) {
        let st = &p.style;
        let x = origin.0 + p.x;
        if let Some(hl) = st.highlight {
            if let Some(pg) = self.pdf.page(page) {
                pg.fill_rect(x, origin.1, p.w, line_h, hl);
            }
        }
        let color = rgb(st.color);
        let baseline = origin.1 + line_base;
        if let (Some(l), false) = (&p.layout, p.space) {
            let slant = if st.italic { SLANT } else { 0.0 };
            for g in &l.glyphs {
                let Some(font) = self.font(g.font) else {
                    continue;
                };
                let text: String = l
                    .text
                    .get(g.cluster as usize..)
                    .and_then(|t| t.chars().next())
                    .map(String::from)
                    .unwrap_or_default();
                self.pdf.glyph(
                    page,
                    font,
                    g.id,
                    l.size,
                    (x + g.x + g.dx, baseline + g.dy),
                    slant,
                    color,
                    &text,
                );
            }
        }
        let thick = (st.size * 0.06).max(0.5);
        if let Some(pg) = self.pdf.page(page) {
            if st.underline {
                pg.fill_rect(x, baseline + st.size * 0.12, p.w, thick, color);
            }
            if st.strike {
                pg.fill_rect(x, baseline - st.size * 0.28, p.w, thick, color);
            }
        }
    }

    fn picture(&mut self, doc: &Doc, lay: &Layout, i: usize) {
        let pic = &lay.pics[i];
        let (page, y) = (pic.page, pic.y - lay.pages[pic.page].y);
        let Some(Block::Picture(src)) = doc.document.blocks.get(pic.block) else {
            return;
        };
        let id = if is_png(&src.bytes) {
            decode_png(&src.bytes).and_then(|d| {
                let rgb: Vec<u8> = d
                    .xrgb
                    .chunks_exact(4)
                    .flat_map(|p| [p[2], p[1], p[0]])
                    .collect();
                self.pdf.add_rgb(d.w, d.h, rgb).ok()
            })
        } else {
            self.pdf.add_jpeg(&src.bytes).ok()
        };
        let Some(pg) = self.pdf.page(page) else {
            return;
        };
        match id {
            Some(im) => pg.image(im, pic.x, y, pic.w, pic.h),
            None => {
                pg.fill_rect(pic.x, y, pic.w, pic.h, [0xEE, 0xEE, 0xEE]);
                pg.stroke_rect(pic.x, y, pic.w, pic.h, 1.0, GRID);
            }
        }
    }
}

/// The document as a PDF file.
pub fn to_pdf(doc: &Doc, title: &str) -> Result<Vec<u8>, ExportError> {
    let mut eng = TextEngine::new().map_err(|_| ExportError::Fonts)?;
    let geo = Geo {
        page_view: true,
        view_w: 1000.0,
        scale: 1.0,
        zoom: 1.0,
    };
    let lay = build(&doc.document, &geo, &mut eng, &mut ParaCache::default());
    let mut w = Writer {
        pdf: Pdf::new(),
        fonts: HashMap::new(),
    };
    w.pdf.set_title(title);
    for pg in &lay.pages {
        let i = w.pdf.add_page(pg.w, pg.h);
        if let Some(p) = w.pdf.page(i) {
            p.fill_rect(0.0, 0.0, pg.w, pg.h, PAPER_RGB);
        }
    }
    for pl in &lay.placed {
        let line = pl.line();
        let origin = (pl.x, pl.y - lay.pages[pl.page].y);
        if pl.para.style == Style::Code {
            if let Some(p) = w.pdf.page(pl.page) {
                p.fill_rect(pl.x - 4.0, origin.1, lay.col_w + 8.0, line.h, CODE_BAND);
            }
        }
        for p in line.pieces.iter().chain(line.marker.iter()) {
            w.piece(pl.page, p, line.base, origin, line.h);
        }
    }
    for t in &lay.tables {
        for &(y, h, page) in &t.rows {
            let y = y - lay.pages[page].y;
            for &cx in &t.col_x {
                if let Some(p) = w.pdf.page(page) {
                    p.stroke_rect(cx, y, t.col_w, h, 0.6, GRID);
                }
            }
        }
    }
    for i in 0..lay.pics.len() {
        w.picture(doc, &lay, i);
    }
    Ok(w.pdf.finish())
}

const PAPER_RGB: [u8; 3] = [255, 255, 255];

#[cfg(test)]
mod tests;
