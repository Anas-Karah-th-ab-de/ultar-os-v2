//! Line breaking, BiDi reordering per line, ellipsis and the resulting [`TextLayout`].

use super::fonts::{arabic_face, latin_face, FontId, Fonts};
use super::shape::{shape, Shaped};
use super::{Overflow, TextAlign, TextStyle, Weight};
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
    ///
    /// Inside a cluster of several letters (a lam-alef ligature) the cluster's advance is
    /// divided evenly among its letters, so every letter boundary has a position of its own.
    /// Combining marks do not count: the caret between a letter and its mark stands after
    /// the letter.
    pub fn caret_x(&self, byte: usize) -> f32 {
        let Some(line) = self.lines.first() else {
            return 0.0;
        };
        let glyphs = &self.glyphs[line.glyphs.clone()];
        let at = byte as u32;
        let Some(cluster) = glyphs.iter().map(|g| g.cluster).filter(|c| *c <= at).max() else {
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
        let own = glyphs.iter().filter(|g| g.cluster == cluster);
        let left = own.clone().map(|g| g.x).fold(f32::MAX, f32::min);
        let right = own
            .clone()
            .map(|g| g.x + g.advance)
            .fold(f32::MIN, f32::max);
        let rtl = own.clone().any(|g| g.rtl);
        let share = self.share_before(cluster as usize..cluster_end as usize, byte);
        if rtl {
            right - (right - left) * share
        } else {
            left + (right - left) * share
        }
    }

    /// The share (0..=1) of the letters of the cluster `range` that lie before `byte`.
    fn share_before(&self, range: Range<usize>, byte: usize) -> f32 {
        if byte >= range.end {
            return 1.0;
        }
        let Some(cluster) = self.text.get(range.clone()) else {
            return 0.0;
        };
        let letters = |part: &str| part.chars().filter(|c| !is_mark(*c)).count();
        let total = letters(cluster);
        let before = cluster
            .get(..byte.saturating_sub(range.start))
            .map_or(0, letters);
        if total == 0 {
            0.0
        } else {
            before as f32 / total as f32
        }
    }

    /// The byte offset on the first line whose caret is nearest to `x` (from the line's left).
    /// Only letter boundaries are returned, never the place between a letter and its mark.
    pub fn index_at_x(&self, x: f32) -> usize {
        let Some(line) = self.lines.first() else {
            return 0;
        };
        let content = self.text.get(line.bytes.clone()).unwrap_or("");
        let boundaries = content
            .char_indices()
            .filter(|(_, ch)| !is_mark(*ch))
            .map(|(offset, _)| line.bytes.start + offset)
            .chain(std::iter::once(line.bytes.end));
        let mut best = (line.bytes.start, f32::MAX);
        for candidate in boundaries {
            let distance = (self.caret_x(candidate) - x).abs();
            if distance < best.1 {
                best = (candidate, distance);
            }
        }
        best.0
    }
}

/// `true` for combining marks (Arabic tashkeel and Quranic marks, general combining
/// diacritics, variation selectors) and joiners: characters that never stand alone.
pub(crate) fn is_mark(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0300..=0x036F | 0x0610..=0x061A | 0x064B..=0x065F | 0x0670 | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4 | 0x06E7 | 0x06E8 | 0x06EA..=0x06ED | 0x08D3..=0x08FF
        | 0x200C | 0x200D | 0xFE00..=0xFE0F | 0x1AB0..=0x1AFF | 0x20D0..=0x20FF)
}

struct Metrics {
    line_height: f32,
    baseline: f32,
}

/// Line height and baseline of a style. They depend on the style only, never on the text, so
/// that Arabic, Latin and mixed labels of one style share their baseline: the line box is
/// centred on the ascent and descent of the role's Latin face and, in Arabic line metrics,
/// of the role's Arabic face too (its deeper descenders move the baseline up).
fn metrics(fonts: &Fonts, style: &TextStyle) -> Metrics {
    let extent = |id: FontId| {
        let (face, scale) = (fonts.face(id), fonts.scale(id, style.size));
        (
            f32::from(face.ascender()) * scale,
            -f32::from(face.descender()) * scale,
        )
    };
    let (mut ascent, mut descent) = extent(latin_face(style.role, Weight::Regular));
    if style.arabic_metrics {
        let (a, d) = extent(arabic_face(style.role, Weight::Regular));
        ascent = ascent.max(a);
        descent = descent.max(d);
    }
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
    first_line: Range<usize>,
    max: f32,
) -> TextLayout {
    let first_line = text.get(..first_line.end).unwrap_or("");
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
        // The end of the first line is a boundary too (no cluster starts at a line break).
        let boundary = |cut: usize| {
            cut == first_line.len() || shaped.cluster_start.get(cut).copied().unwrap_or(false)
        };
        while cut > 0 && !(boundary(cut) && fits(cut)) {
            cut -= 1;
        }
        let kept = first_line.get(..cut).unwrap_or("").trim_end();
        // The ellipsis is a neutral character: after a run against the paragraph direction
        // (a number or Latin word in Arabic, Arabic in English) UAX #9 would resolve it to the
        // paragraph direction and put it at the far end of that run, away from the cut. A
        // direction mark of the run's direction on both sides keeps it next to the cut.
        let last_rtl = shaped
            .glyphs
            .iter()
            .filter(|g| (g.cluster as usize) < kept.len())
            .max_by_key(|g| g.cluster)
            .map(|g| g.rtl);
        let mark = match last_rtl {
            Some(rtl) if rtl != style.direction.is_rtl() => {
                Some(if rtl { '\u{200F}' } else { '\u{200E}' })
            }
            _ => None,
        };
        let mut candidate = String::with_capacity(cut + ELLIPSIS.len_utf8() + 6);
        candidate.push_str(kept);
        candidate.extend(mark);
        candidate.push(ELLIPSIS);
        candidate.extend(mark);
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
            let first = layout.lines.first().map_or(0..0, |line| line.bytes.clone());
            truncate(fonts, buffer, text, style, &shaped, first, max)
        }
        _ => layout,
    }
}
