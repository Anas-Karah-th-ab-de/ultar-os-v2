//! Itemisation (BiDi level × font) and shaping with rustybuzz.

use super::fonts::{FontId, Fonts};
use super::TextStyle;
use rustybuzz::UnicodeBuffer;
use std::ops::Range;
use unicode_bidi::Level;

/// One shaped glyph, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ShapedGlyph {
    pub(crate) font: FontId,
    pub(crate) id: u16,
    /// Byte offset in the text of the first character of the glyph's cluster.
    pub(crate) cluster: u32,
    pub(crate) advance: f32,
    pub(crate) dx: f32,
    pub(crate) dy: f32,
    pub(crate) rtl: bool,
}

/// A stretch of text with one level and one font; its glyphs are in visual order.
#[derive(Debug, Clone)]
pub(crate) struct ShapedRun {
    pub(crate) bytes: Range<usize>,
    pub(crate) glyphs: Range<usize>,
}

/// The shaped text: glyphs of all runs plus per-byte advance sums for line breaking.
#[derive(Debug, Default)]
pub(crate) struct Shaped {
    pub(crate) glyphs: Vec<ShapedGlyph>,
    pub(crate) runs: Vec<ShapedRun>,
    /// `prefix[i]` = summed advance of all clusters starting before byte `i`.
    pub(crate) prefix: Vec<f32>,
    /// `true` at bytes where a cluster starts (a line may break or be cut there).
    pub(crate) cluster_start: Vec<bool>,
}

impl Shaped {
    /// Advance of the clusters in `range` (both ends on cluster boundaries).
    pub(crate) fn width(&self, range: Range<usize>) -> f32 {
        match (self.prefix.get(range.start), self.prefix.get(range.end)) {
            (Some(a), Some(b)) => b - a,
            _ => 0.0,
        }
    }
}

/// `true` for characters that are never drawn and end a shaping run: BiDi controls and
/// line/paragraph separators.
pub fn is_bidi_control(ch: char) -> bool {
    matches!(ch,
        '\u{200E}' | '\u{200F}' | '\u{061C}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn ends_run(ch: char) -> bool {
    is_bidi_control(ch) || matches!(ch, '\n' | '\r' | '\u{2028}' | '\u{2029}' | '\u{0085}')
}

/// Splits `text` into runs of equal level and font, skipping undrawn characters.
fn itemise(
    fonts: &Fonts,
    text: &str,
    levels: &[Level],
    style: &TextStyle,
) -> Vec<(Range<usize>, Level, FontId)> {
    let mut runs: Vec<(Range<usize>, Level, FontId)> = Vec::new();
    let mut open = false;
    for (i, ch) in text.char_indices() {
        if ends_run(ch) {
            open = false;
            continue;
        }
        let level = levels.get(i).copied().unwrap_or_else(Level::ltr);
        let previous = runs.last().filter(|_| open);
        let font = fonts.pick(ch, previous.map(|r| r.2), style.role, style.weight);
        let end = i + ch.len_utf8();
        match runs.last_mut() {
            Some(run) if open && run.1 == level && run.2 == font => run.0.end = end,
            _ => runs.push((i..end, level, font)),
        }
        open = true;
    }
    runs
}

/// Shapes the whole text. `levels` are the resolved BiDi levels per byte.
pub(crate) fn shape(
    fonts: &Fonts,
    buffer: &mut Option<UnicodeBuffer>,
    text: &str,
    levels: &[Level],
    style: &TextStyle,
) -> Shaped {
    let mut shaped = Shaped {
        prefix: vec![0.0; text.len() + 1],
        cluster_start: vec![false; text.len() + 1],
        ..Shaped::default()
    };
    for (bytes, level, font) in itemise(fonts, text, levels, style) {
        let mut input = buffer.take().unwrap_or_default();
        input.push_str(&text[bytes.clone()]);
        input.set_direction(if level.is_rtl() {
            rustybuzz::Direction::RightToLeft
        } else {
            rustybuzz::Direction::LeftToRight
        });
        input.guess_segment_properties();
        let output = rustybuzz::shape(fonts.face(font), &[], input);
        let scale = fonts.scale(font, style.size);
        let first = shaped.glyphs.len();
        for (info, pos) in output.glyph_infos().iter().zip(output.glyph_positions()) {
            let cluster = bytes.start + info.cluster as usize;
            let advance = pos.x_advance as f32 * scale;
            shaped.glyphs.push(ShapedGlyph {
                font,
                id: info.glyph_id as u16,
                cluster: cluster as u32,
                advance,
                dx: pos.x_offset as f32 * scale,
                dy: -(pos.y_offset as f32) * scale,
                rtl: level.is_rtl(),
            });
            if let Some(slot) = shaped.prefix.get_mut(cluster + 1) {
                *slot += advance;
            }
            if let Some(flag) = shaped.cluster_start.get_mut(cluster) {
                *flag = true;
            }
        }
        shaped.runs.push(ShapedRun {
            bytes,
            glyphs: first..shaped.glyphs.len(),
        });
        *buffer = Some(output.clear());
    }
    let mut sum = 0.0;
    for value in shaped.prefix.iter_mut() {
        sum += *value;
        *value = sum;
    }
    shaped
}
