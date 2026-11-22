//! Line breaking, BiDi reordering per line, ellipsis and the resulting [`TextLayout`].

use super::fonts::{FontId, Fonts};
use super::shape::{shape, Shaped};
use super::{Overflow, TextAlign, TextStyle};
use rustybuzz::UnicodeBuffer;
use std::ops::Range;
use unicode_bidi::{BidiInfo, Level, ParagraphInfo};

/// The character that marks cut-off text.
pub const ELLIPSIS: char = '…';

/// One glyph placed on its line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PositionedGlyph {
    /// Font file the glyph comes from.
    pub font: FontId,
    /// Glyph index in that font.
    pub id: u16,
    /// Pen position from the left end of the line, in pixels.
    pub x: f32,
    /// Horizontal shaping offset (mark positioning).
    pub dx: f32,
    /// Vertical shaping offset, positive downwards (mark positioning).
    pub dy: f32,
    /// Advance width.
    pub advance: f32,
    /// Byte offset in the laid-out text of the first character of the glyph's cluster.
    pub cluster: u32,
    /// `true` when the glyph belongs to a right-to-left run.
    pub rtl: bool,
}

/// One visual line.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    /// Its glyphs in [`TextLayout::glyphs`], left to right.
    pub glyphs: Range<usize>,
    /// The bytes of the text on this line.
    pub bytes: Range<usize>,
    /// Summed advance in pixels.
    pub width: f32,
    /// Top of the line box from the top of the layout.
    pub top: f32,
    /// Baseline from the top of the layout.
    pub baseline: f32,
}

/// Shaped, reordered and broken text, ready to measure and draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TextLayout {
    /// The text that was laid out (differs from the input when an ellipsis was added).
    pub text: String,
    /// All glyphs, line after line, each line in visual order from the left.
    pub glyphs: Vec<PositionedGlyph>,
    /// The lines, top to bottom (always at least one).
    pub lines: Vec<Line>,
    /// Width of the widest line.
    pub width: f32,
    /// Height of all lines.
    pub height: f32,
    /// `true` when text was cut and an ellipsis added.
    pub truncated: bool,
    /// `true` when the paragraph direction is right-to-left.
    pub rtl: bool,
    /// Font size in pixels the text was shaped at.
    pub size: f32,
}

impl TextLayout {
    /// Distance from the left edge of a box of `box_width` to the left end of `line`.
    pub fn line_offset(&self, line: &Line, box_width: f32, align: TextAlign) -> f32 {
        let free = box_width - line.width;
        match (align, self.rtl) {
            (TextAlign::Center, _) => free / 2.0,
            (TextAlign::Start, false) | (TextAlign::End, true) => 0.0,
            (TextAlign::Start, true) | (TextAlign::End, false) => free,
        }
    }

    /// Cluster byte offsets of all glyphs in visual order (left to right, line by line).
    pub fn visual_clusters(&self) -> Vec<u32> {
        self.glyphs.iter().map(|g| g.cluster).collect()
    }

    /// Caret position for the byte offset `byte` on the first line, from the line's left end.
    pub fn caret_x(&self, byte: usize) -> f32 {
        let Some(line) = self.lines.first() else {
            return 0.0;
        };
        let glyphs = &self.glyphs[line.glyphs.clone()];
        let byte = byte as u32;
        let Some(cluster) = glyphs
            .iter()
            .map(|g| g.cluster)
            .filter(|c| *c <= byte)
            .max()
        else {
            return glyphs
                .first()
                .map_or(0.0, |g| if g.rtl { line.width } else { 0.0 });
        };
        let cluster_end = glyphs
            .iter()
            .map(|g| g.cluster)
            .filter(|c| *c > cluster)
            .min()
            .unwrap_or(line.bytes.end as u32);
        let after = byte >= cluster_end;
        let own = glyphs.iter().filter(|g| g.cluster == cluster);
        let left = own.clone().map(|g| g.x).fold(f32::MAX, f32::min);
        let right = own
            .clone()
            .map(|g| g.x + g.advance)
            .fold(f32::MIN, f32::max);
        let rtl = own.clone().any(|g| g.rtl);
        if rtl == after {
            left
        } else {
            right
        }
    }

    /// The byte offset on the first line whose caret is nearest to `x` (from the line's left).
    pub fn index_at_x(&self, x: f32) -> usize {
        let Some(line) = self.lines.first() else {
            return 0;
        };
        let glyphs = &self.glyphs[line.glyphs.clone()];
        let ends = std::iter::once(line.bytes.end);
        let candidates = glyphs.iter().map(|g| g.cluster as usize).chain(ends);
        let mut best = (line.bytes.start, f32::MAX);
        for candidate in candidates {
            let distance = (self.caret_x(candidate) - x).abs();
            if distance < best.1 {
                best = (candidate, distance);
            }
        }
        best.0
    }
}

struct Metrics {
    line_height: f32,
    baseline: f32,
}

fn metrics(fonts: &Fonts, style: &TextStyle) -> Metrics {
    let face = fonts.face(FontId::Latin);
    let scale = fonts.scale(FontId::Latin, style.size);
    let ascent = f32::from(face.ascender()) * scale;
    let descent = -f32::from(face.descender()) * scale;
    let line_height = (style.size * style.line_height).round().max(1.0);
    Metrics {
        line_height,
        baseline: ((line_height - (ascent + descent)) / 2.0 + ascent).round(),
    }
}

/// Lays out `text`; also returns the shaping data for ellipsis cutting.
fn build(
    fonts: &Fonts,
    buffer: &mut Option<UnicodeBuffer>,
    text: &str,
    style: &TextStyle,
    wrap: Option<f32>,
) -> (TextLayout, Shaped) {
    let base = if style.direction.is_rtl() {
        Level::rtl()
    } else {
        Level::ltr()
    };
    let bidi = BidiInfo::new(text, Some(base));
    let shaped = shape(fonts, buffer, text, &bidi.levels, style);
    let m = metrics(fonts, style);
    let mut out = TextLayout {
        text: text.to_string(),
        rtl: style.direction.is_rtl(),
        size: style.size,
        ..TextLayout::default()
    };
    for para in &bidi.paragraphs {
        let content =
            text[para.range.clone()].trim_end_matches(['\n', '\r', '\u{2029}', '\u{0085}']);
        let range = para.range.start..para.range.start + content.len();
        for line in break_lines(&shaped, text, range, wrap) {
            push_line(&mut out, &bidi, para, &shaped, line, &m);
        }
    }
    if out.lines.is_empty() {
        out.lines.push(Line {
            glyphs: 0..0,
            bytes: 0..0,
            width: 0.0,
            top: 0.0,
            baseline: m.baseline,
        });
    }
    out.height = out.lines.len() as f32 * m.line_height;
    out.width = out.lines.iter().map(|l| l.width).fold(0.0, f32::max);
    (out, shaped)
}

/// Greedy line breaking at whitespace; words wider than the line break at cluster boundaries.
/// Lines ended by a break lose their trailing whitespace.
fn break_lines(
    shaped: &Shaped,
    text: &str,
    range: Range<usize>,
    wrap: Option<f32>,
) -> Vec<Range<usize>> {
    let Some(max) = wrap else {
        return vec![range];
    };
    let mut lines = Vec::new();
    let mut start = range.start;
    let mut last_break: Option<usize> = None;
    let mut previous_space = false;
    for (offset, ch) in text[range.clone()].char_indices() {
        let i = range.start + offset;
        let space = ch.is_whitespace();
        if previous_space && !space {
            last_break = Some(i);
        }
        previous_space = space;
        if space {
            continue;
        }
        let end = i + ch.len_utf8();
        while shaped.width(start..end) > max && i > start {
            let emergency = shaped.cluster_start.get(i).copied().unwrap_or(false);
            let at = match last_break.filter(|b| *b > start) {
                Some(b) => b,
                None if emergency => i,
                None => break,
            };
            lines.push(start..start + text[start..at].trim_end().len());
            start = at;
            last_break = None;
        }
    }
    lines.push(start..range.end);
    lines
}

fn push_line(
    out: &mut TextLayout,
    bidi: &BidiInfo<'_>,
    para: &ParagraphInfo,
    shaped: &Shaped,
    line: Range<usize>,
    m: &Metrics,
) {
    let first = out.glyphs.len();
    let mut pen = 0.0f32;
    if !line.is_empty() {
        let (levels, runs) = bidi.visual_runs(para, line.clone());
        for visual in runs {
            let rtl = levels.get(visual.start).is_some_and(Level::is_rtl);
            let overlapping = shaped
                .runs
                .iter()
                .filter(|r| r.bytes.start < visual.end && r.bytes.end > visual.start);
            let ordered: Vec<_> = if rtl {
                overlapping.rev().collect()
            } else {
                overlapping.collect()
            };
            for run in ordered {
                for g in &shaped.glyphs[run.glyphs.clone()] {
                    if !visual.contains(&(g.cluster as usize)) {
                        continue;
                    }
                    out.glyphs.push(PositionedGlyph {
                        font: g.font,
                        id: g.id,
                        x: pen,
                        dx: g.dx,
                        dy: g.dy,
                        advance: g.advance,
                        cluster: g.cluster,
                        rtl: g.rtl,
                    });
                    pen += g.advance;
                }
            }
        }
    }
    let top = out.lines.len() as f32 * m.line_height;
    out.lines.push(Line {
        glyphs: first..out.glyphs.len(),
        bytes: line,
        width: pen,
        top,
        baseline: top + m.baseline,
    });
}

/// Cuts `text` so that it fits `max` pixels on one line with an ellipsis at its end.
fn truncate(
    fonts: &Fonts,
    buffer: &mut Option<UnicodeBuffer>,
    text: &str,
    style: &TextStyle,
    shaped: &Shaped,
    max: f32,
) -> TextLayout {
    let first_line = text.lines().next().unwrap_or("");
    let ellipsis_width = build(
        fonts,
        buffer,
        ELLIPSIS.encode_utf8(&mut [0; 4]),
        style,
        None,
    )
    .0
    .width;
    let mut cut = first_line.len();
    let fits = |cut: usize| shaped.width(0..cut) + ellipsis_width <= max;
    loop {
        while cut > 0 && !(shaped.cluster_start.get(cut).copied().unwrap_or(false) && fits(cut)) {
            cut -= 1;
        }
        let mut candidate = String::with_capacity(cut + ELLIPSIS.len_utf8());
        candidate.push_str(first_line.get(..cut).unwrap_or("").trim_end());
        candidate.push(ELLIPSIS);
        let (mut layout, _) = build(fonts, buffer, &candidate, style, None);
        if layout.width <= max || cut == 0 {
            layout.truncated = true;
            return layout;
        }
        // Re-shaping the shorter text changed its width (different joining); cut further.
        cut -= 1;
    }
}

/// Full layout according to the style's overflow rule.
pub(crate) fn layout_text(
    fonts: &Fonts,
    buffer: &mut Option<UnicodeBuffer>,
    text: &str,
    style: &TextStyle,
    max_width: Option<f32>,
) -> TextLayout {
    let wrap = max_width.filter(|_| style.overflow == Overflow::Wrap);
    let (layout, shaped) = build(fonts, buffer, text, style, wrap);
    match max_width {
        Some(max)
            if style.overflow == Overflow::Ellipsis
                && (layout.width > max || layout.lines.len() > 1) =>
        {
            truncate(fonts, buffer, text, style, &shaped, max)
        }
        _ => layout,
    }
}
