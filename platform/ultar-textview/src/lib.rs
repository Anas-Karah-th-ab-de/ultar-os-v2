//! # ultar-textview – many lines of text in a picture
//!
//! The editor and the terminal show their text in an [`ultar_ui::widgets::PixelArea`]: this
//! crate draws the visible lines of a [`Source`] into a [`Canvas`] and answers where a point
//! lies in the text. Each line takes the direction of its first strong letter (an Arabic line
//! runs right to left and starts at the right edge, a Latin line at the left), so Arabic and
//! code mix in one file. Lines are not wrapped; a line wider than the view is clipped.
//!
//! Positions are `(line, byte)` with the byte offset inside the line ([`Pos`]).

#![forbid(unsafe_code)]

use std::rc::Rc;
use ultar_i18n::bidi::direction_of;
use ultar_i18n::Direction;
use ultar_ui::canvas::{Canvas, PixelFormat};
use ultar_ui::geom::Rect;
use ultar_ui::icon::IconCache;
use ultar_ui::paint::{PaintEnv, Painter, Resources, TextSpec};
use ultar_ui::text::{FontRole, TextAlign, TextEngine, TextLayout};
use ultar_ui::tokens::{typography, ColorRole};
use ultar_ui::Color;

/// Space between the text and the edges of the view, in points.
const PAD: f32 = 8.0;
/// Width of the caret, in points.
const CARET_W: f32 = 1.5;

/// A place in the text: line number and byte offset inside that line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Pos {
    /// Line, from 0.
    pub line: usize,
    /// Byte offset inside the line (on a character boundary).
    pub byte: usize,
}

impl Pos {
    /// The place `byte` on `line`.
    pub const fn new(line: usize, byte: usize) -> Pos {
        Pos { line, byte }
    }
}

/// The lines a view shows.
pub trait Source {
    /// Number of lines (at least one for an empty text).
    fn lines(&self) -> usize;
    /// Line `i` without its line end.
    fn line(&self, i: usize) -> &str;
}

impl Source for [String] {
    fn lines(&self) -> usize {
        self.len()
    }
    fn line(&self, i: usize) -> &str {
        self.get(i).map_or("", String::as_str)
    }
}

impl Source for Vec<String> {
    fn lines(&self) -> usize {
        self.len()
    }
    fn line(&self, i: usize) -> &str {
        self.get(i).map_or("", String::as_str)
    }
}

/// What is marked in the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Marks {
    /// The caret, when the view shows one.
    pub caret: Option<Pos>,
    /// The selected range (either order).
    pub selection: Option<(Pos, Pos)>,
}

/// Why a view could not be made or drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewError(pub String);

impl std::fmt::Display for ViewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "text view: {}", self.0)
    }
}

impl std::error::Error for ViewError {}

/// A text view: fonts, the first visible line and how lines are drawn.
pub struct TextView {
    resources: Resources,
    env: PaintEnv,
    spec: TextSpec,
    /// First visible line.
    pub first: usize,
    /// Line numbers at the start edge (editor).
    pub numbers: bool,
}

impl TextView {
    /// A view in `env` (language, theme, scale); `mono` draws in the fixed-width font.
    pub fn new(env: PaintEnv, mono: bool) -> Result<TextView, ViewError> {
        let text = TextEngine::new().map_err(|e| ViewError(format!("{e:?}")))?;
        let icons = IconCache::new().map_err(|e| ViewError(format!("{e:?}")))?;
        let role = if mono { FontRole::Mono } else { FontRole::Text };
        Ok(TextView {
            resources: Resources { text, icons },
            env,
            spec: TextSpec::new(typography::BODY).role(role),
            first: 0,
            numbers: false,
        })
    }

    /// Another language, theme or scale.
    pub fn set_env(&mut self, env: PaintEnv) {
        self.env = env;
    }

    /// The paint environment.
    pub fn env(&self) -> PaintEnv {
        self.env
    }

    fn layout(&mut self, text: &str, dir: Direction) -> Rc<TextLayout> {
        let shown = shown_text(text);
        ultar_ui::paint::layout_directed(
            &mut self.resources,
            &self.env,
            &shown,
            self.spec,
            None,
            dir,
        )
    }

    /// Height of one line in device pixels.
    pub fn line_height(&mut self) -> f32 {
        let probe = self.layout("Ag عربي", Direction::Ltr);
        probe.height.max(1.0).ceil()
    }

    /// Whole lines that fit into `height` device pixels.
    pub fn rows(&mut self, height: u32) -> usize {
        let pad = 2.0 * PAD * self.env.scale;
        ((height as f32 - pad).max(0.0) / self.line_height())
            .floor()
            .max(1.0) as usize
    }

    /// Scrolls so that `line` is visible in a view of `rows` lines.
    pub fn reveal(&mut self, line: usize, rows: usize) {
        if line < self.first {
            self.first = line;
        } else if line >= self.first + rows.max(1) {
            self.first = line + 1 - rows.max(1);
        }
    }

    /// Scrolls by `lines` (negative: up), within the text.
    pub fn scroll(&mut self, lines: isize, total: usize, rows: usize) {
        let last = total.saturating_sub(rows.max(1));
        let next = self.first as isize + lines;
        self.first = (next.max(0) as usize).min(last);
    }

    fn gutter(&mut self, total: usize) -> f32 {
        if !self.numbers {
            return 0.0;
        }
        let digits = total.max(1).to_string().len().max(3);
        let probe = self.layout(&"8".repeat(digits), Direction::Ltr);
        probe.width + 2.0 * PAD * self.env.scale
    }

    /// The text column: left and right edge in pixels for a view `width` wide.
    fn column(&mut self, width: u32, total: usize) -> (f32, f32) {
        let pad = PAD * self.env.scale;
        let gutter = self.gutter(total);
        let w = width as f32;
        if self.env.direction == Direction::Rtl {
            (pad, w - gutter - pad)
        } else {
            (gutter + pad, w - pad)
        }
    }

    /// Where the text of `line` starts (left edge of its layout) in a column.
    fn line_left(layout: &TextLayout, column: (f32, f32)) -> f32 {
        if layout.rtl {
            column.1 - layout.width
        } else {
            column.0
        }
    }

    /// Draws the visible lines of `source` into a new canvas of `width` × `height` pixels.
    pub fn render(
        &mut self,
        width: u32,
        height: u32,
        source: &dyn Source,
        marks: &Marks,
    ) -> Result<Canvas, ViewError> {
        let mut canvas = Canvas::new(width.max(1), height.max(1), PixelFormat::Bgrx8)
            .map_err(|e| ViewError(format!("{e:?}")))?;
        let total = source.lines();
        let line_h = self.line_height();
        let rows = self.rows(height);
        let column = self.column(width, total);
        let gutter = self.gutter(total);
        let s = self.env.scale;
        let top_pad = PAD * s;
        let selection = marks
            .selection
            .map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
        let last = (self.first + rows + 1).min(total);
        let mut lines = Vec::with_capacity(last.saturating_sub(self.first));
        for i in self.first..last {
            let text = source.line(i);
            let dir = direction_of(text, self.env.direction);
            lines.push((i, self.layout(text, dir)));
        }
        let numbers: Vec<Rc<TextLayout>> = if self.numbers {
            (self.first..last)
                .map(|i| self.layout(&(i + 1).to_string(), Direction::Ltr))
                .collect()
        } else {
            Vec::new()
        };
        let env = self.env;
        let mut p = Painter::new(&mut canvas, &mut self.resources, env);
        let ground = p.color(ColorRole::Surface);
        p.clear(ground);
        let pt = |px: f32| px / s;
        if gutter > 0.0 {
            let gx = if env.direction == Direction::Rtl {
                width as f32 - gutter
            } else {
                0.0
            };
            let bar = p.color(ColorRole::Raised);
            p.fill(
                Rect::new(pt(gx), 0.0, pt(gutter), pt(height as f32)),
                None,
                bar,
            );
        }
        let text_color = p.color(ColorRole::Text);
        let muted = p.color(ColorRole::TextMuted);
        let accent = p.color(ColorRole::Accent);
        let select = accent.with_alpha(0.30);
        for (row, (i, layout)) in lines.iter().enumerate() {
            let top = top_pad + row as f32 * line_h;
            let left = Self::line_left(layout, column);
            if let Some((a, b)) = selection {
                if let Some((x0, x1)) = selected_span(layout, *i, a, b, column) {
                    p.fill(
                        Rect::new(pt(x0), pt(top), pt(x1 - x0), pt(line_h)),
                        None,
                        select,
                    );
                }
            }
            let rect = Rect::new(pt(left), pt(top), pt(layout.width + 2.0), pt(line_h));
            p.text(layout, rect, TextAlign::Start, text_color);
            if let Some(n) = numbers.get(row) {
                let nx = if env.direction == Direction::Rtl {
                    width as f32 - gutter + PAD * s
                } else {
                    gutter - PAD * s - n.width
                };
                p.text(
                    n,
                    Rect::new(pt(nx), pt(top), pt(n.width + 2.0), pt(line_h)),
                    TextAlign::Start,
                    muted,
                );
            }
            if let Some(caret) = marks.caret.filter(|c| c.line == *i) {
                let x = left + layout.caret_x(caret.byte.min(layout.text.len()));
                p.fill(Rect::new(pt(x), pt(top), CARET_W, pt(line_h)), None, accent);
            }
        }
        drop(p);
        canvas.take_damage();
        Ok(canvas)
    }

    /// The place nearest to the point `(x, y)` (device pixels inside the view).
    pub fn hit(&mut self, width: u32, x: f32, y: f32, source: &dyn Source) -> Pos {
        let total = source.lines().max(1);
        let line_h = self.line_height();
        let row = ((y - PAD * self.env.scale) / line_h).floor().max(0.0) as usize;
        let line = (self.first + row).min(total - 1);
        let text = source.line(line);
        let dir = direction_of(text, self.env.direction);
        let layout = self.layout(text, dir);
        let column = self.column(width, total);
        let left = Self::line_left(&layout, column);
        let byte = layout.index_at_x(x - left).min(text.len());
        Pos::new(line, byte)
    }

    /// `true` when `line` of `source` runs right to left.
    pub fn is_rtl(&self, source: &dyn Source, line: usize) -> bool {
        direction_of(source.line(line), self.env.direction) == Direction::Rtl
    }

    /// The colour of the view's ground (for the part a picture does not cover).
    pub fn ground(&self) -> Color {
        self.env.palette().get(ColorRole::Surface)
    }
}

/// The text as drawn: a tab or another control character shows as one space (same byte
/// length, so offsets stay valid).
fn shown_text(text: &str) -> String {
    // a control character is one byte, the dot two: keep the byte length with a space
    text.chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}

/// The selected part of line `i` as pixel edges (whole width for inner lines).
fn selected_span(
    layout: &TextLayout,
    i: usize,
    a: Pos,
    b: Pos,
    column: (f32, f32),
) -> Option<(f32, f32)> {
    if i < a.line || i > b.line {
        return None;
    }
    let left = TextView::line_left(layout, column);
    let len = layout.text.len();
    let start = if i == a.line { a.byte.min(len) } else { 0 };
    let end = if i == b.line { b.byte.min(len) } else { len };
    let (mut x0, mut x1) = (left + layout.caret_x(start), left + layout.caret_x(end));
    if x0 > x1 {
        std::mem::swap(&mut x0, &mut x1);
    }
    if i != b.line {
        // the line end is selected too: a little more, so an empty line shows it
        x1 += 6.0;
    }
    (x1 > x0).then_some((x0, x1))
}

#[cfg(test)]
mod tests;
