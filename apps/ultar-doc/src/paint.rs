//! Drawing a laid out document into a picture: the paper, the pieces of text with their
//! bold, italic (sheared glyphs: the fonts have no italic faces), underline, strike-through,
//! colour and highlight, list markers, table grids, pictures, the selection, search hits and
//! the caret. The paper is white and the ink dark in every theme (a document is a sheet of
//! paper); the window around it follows the theme.

use crate::layout::query::{CaretPos, SelRect};
use crate::layout::{Layout, Placed};
use crate::layout::{LineBox, Piece, PieceStyle};
use crate::picture::PictureCache;
use ultar_i18n::Direction;
use ultar_office::doc::{Block, Document, Style};
use ultar_ui::canvas::{Canvas, CanvasError, PixelFormat};
use ultar_ui::geom::{PixelRect, Rect};
use ultar_ui::paint::PaintEnv;
use ultar_ui::text::{TextAlign, TextEngine, TextError, TextLayout, TextStyle};
use ultar_ui::tokens::ColorRole;
use ultar_ui::Color;

/// The colour of the paper (a document colour, not a theme colour).
pub const PAPER: Color = Color::rgb(255, 255, 255);
/// The colour of text without an own colour.
pub const INK: Color = Color::rgb(0x1B, 0x1B, 0x1B);
/// Background of a code paragraph.
const CODE_BAND: Color = Color::rgb(0xF3, 0xF4, 0xF6);
/// Lines of a table.
const GRID: Color = Color::rgb(0x99, 0x99, 0x99);
/// Slant of italic text (tangent).
const SLANT: f32 = 0.21;

/// What is marked in the document view.
#[derive(Default)]
pub struct Marks {
    pub selection: Vec<SelRect>,
    pub found: Vec<SelRect>,
    pub current: Option<SelRect>,
    pub caret: Option<CaretPos>,
}

/// The text engine and picture cache the drawing needs.
pub struct Artist {
    pub eng: TextEngine,
    pics: PictureCache,
}

/// The x of the left edge of the paper in the view: centred when it fits, else scrolled.
pub fn paper_x(lay: &Layout, view_w: f32, scroll_x: f32) -> f32 {
    if lay.paper_w <= view_w {
        ((view_w - lay.paper_w) / 2.0).floor()
    } else {
        -scroll_x
    }
}

fn color_of(c: [u8; 3]) -> Color {
    Color::rgb(c[0], c[1], c[2])
}

fn fill(canvas: &mut Canvas, x: f32, y: f32, w: f32, h: f32, color: Color) {
    let (x0, y0) = (x.round() as i32, y.round() as i32);
    let (x1, y1) = ((x + w).round() as i32, ((y + h).round() as i32).max(y0 + 1));
    canvas.blend_rect(PixelRect::new(x0, y0, x1.max(x0), y1), color);
}

/// The sheets of paper with a thin edge.
fn sheets(canvas: &mut Canvas, lay: &Layout, edge: Color, (px0, top, view_h): (f32, f32, f32)) {
    for page in &lay.pages {
        if page.y + page.h < top || page.y > top + view_h {
            continue;
        }
        let (y, w, h) = (page.y - top, page.w, page.h);
        fill(canvas, px0 - 1.0, y - 1.0, w + 2.0, h + 2.0, edge);
        fill(canvas, px0, y, w, h, PAPER);
    }
}

/// The band behind lines of code.
fn code_bands(canvas: &mut Canvas, lay: &Layout, (px0, top, view_h): (f32, f32, f32)) {
    for pl in lay.placed.iter().filter(|p| p.para.style == Style::Code) {
        if pl.y + pl.line().h >= top && pl.y <= top + view_h {
            let band = (px0 + pl.x - 4.0, pl.y - top, lay.col_w + 8.0, pl.line().h);
            fill(canvas, band.0, band.1, band.2, band.3, CODE_BAND);
        }
    }
}

/// The found words, the current hit and the selection.
fn highlights(canvas: &mut Canvas, marks: &Marks, accent: Color, (px0, top): (f32, f32)) {
    let found = Color::rgb(0xFF, 0xE0, 0x66).with_alpha(0.6);
    let current = Color::rgb(0xFF, 0x9F, 0x1C).with_alpha(0.7);
    let selection = accent.with_alpha(0.32);
    let rects = marks
        .found
        .iter()
        .map(|r| (r, found))
        .chain(marks.current.iter().map(|r| (r, current)))
        .chain(marks.selection.iter().map(|r| (r, selection)));
    for (r, color) in rects {
        fill(canvas, px0 + r.x, r.y - top, r.w, r.h, color);
    }
}

impl Artist {
    pub fn new() -> Result<Artist, TextError> {
        Ok(Artist {
            eng: TextEngine::new()?,
            pics: PictureCache::default(),
        })
    }

    /// Draws the visible part of the document into a canvas of `w` x `h` pixels, scrolled by
    /// `scroll` (x, y).
    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        doc: &Document,
        lay: &Layout,
        size: (u32, u32),
        scroll: (f32, f32),
        env: &PaintEnv,
        marks: &Marks,
        frame_label: &str,
    ) -> Result<Canvas, CanvasError> {
        let (w, h) = size;
        let mut canvas = Canvas::new(w.max(1), h.max(1), PixelFormat::Bgrx8)?;
        let pal = env.palette();
        canvas.clear(pal.get(ColorRole::Raised));
        let (view_w, view_h) = (w as f32, h as f32);
        let px0 = paper_x(lay, view_w, scroll.0); // physical
        let top = scroll.1;
        let edge = pal.get(ColorRole::TextMuted).with_alpha(0.45);
        sheets(&mut canvas, lay, edge, (px0, top, view_h));
        let visible = |y: f32, hh: f32| y + hh >= top && y <= top + view_h;
        code_bands(&mut canvas, lay, (px0, top, view_h));
        self.grids(&mut canvas, lay, (px0, top, view_h));
        let accent = pal.get(ColorRole::Accent);
        highlights(&mut canvas, marks, accent, (px0, top));
        for pl in lay.placed.iter().filter(|p| visible(p.y, p.line().h)) {
            self.line(&mut canvas, pl, px0, top);
        }
        self.pictures(
            &mut canvas,
            doc,
            lay,
            (px0, top, view_h),
            (env.direction, frame_label),
        );
        if let Some(c) = marks.caret {
            fill(
                &mut canvas,
                px0 + c.x - 0.5,
                c.y - top,
                1.5_f32.max(lay.k * 0.12),
                c.h,
                accent,
            );
        }
        Ok(canvas)
    }

    fn grids(&mut self, canvas: &mut Canvas, lay: &Layout, (px0, top, view_h): (f32, f32, f32)) {
        for t in &lay.tables {
            for &(y, h, _) in &t.rows {
                if y + h < top || y > top + view_h {
                    continue;
                }
                for &cx in &t.col_x {
                    let x = px0 + cx;
                    fill(canvas, x, y - top, t.col_w + 1.0, 1.0, GRID);
                    fill(canvas, x, y - top + h, t.col_w + 1.0, 1.0, GRID);
                    fill(canvas, x, y - top, 1.0, h, GRID);
                    fill(canvas, x + t.col_w, y - top, 1.0, h + 1.0, GRID);
                }
            }
        }
    }

    fn line(&mut self, canvas: &mut Canvas, pl: &Placed, px0: f32, top: f32) {
        let line = pl.line();
        let (ox, oy) = (px0 + pl.x, pl.y - top);
        for p in line.pieces.iter().chain(line.marker.iter()) {
            self.piece(canvas, p, line, (ox, oy));
        }
    }

    fn piece(&mut self, canvas: &mut Canvas, p: &Piece, line: &LineBox, (ox, oy): (f32, f32)) {
        let st: &PieceStyle = &p.style;
        let x = ox + p.x;
        if let Some(hl) = st.highlight {
            fill(canvas, x, oy, p.w, line.h, color_of(hl));
        }
        let color = st.color.map_or(INK, color_of);
        let baseline = oy + line.base;
        if let (Some(l), false) = (&p.layout, p.space) {
            let first = l.lines.first().map_or(0.0, |ln| ln.baseline);
            let area = Rect::new(x, baseline - first, l.width, l.height);
            if st.italic {
                self.slanted(canvas, l, area, color);
            } else {
                self.eng.draw(canvas, l, area, TextAlign::Start, color);
            }
        }
        let thick = (st.size * 0.06).max(1.0);
        if st.underline {
            fill(canvas, x, baseline + st.size * 0.12, p.w, thick, color);
        }
        if st.strike {
            fill(canvas, x, baseline - st.size * 0.28, p.w, thick, color);
        }
    }

    /// Draws `l` with sheared glyphs: the text goes into a scratch picture (dark on white)
    /// and each row of it is shifted by its height above the baseline.
    fn slanted(&mut self, canvas: &mut Canvas, l: &TextLayout, area: Rect, color: Color) {
        let pad = (l.height * SLANT).ceil() as u32 + 2;
        let (w, h) = (l.width.ceil() as u32 + pad, l.height.ceil() as u32 + 1);
        let Ok(mut tmp) = Canvas::new(w.max(1), h.max(1), PixelFormat::Bgrx8) else {
            return;
        };
        tmp.clear(Color::rgb(255, 255, 255));
        self.eng.draw(
            &mut tmp,
            l,
            Rect::new(0.0, 0.0, l.width, l.height),
            TextAlign::Start,
            Color::BLACK,
        );
        let base = l.lines.first().map_or(l.height * 0.8, |ln| ln.baseline);
        let stride = w as usize * 4;
        for row in 0..h as usize {
            let Some(src) = tmp.data().get(row * stride..(row + 1) * stride) else {
                break;
            };
            let mask: Vec<u8> = src.chunks_exact(4).map(|px| 255 - px[1]).collect();
            let shift = ((base - row as f32) * SLANT).round() as i32;
            let _ = canvas.fill_mask(
                area.x.round() as i32 + shift,
                area.y.round() as i32 + row as i32,
                w as usize,
                1,
                &mask,
                color,
            );
        }
    }

    fn pictures(
        &mut self,
        canvas: &mut Canvas,
        doc: &Document,
        lay: &Layout,
        (px0, top, view_h): (f32, f32, f32),
        (dir, label): (Direction, &str),
    ) {
        let _ = dir;
        for pic in &lay.pics {
            if pic.y + pic.h < top || pic.y > top + view_h {
                continue;
            }
            let Some(Block::Picture(src)) = doc.blocks.get(pic.block) else {
                continue;
            };
            let rect = PixelRect::new(
                (px0 + pic.x).round() as i32,
                (pic.y - top).round() as i32,
                (px0 + pic.x + pic.w).round() as i32,
                (pic.y - top + pic.h).round() as i32,
            );
            match self.pics.get(&src.bytes) {
                Some(d) => canvas.draw_scaled(rect, d.w, d.h, &d.xrgb),
                None => self.frame(
                    canvas,
                    pic.x + px0,
                    pic.y - top,
                    (pic.w, pic.h),
                    &format!("{label} {}", src.name),
                ),
            }
        }
    }

    /// A picture that cannot be shown: a framed grey box with its name.
    fn frame(&mut self, canvas: &mut Canvas, x: f32, y: f32, (w, h): (f32, f32), text: &str) {
        fill(canvas, x, y, w, h, Color::rgb(0xEE, 0xEE, 0xEE));
        for (ex, ey, ew, eh) in [
            (x, y, w, 1.0),
            (x, y + h - 1.0, w, 1.0),
            (x, y, 1.0, h),
            (x + w - 1.0, y, 1.0, h),
        ] {
            fill(canvas, ex, ey, ew, eh, GRID);
        }
        let style = TextStyle::new(13.0, ultar_i18n::Lang::En);
        let l = self.eng.layout(text, &style, None);
        let area = Rect::new(
            x + (w - l.width).max(0.0) / 2.0,
            y + (h - l.height).max(0.0) / 2.0,
            l.width,
            l.height,
        );
        self.eng.draw(canvas, &l, area, TextAlign::Start, INK);
    }
}
