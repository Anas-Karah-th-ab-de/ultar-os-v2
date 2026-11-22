//! Arabic-first text engine: shaping (rustybuzz), BiDi (unicode-bidi), line breaking,
//! ellipsis, measurement and cached glyph rasterisation (`ab_glyph_rasterizer`).
//!
//! All sizes here are device pixels; the painter converts from logical points.

mod cache;
mod fonts;
mod layout;
mod raster;
mod shape;

pub use cache::CacheStats;
pub use fonts::{missing_families, FontId, FontRole, TextError, Weight};
pub use layout::{Line, PositionedGlyph, TextLayout, ELLIPSIS};
pub use shape::is_bidi_control;

use crate::canvas::Canvas;
use crate::color::Color;
use crate::geom::Rect;
use crate::tokens::typography;
use cache::TwoGen;
use raster::{GlyphKey, GlyphMask, SUBPIXEL_STEPS};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use ultar_i18n::{Direction, Lang};

/// Bytes of glyph bitmaps one cache generation may hold (two generations exist).
pub const GLYPH_CACHE_BYTES: usize = 1 << 20;
/// Bytes of layouts one cache generation may hold (two generations exist).
pub const LAYOUT_CACHE_BYTES: usize = 1 << 20;

/// What happens to text wider than the available width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Overflow {
    /// One line per paragraph; the caller clips.
    #[default]
    Clip,
    /// Break into lines at word boundaries.
    Wrap,
    /// One line, cut with an ellipsis at the end.
    Ellipsis,
}

/// Horizontal placement of lines in their box, in logical terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TextAlign {
    /// At the start edge (right in right-to-left paragraphs).
    #[default]
    Start,
    /// Centred.
    Center,
    /// At the end edge.
    End,
}

/// Everything that influences shaping and line layout of a text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Font size in pixels per em.
    pub size: f32,
    /// Weight.
    pub weight: Weight,
    /// Font role.
    pub role: FontRole,
    /// Paragraph base direction.
    pub direction: Direction,
    /// Overflow rule.
    pub overflow: Overflow,
    /// Line height as a multiple of the size.
    pub line_height: f32,
}

impl TextStyle {
    /// A regular text style for `lang`: its direction and its line height from the tokens.
    pub fn new(size: f32, lang: Lang) -> TextStyle {
        TextStyle {
            size,
            weight: Weight::Regular,
            role: FontRole::Text,
            direction: lang.direction(),
            overflow: Overflow::Clip,
            line_height: match lang {
                Lang::Ar => typography::LINE_HEIGHT_ARABIC,
                _ => typography::LINE_HEIGHT_LATIN,
            },
        }
    }

    /// The same style with another weight.
    pub fn with_weight(mut self, weight: Weight) -> TextStyle {
        self.weight = weight;
        self
    }

    /// The same style with another font role.
    pub fn with_role(mut self, role: FontRole) -> TextStyle {
        self.role = role;
        self
    }

    /// The same style with another overflow rule.
    pub fn with_overflow(mut self, overflow: Overflow) -> TextStyle {
        self.overflow = overflow;
        self
    }

    fn hash_into(&self, hasher: &mut impl Hasher) {
        self.size.to_bits().hash(hasher);
        self.weight.hash(hasher);
        self.role.hash(hasher);
        self.direction.is_rtl().hash(hasher);
        self.overflow.hash(hasher);
        self.line_height.to_bits().hash(hasher);
    }
}

/// Wraps `text` in a first-strong isolate so that it keeps its own direction inside text of
/// the other direction (for example `3840 × 2160` or `60 Hz` inside Arabic).
pub fn isolate(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 6);
    out.push('\u{2068}');
    out.push_str(text);
    out.push('\u{2069}');
    out
}

struct CachedLayout {
    text: Box<str>,
    style: TextStyle,
    max_width: Option<f32>,
    layout: Rc<TextLayout>,
}

/// The text engine: fonts, shaping buffer and the two bounded caches.
pub struct TextEngine {
    fonts: fonts::Fonts,
    buffer: Option<rustybuzz::UnicodeBuffer>,
    glyphs: TwoGen<GlyphKey, Rc<GlyphMask>>,
    layouts: TwoGen<u64, Rc<CachedLayout>>,
}

impl TextEngine {
    /// An engine with the default cache limits.
    pub fn new() -> Result<TextEngine, TextError> {
        TextEngine::with_limits(GLYPH_CACHE_BYTES, LAYOUT_CACHE_BYTES)
    }

    /// An engine whose cache generations hold at most the given number of bytes each.
    pub fn with_limits(glyph_bytes: usize, layout_bytes: usize) -> Result<TextEngine, TextError> {
        Ok(TextEngine {
            fonts: fonts::Fonts::new()?,
            buffer: None,
            glyphs: TwoGen::new(glyph_bytes),
            layouts: TwoGen::new(layout_bytes),
        })
    }

    /// Shapes and lays out `text`. `max_width` (pixels) is used by the wrap and ellipsis
    /// overflow rules. Results are cached.
    pub fn layout(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> Rc<TextLayout> {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut hasher);
        style.hash_into(&mut hasher);
        max_width.map(f32::to_bits).hash(&mut hasher);
        let key = hasher.finish();
        let (fonts, buffer) = (&self.fonts, &mut self.buffer);
        let mut build = || {
            let layout = layout::layout_text(fonts, buffer, text, style, max_width);
            let cost = text.len()
                + layout.text.len()
                + layout.glyphs.len() * std::mem::size_of::<PositionedGlyph>()
                + std::mem::size_of::<CachedLayout>();
            let entry = CachedLayout {
                text: text.into(),
                style: *style,
                max_width,
                layout: Rc::new(layout),
            };
            (Rc::new(entry), cost)
        };
        let cached = self.layouts.get_or_insert_with(&key, &mut build);
        if &*cached.text == text && cached.style == *style && cached.max_width == max_width {
            return Rc::clone(&cached.layout);
        }
        // Hash collision: rebuild for this text and let it take the slot.
        let (entry, cost) = build();
        let layout = Rc::clone(&entry.layout);
        self.layouts.replace(&key, entry, cost);
        layout
    }

    /// The glyph a font maps `ch` to before shaping (its isolated form), if any.
    pub fn nominal_glyph(&self, font: FontId, ch: char) -> Option<u16> {
        self.fonts.face(font).glyph_index(ch).map(|g| g.0)
    }

    /// Draws a layout into `area` (pixels): lines are aligned horizontally by `align`, the
    /// first line starts at the top of `area`.
    pub fn draw(
        &mut self,
        canvas: &mut Canvas,
        layout: &TextLayout,
        area: Rect,
        align: TextAlign,
        color: Color,
    ) {
        let steps = f32::from(SUBPIXEL_STEPS);
        for line in &layout.lines {
            let left = area.x + layout.line_offset(line, area.w, align);
            let baseline = (area.y + line.baseline).round();
            for g in &layout.glyphs[line.glyphs.clone()] {
                let pen = left + g.x + g.dx;
                let mut whole = pen.floor();
                let mut bin = ((pen - whole) * steps).round();
                if bin >= steps {
                    bin = 0.0;
                    whole += 1.0;
                }
                let key = GlyphKey {
                    font: g.font,
                    id: g.id,
                    size_bits: layout.size.to_bits(),
                    subpixel: bin as u8,
                };
                let fonts = &self.fonts;
                let mask = self.glyphs.get_or_insert_with(&key, || {
                    let mask = raster::rasterise(fonts, key);
                    let cost = mask.data.len() + std::mem::size_of::<GlyphMask>();
                    (Rc::new(mask), cost)
                });
                let x = whole as i32 + mask.left;
                let y = (baseline + g.dy).round() as i32 + mask.top;
                // The mask length always matches its dimensions.
                let _ = canvas.fill_mask(x, y, mask.width, mask.height, &mask.data, color);
            }
        }
    }

    /// Counters of the glyph cache.
    pub fn glyph_cache_stats(&self) -> CacheStats {
        self.glyphs.stats()
    }

    /// Counters of the layout cache.
    pub fn layout_cache_stats(&self) -> CacheStats {
        self.layouts.stats()
    }

    /// Drops all cached glyphs and layouts (for example after a memory-pressure signal).
    pub fn clear_caches(&mut self) {
        self.glyphs.clear();
        self.layouts.clear();
    }
}

#[cfg(test)]
mod tests;
