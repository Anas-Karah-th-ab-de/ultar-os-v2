//! Draws the visible part of a sheet into a picture: cells with their formats, grid lines,
//! the frozen parts, the row and column headers, the selection and the cell being edited.

use crate::book::Book;
use crate::frame::{Frame, Seg};
use crate::sel::{Sel, Span};
use ultar_i18n::Direction;
use ultar_office::sheet::col_name;
use ultar_office::style::Align;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::Rect;
use ultar_ui::icon::IconCache;
use ultar_ui::paint::{PaintEnv, Painter, Resources, TextSpec};
use ultar_ui::text::{FontRole, Overflow, TextAlign, TextEngine, Weight};
use ultar_ui::tokens::{typography, ColorRole};
use ultar_ui::Color;

/// Space between a cell's edge and its text, in points.
const PAD: f32 = 5.0;

/// A cell being edited: where, the text, the caret (byte offset) and a selection.
#[derive(Debug, Clone, PartialEq)]
pub struct Editing {
    /// The cell.
    pub at: (u32, u32),
    /// The text typed so far.
    pub text: String,
    /// Caret as a byte offset.
    pub caret: usize,
}

/// Why drawing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DrawError(pub String);

/// Fonts and the paint environment of a grid.
pub struct Drawer {
    resources: Resources,
    /// The paint environment (language, theme, scale).
    pub env: PaintEnv,
}

fn rgb(v: u32) -> Color {
    Color::rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

fn ink_on(fill: Color) -> Color {
    if fill.relative_luminance() > 0.4 {
        Color::rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color::rgb(0xf5, 0xf5, 0xf5)
    }
}

impl Drawer {
    /// A drawer in `env`.
    pub fn new(env: PaintEnv) -> Result<Drawer, DrawError> {
        let text = TextEngine::new().map_err(|e| DrawError(format!("{e:?}")))?;
        let icons = IconCache::new().map_err(|e| DrawError(format!("{e:?}")))?;
        Ok(Drawer {
            resources: Resources { text, icons },
            env,
        })
    }

    /// Draws `book`'s shown sheet seen through `frame` into a picture of `w` × `h` device
    /// pixels.
    pub fn draw(
        &mut self,
        book: &mut Book,
        frame: &Frame,
        sel: &Sel,
        edit: Option<&Editing>,
        hint: Option<&str>,
        px: (u32, u32),
    ) -> Result<Canvas, DrawError> {
        let mut canvas = Canvas::new(px.0.max(1), px.1.max(1), PixelFormat::Bgrx8)
            .map_err(|e| DrawError(format!("{e:?}")))?;
        let env = self.env;
        let mut p = Painter::new(&mut canvas, &mut self.resources, env);
        let ground = p.color(ColorRole::Surface);
        p.clear(ground);
        let line = p.color(ColorRole::Line);
        let dir = if frame.rtl {
            Direction::Rtl
        } else {
            Direction::Ltr
        };
        let body = TextSpec::new(typography::BODY).role(FontRole::Text);
        // cells
        for row in &frame.rows {
            for col in &frame.cols {
                Self::cell(&mut p, book, (row, col), dir, body, line);
            }
        }
        if let Some(h) = hint {
            let muted = p.color(ColorRole::TextMuted);
            let layout = p.layout_directed(h, body, None, dir);
            let rect = Rect::new(frame.hdr_w, frame.hdr_h + 60.0, frame.w - frame.hdr_w, 28.0);
            p.text(&layout, rect, TextAlign::Center, muted);
        }
        Self::separators(&mut p, frame);
        Self::selection(&mut p, frame, sel);
        Self::headers(&mut p, book, frame, sel, body);
        if let Some(e) = edit {
            Self::editor(&mut p, book, frame, e, dir, body);
        }
        canvas.take_damage();
        Ok(canvas)
    }

    fn cell(
        p: &mut Painter<'_>,
        book: &mut Book,
        (row, col): (&Seg, &Seg),
        dir: Direction,
        body: TextSpec,
        line: Color,
    ) {
        let rect = Rect::new(col.pos, row.pos, col.size, row.size);
        let style = book.sheet().styles.get(&(row.idx, col.idx)).cloned();
        let fill = style.as_ref().and_then(|s| s.fill).map(rgb);
        if let Some(f) = fill {
            p.fill(rect, None, f);
        }
        let hair = p.hairline();
        // grid line at the end and the bottom edge of the cell
        p.fill(
            Rect::new(rect.x, rect.bottom() - hair, rect.w, hair),
            None,
            line,
        );
        p.fill(
            Rect::new(rect.x + rect.w - hair, rect.y, hair, rect.h),
            None,
            line,
        );
        if book.sheet().get(row.idx, col.idx).is_empty() {
            return;
        }
        let shown = book.shown(row.idx, col.idx);
        if shown.text.is_empty() {
            return;
        }
        let style = style.unwrap_or_default();
        let color = match (style.color, fill) {
            (Some(c), _) => rgb(c),
            (None, Some(f)) => ink_on(f),
            (None, None) if shown.error => p.color(ColorRole::Danger),
            (None, None) => p.color(ColorRole::Text),
        };
        let spec = if style.bold {
            body.weight(Weight::Bold)
        } else {
            body
        };
        let spec = if style.wrap {
            spec.overflow(Overflow::Wrap)
        } else {
            spec
        };
        let width = (rect.w - 2.0 * PAD).max(1.0);
        // numbers and errors (`#DIV/0!`) read left to right even in a right-to-left sheet;
        // they are placed by the sheet's start and end edges, not by their own direction
        let latin = shown.number || shown.error;
        let rtl_sheet = dir == Direction::Rtl;
        let base = if latin { Direction::Ltr } else { dir };
        let layout = p.layout_directed(&shown.text, spec, style.wrap.then_some(width), base);
        let at_start = |start: bool| {
            // `start`: the start edge of the sheet; the paragraph's own start is the left
            // edge when it runs left to right
            if latin && rtl_sheet == start {
                TextAlign::End
            } else if latin || start {
                TextAlign::Start
            } else {
                TextAlign::End
            }
        };
        let align = match style.align {
            Align::General if latin => at_start(false),
            Align::General | Align::Start => at_start(true),
            Align::Center => TextAlign::Center,
            Align::End => at_start(false),
        };
        let inner = Rect::new(rect.x + PAD, rect.y, width, rect.h);
        let prev = p.clip(rect.intersect(&Rect::new(rect.x, rect.y, rect.w - 0.5, rect.h - 0.5)));
        p.text(&layout, inner, align, color);
        p.restore_clip(prev);
    }

    fn separators(p: &mut Painter<'_>, frame: &Frame) {
        let c = p.color(ColorRole::TextMuted);
        let hair = p.hairline() * 2.0;
        let fr = frame.rows.iter().rfind(|s| s.frozen);
        let fc = frame.cols.iter().rfind(|s| s.frozen);
        if let Some(r) = fr {
            p.fill(
                Rect::new(0.0, r.pos + r.size - hair, frame.w, hair),
                None,
                c,
            );
        }
        if let Some(s) = fc {
            let x = if frame.rtl {
                s.pos
            } else {
                s.pos + s.size - hair
            };
            p.fill(Rect::new(x, 0.0, hair, frame.h), None, c);
        }
    }

    /// The selected rectangle clipped to what is on screen.
    fn visible(frame: &Frame, rect: (u32, u32, u32, u32)) -> Option<Rect> {
        let (r0, c0, r1, c1) = rect;
        let rows: Vec<&Seg> = frame
            .rows
            .iter()
            .filter(|s| (r0..=r1).contains(&s.idx))
            .collect();
        let cols: Vec<&Seg> = frame
            .cols
            .iter()
            .filter(|s| (c0..=c1).contains(&s.idx))
            .collect();
        let x0 = cols.iter().map(|s| s.pos).fold(f32::MAX, f32::min);
        let x1 = cols.iter().map(|s| s.pos + s.size).fold(f32::MIN, f32::max);
        let y0 = rows.iter().map(|s| s.pos).fold(f32::MAX, f32::min);
        let y1 = rows.iter().map(|s| s.pos + s.size).fold(f32::MIN, f32::max);
        (!rows.is_empty() && !cols.is_empty()).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    fn selection(p: &mut Painter<'_>, frame: &Frame, sel: &Sel) {
        let accent = p.color(ColorRole::Accent);
        if let Some(r) = Self::visible(frame, sel.rect()) {
            if !sel.single() {
                p.fill(r, None, accent.with_alpha(0.14));
            }
            p.stroke(r, None, 2.0, accent);
        }
        if let Some(r) = Self::visible(frame, (sel.cur.0, sel.cur.1, sel.cur.0, sel.cur.1)) {
            if !sel.single() {
                p.stroke(r.inflate(-1.0), None, 1.5, accent);
            }
        }
    }

    fn headers(p: &mut Painter<'_>, book: &Book, frame: &Frame, sel: &Sel, body: TextSpec) {
        let bar = p.color(ColorRole::Bar);
        let line = p.color(ColorRole::Line);
        let muted = p.color(ColorRole::TextMuted);
        let text = p.color(ColorRole::Text);
        let accent = p.color(ColorRole::Accent);
        let (r0, c0, r1, c1) = sel.rect();
        let hdr_x = if frame.rtl {
            frame.w - frame.hdr_w
        } else {
            0.0
        };
        // bands
        p.fill(Rect::new(0.0, 0.0, frame.w, frame.hdr_h), None, bar);
        p.fill(Rect::new(hdr_x, 0.0, frame.hdr_w, frame.h), None, bar);
        let hair = p.hairline();
        p.fill(
            Rect::new(0.0, frame.hdr_h - hair, frame.w, hair),
            None,
            line,
        );
        let edge = if frame.rtl { hdr_x } else { frame.hdr_w - hair };
        p.fill(Rect::new(edge, 0.0, hair, frame.h), None, line);
        let small = body.weight(Weight::Medium);
        for s in &frame.cols {
            let rect = Rect::new(s.pos, 0.0, s.size, frame.hdr_h);
            let on = (c0..=c1).contains(&s.idx);
            if on {
                p.fill(
                    rect,
                    None,
                    accent.with_alpha(if sel.span == Span::Cols { 0.35 } else { 0.18 }),
                );
            }
            let l = p.layout(&col_name(s.idx), small, None);
            let prev = p.clip(rect);
            p.text(&l, rect, TextAlign::Center, if on { text } else { muted });
            p.restore_clip(prev);
            p.fill(
                Rect::new(s.pos + s.size - hair, 0.0, hair, frame.hdr_h),
                None,
                line,
            );
        }
        for s in &frame.rows {
            let rect = Rect::new(hdr_x, s.pos, frame.hdr_w, s.size);
            let on = (r0..=r1).contains(&s.idx);
            if on {
                p.fill(
                    rect,
                    None,
                    accent.with_alpha(if sel.span == Span::Rows { 0.35 } else { 0.18 }),
                );
            }
            let l = p.layout(&(s.idx + 1).to_string(), small, None);
            p.text(&l, rect, TextAlign::Center, if on { text } else { muted });
            p.fill(
                Rect::new(hdr_x, s.pos + s.size - hair, frame.hdr_w, hair),
                None,
                line,
            );
        }
        let _ = book;
    }

    fn editor(
        p: &mut Painter<'_>,
        book: &Book,
        frame: &Frame,
        e: &Editing,
        dir: Direction,
        body: TextSpec,
    ) {
        let Some((x, y, w, h)) = frame.cell(e.at.0, e.at.1) else {
            return;
        };
        let bg = p.color(ColorRole::Raised);
        let accent = p.color(ColorRole::Accent);
        let text = p.color(ColorRole::Text);
        let layout = p.layout_directed(&e.text, body, None, dir);
        // the editor grows to the end of the view when the text is longer than the cell
        let need = layout.width / p.env().scale + 2.0 * PAD + 2.0;
        let width = w.max(need).min(frame.w - 4.0);
        let rect = if frame.rtl {
            Rect::new(x + w - width, y, width, h)
        } else {
            Rect::new(x, y, width, h)
        };
        p.fill(rect, None, bg);
        p.stroke(rect, None, 2.0, accent);
        let inner = Rect::new(rect.x + PAD, rect.y, rect.w - 2.0 * PAD, rect.h);
        let prev = p.clip(rect);
        p.text(&layout, inner, TextAlign::Start, text);
        let s = p.env().scale;
        let cx = layout.caret_x(e.caret.min(layout.text.len())) / s;
        let origin = if layout.rtl {
            inner.x + inner.w - layout.width / s
        } else {
            inner.x
        };
        let top = rect.y + (rect.h - layout.height / s) / 2.0;
        p.fill(
            Rect::new(origin + cx, top, 1.5, layout.height / s),
            None,
            accent,
        );
        p.restore_clip(prev);
        let _ = book;
    }
}
