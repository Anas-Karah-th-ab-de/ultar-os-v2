//! The embedded font files and the per-character font choice.
//!
//! Only fonts that ship in the repository (`assets/fonts`) are used; nothing is downloaded.
//! Families named by the tokens but not present are reported by [`missing_families`].

use crate::tokens::typography;
use rustybuzz::Face;
use std::fmt;

static ARABIC: &[u8] = include_bytes!("../../../../assets/fonts/NotoSansArabic-Regular.ttf");
static LATIN: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Regular.ttf");
static LATIN_BOLD: &[u8] = include_bytes!("../../../../assets/fonts/NotoSans-Bold.ttf");
static MONO: &[u8] = include_bytes!("../../../../assets/fonts/DejaVuSansMono.ttf");

/// One embedded font file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Noto Sans Arabic Regular – all Arabic text.
    Arabic,
    /// Noto Sans Regular – Latin text, digits, symbols.
    Latin,
    /// Noto Sans Bold – Latin text at weights 600 and 700.
    LatinBold,
    /// DejaVu Sans Mono – fixed-width text.
    Mono,
}

impl FontId {
    /// All embedded fonts.
    pub const ALL: [FontId; 4] = [
        FontId::Arabic,
        FontId::Latin,
        FontId::LatinBold,
        FontId::Mono,
    ];

    /// File name under `assets/fonts`.
    pub const fn file_name(self) -> &'static str {
        match self {
            FontId::Arabic => "NotoSansArabic-Regular.ttf",
            FontId::Latin => "NotoSans-Regular.ttf",
            FontId::LatinBold => "NotoSans-Bold.ttf",
            FontId::Mono => "DejaVuSansMono.ttf",
        }
    }

    /// Family name as stored in the font file.
    pub const fn family(self) -> &'static str {
        match self {
            FontId::Arabic => "Noto Sans Arabic",
            FontId::Latin | FontId::LatinBold => "Noto Sans",
            FontId::Mono => "DejaVu Sans Mono",
        }
    }

    fn bytes(self) -> &'static [u8] {
        match self {
            FontId::Arabic => ARABIC,
            FontId::Latin => LATIN,
            FontId::LatinBold => LATIN_BOLD,
            FontId::Mono => MONO,
        }
    }
}

/// Token families (`[type] family_*`) for which no font file is embedded; a substitute is used.
pub fn missing_families() -> Vec<&'static str> {
    [
        typography::FAMILY_ARABIC_TEXT,
        typography::FAMILY_ARABIC_TITLE,
        typography::FAMILY_LATIN,
        typography::FAMILY_MONO,
    ]
    .into_iter()
    .filter(|family| !FontId::ALL.iter().any(|font| font.family() == *family))
    .collect()
}

/// Which kind of text a style is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FontRole {
    /// Running text and controls.
    #[default]
    Text,
    /// Titles (same files as `Text` until a title family is embedded).
    Title,
    /// Fixed-width text.
    Mono,
}

/// Font weight, one of the token weights.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Weight {
    /// 400.
    #[default]
    Regular,
    /// 500 (drawn with the regular file).
    Medium,
    /// 600 (drawn with the bold file).
    Semibold,
    /// 700.
    Bold,
}

impl Weight {
    /// The numeric weight from `[type] weights`.
    pub const fn value(self) -> u16 {
        typography::WEIGHTS[self as usize]
    }

    /// `true` when the bold file is used.
    pub const fn uses_bold_file(self) -> bool {
        matches!(self, Weight::Semibold | Weight::Bold)
    }
}

/// Why the text engine could not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextError {
    /// An embedded font file is not a valid font.
    InvalidFont(FontId),
}

impl fmt::Display for TextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TextError::InvalidFont(id) => write!(f, "embedded font {} is invalid", id.file_name()),
        }
    }
}

impl std::error::Error for TextError {}

/// The parsed embedded fonts.
pub(crate) struct Fonts {
    faces: [Face<'static>; 4],
}

/// `true` for characters of the Arabic script blocks.
pub(crate) fn is_arabic(ch: char) -> bool {
    matches!(u32::from(ch),
        0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}

/// `true` for characters that take the font of their neighbour (spaces, joiners).
fn is_neutral(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '\u{200C}' | '\u{200D}')
}

impl Fonts {
    pub(crate) fn new() -> Result<Fonts, TextError> {
        let load = |id: FontId| Face::from_slice(id.bytes(), 0).ok_or(TextError::InvalidFont(id));
        Ok(Fonts {
            faces: [
                load(FontId::Arabic)?,
                load(FontId::Latin)?,
                load(FontId::LatinBold)?,
                load(FontId::Mono)?,
            ],
        })
    }

    pub(crate) fn face(&self, id: FontId) -> &Face<'static> {
        &self.faces[id as usize]
    }

    pub(crate) fn has_glyph(&self, id: FontId, ch: char) -> bool {
        self.face(id).glyph_index(ch).is_some()
    }

    /// Pixels per font unit at `size` pixels per em.
    pub(crate) fn scale(&self, id: FontId, size: f32) -> f32 {
        size / (self.face(id).units_per_em().max(1) as f32)
    }

    /// The font for `ch`; `previous` is the font of the character before it in the same run.
    pub(crate) fn pick(
        &self,
        ch: char,
        previous: Option<FontId>,
        role: FontRole,
        weight: Weight,
    ) -> FontId {
        if let Some(prev) = previous {
            if is_neutral(ch) && self.has_glyph(prev, ch) {
                return prev;
            }
        }
        let latin = if weight.uses_bold_file() {
            FontId::LatinBold
        } else {
            FontId::Latin
        };
        let order = match (role, is_arabic(ch)) {
            (FontRole::Mono, _) => [FontId::Mono, FontId::Arabic, latin],
            (_, true) => [FontId::Arabic, FontId::Mono, latin],
            (_, false) => [latin, FontId::Arabic, FontId::Mono],
        };
        order
            .into_iter()
            .find(|id| self.has_glyph(*id, ch))
            .unwrap_or(order[0])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_load_and_names_match_the_files() {
        let fonts = Fonts::new().unwrap();
        for id in FontId::ALL {
            let face = fonts.face(id);
            let family = face
                .names()
                .into_iter()
                .find(|n| n.name_id == rustybuzz::ttf_parser::name_id::FAMILY && n.is_unicode())
                .and_then(|n| n.to_string())
                .unwrap();
            assert_eq!(family, id.family(), "{}", id.file_name());
            assert!(fonts.scale(id, 16.0) > 0.0);
        }
    }

    #[test]
    fn font_choice_by_script_role_and_weight() {
        let fonts = Fonts::new().unwrap();
        let pick = |ch, prev, role, weight| fonts.pick(ch, prev, role, weight);
        assert_eq!(
            pick('س', None, FontRole::Text, Weight::Regular),
            FontId::Arabic
        );
        assert_eq!(
            pick('a', None, FontRole::Text, Weight::Medium),
            FontId::Latin
        );
        assert_eq!(
            pick('a', None, FontRole::Title, Weight::Semibold),
            FontId::LatinBold
        );
        assert_eq!(
            pick('7', None, FontRole::Text, Weight::Regular),
            FontId::Latin
        );
        assert_eq!(pick('a', None, FontRole::Mono, Weight::Bold), FontId::Mono);
        assert_eq!(
            pick(' ', Some(FontId::Arabic), FontRole::Text, Weight::Regular),
            FontId::Arabic
        );
        assert_eq!(
            pick('…', None, FontRole::Text, Weight::Regular),
            FontId::Latin
        );
        assert_eq!(
            pick('،', Some(FontId::Latin), FontRole::Text, Weight::Regular),
            FontId::Arabic
        );
    }

    #[test]
    fn gaps_are_exactly_the_known_ones() {
        // Naskh, Kufi and Noto Sans Mono are not in the repository (see INTEGRATION.md).
        assert_eq!(
            missing_families(),
            vec!["Noto Naskh Arabic", "Noto Kufi Arabic", "Noto Sans Mono"]
        );
        assert_eq!(Weight::Regular.value(), 400);
        assert_eq!(Weight::Bold.value(), 700);
        assert!(!Weight::Medium.uses_bold_file());
        assert!(TextError::InvalidFont(FontId::Mono)
            .to_string()
            .contains("DejaVu"));
    }
}
