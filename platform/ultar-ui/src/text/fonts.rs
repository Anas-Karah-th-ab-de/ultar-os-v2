//! The embedded font files and the per-character font choice.
//!
//! Only fonts that ship in the repository (`assets/fonts`, provenance in `SOURCES.md` there)
//! are used; nothing is downloaded. All files are static instances: the rasteriser never has
//! to apply variation axes.
//!
//! Choice rules (design spec §3 "Schrift", tokens `[type]`):
//! - Arabic letters use the Arabic face of the role: `family_arabic_text` for running text,
//!   `family_arabic_title` for titles, `family_mono_arabic` inside fixed-width text.
//! - Everything else – Latin letters, punctuation, symbols and the digits 0–9 – uses
//!   `family_latin` (or `family_mono`), also inside Arabic text, so that numbers look the
//!   same in every language. Arabic-Indic digits belong to the Arabic block and use the
//!   Arabic face.
//! - Spaces and joiners take the font of the character before them.
//! - A character the preferred face lacks falls back along [`Fonts::pick`]'s chain.
//! - Weights map to the files that exist, see [`Weight`].

use crate::tokens::typography;
use rustybuzz::Face;
use std::fmt;

macro_rules! fonts {
    ($( $(#[$doc:meta])* $variant:ident, $file:literal, $family:expr; )*) => {
        /// One embedded font file.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum FontId {
            $( $(#[$doc])* $variant, )*
        }

        impl FontId {
            /// All embedded fonts.
            pub const ALL: &'static [FontId] = &[ $( FontId::$variant, )* ];

            /// File name under `assets/fonts`.
            pub const fn file_name(self) -> &'static str {
                match self { $( FontId::$variant => $file, )* }
            }

            /// Family name as stored in the font file (and as named by the tokens).
            pub const fn family(self) -> &'static str {
                match self { $( FontId::$variant => $family, )* }
            }

            fn bytes(self) -> &'static [u8] {
                match self {
                    $( FontId::$variant =>
                        include_bytes!(concat!("../../../../assets/fonts/", $file)), )*
                }
            }
        }
    };
}

fonts! {
    /// Noto Naskh Arabic Regular – Arabic running text at weight 400.
    Arabic, "NotoNaskhArabic-Regular.ttf", typography::FAMILY_ARABIC_TEXT;
    /// Noto Naskh Arabic Medium – Arabic running text at weight 500.
    ArabicMedium, "NotoNaskhArabic-Medium.ttf", typography::FAMILY_ARABIC_TEXT;
    /// Noto Naskh Arabic Bold – Arabic running text at weights 600 and 700.
    ArabicBold, "NotoNaskhArabic-Bold.ttf", typography::FAMILY_ARABIC_TEXT;
    /// Noto Kufi Arabic Regular – Arabic titles at weights 400 and 500.
    ArabicTitle, "NotoKufiArabic-Regular.ttf", typography::FAMILY_ARABIC_TITLE;
    /// Noto Kufi Arabic Bold – Arabic titles at weights 600 and 700.
    ArabicTitleBold, "NotoKufiArabic-Bold.ttf", typography::FAMILY_ARABIC_TITLE;
    /// Noto Sans Arabic Regular – Arabic inside fixed-width text.
    ArabicSans, "NotoSansArabic-Regular.ttf", typography::FAMILY_MONO_ARABIC;
    /// Noto Sans Arabic Bold – Arabic inside bold fixed-width text.
    ArabicSansBold, "NotoSansArabic-Bold.ttf", typography::FAMILY_MONO_ARABIC;
    /// Noto Sans Regular – Latin text, digits, symbols at weights 400 and 500.
    Latin, "NotoSans-Regular.ttf", typography::FAMILY_LATIN;
    /// Noto Sans Bold – Latin text, digits, symbols at weights 600 and 700.
    LatinBold, "NotoSans-Bold.ttf", typography::FAMILY_LATIN;
    /// Noto Sans Mono Regular – fixed-width text at weights 400 and 500.
    Mono, "NotoSansMono-Regular.ttf", typography::FAMILY_MONO;
    /// Noto Sans Mono Bold – fixed-width text at weights 600 and 700.
    MonoBold, "NotoSansMono-Bold.ttf", typography::FAMILY_MONO;
}

/// Token families (`[type] family_*`) for which no font file is embedded. Empty: every token
/// family ships; the function stays so that a future token without a file is noticed.
pub fn missing_families() -> Vec<&'static str> {
    [
        typography::FAMILY_ARABIC_TEXT,
        typography::FAMILY_ARABIC_TITLE,
        typography::FAMILY_LATIN,
        typography::FAMILY_MONO,
        typography::FAMILY_MONO_ARABIC,
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
    /// Titles and headings.
    Title,
    /// Fixed-width text.
    Mono,
}

/// Font weight, one of the token weights.
///
/// Files exist for: Arabic text 400 / 500 / 700, every other family 400 / 700. A weight
/// without its own file uses the nearest one: 500 → 400 (except Arabic text), 600 → 700.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Weight {
    /// 400.
    #[default]
    Regular,
    /// 500.
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

/// The parsed embedded fonts, indexed by [`FontId`].
pub(crate) struct Fonts {
    faces: Vec<Face<'static>>,
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

/// The Arabic face of a role at a weight.
pub(crate) const fn arabic_face(role: FontRole, weight: Weight) -> FontId {
    match (role, weight) {
        (FontRole::Text, Weight::Regular) => FontId::Arabic,
        (FontRole::Text, Weight::Medium) => FontId::ArabicMedium,
        (FontRole::Text, _) => FontId::ArabicBold,
        (FontRole::Title, Weight::Regular | Weight::Medium) => FontId::ArabicTitle,
        (FontRole::Title, _) => FontId::ArabicTitleBold,
        (FontRole::Mono, Weight::Regular | Weight::Medium) => FontId::ArabicSans,
        (FontRole::Mono, _) => FontId::ArabicSansBold,
    }
}

/// The face for everything that is not Arabic (Latin, digits, symbols) of a role at a weight.
pub(crate) const fn latin_face(role: FontRole, weight: Weight) -> FontId {
    match (role, weight.uses_bold_file()) {
        (FontRole::Mono, false) => FontId::Mono,
        (FontRole::Mono, true) => FontId::MonoBold,
        (_, false) => FontId::Latin,
        (_, true) => FontId::LatinBold,
    }
}

impl Fonts {
    pub(crate) fn new() -> Result<Fonts, TextError> {
        let mut faces = Vec::with_capacity(FontId::ALL.len());
        for id in FontId::ALL {
            faces.push(Face::from_slice(id.bytes(), 0).ok_or(TextError::InvalidFont(*id))?);
        }
        Ok(Fonts { faces })
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
    ///
    /// Fallback chain for Arabic letters: the role's Arabic face, the Arabic text face, Noto
    /// Sans Arabic, then the Latin face. For all other characters: the role's Latin (or
    /// mono) face, the other one of the two (box drawing exists only in the mono face), then
    /// the Arabic faces. A character no face has gets the first font's "missing" glyph.
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
        let arabic = arabic_face(role, weight);
        let text = arabic_face(FontRole::Text, weight);
        let sans = arabic_face(FontRole::Mono, weight);
        let latin = latin_face(role, weight);
        let other = match role {
            FontRole::Mono => latin_face(FontRole::Text, weight),
            _ => latin_face(FontRole::Mono, weight),
        };
        let order = if is_arabic(ch) {
            [arabic, text, sans, latin, other]
        } else {
            [latin, other, arabic, text, sans]
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
    use rustybuzz::ttf_parser::name_id;

    fn name(face: &Face<'_>, id: u16) -> Option<String> {
        face.names()
            .into_iter()
            .find(|n| n.name_id == id && n.is_unicode())
            .and_then(|n| n.to_string())
    }

    #[test]
    fn fonts_load_and_names_match_the_files() {
        let fonts = Fonts::new().unwrap();
        for (index, id) in FontId::ALL.iter().enumerate() {
            assert_eq!(*id as usize, index);
            let face = fonts.face(*id);
            let family = name(face, name_id::TYPOGRAPHIC_FAMILY)
                .or_else(|| name(face, name_id::FAMILY))
                .unwrap();
            assert_eq!(family, id.family(), "{}", id.file_name());
            assert!(fonts.scale(*id, 16.0) > 0.0);
            assert!(
                !face.is_variable(),
                "{}: static instances only",
                id.file_name()
            );
        }
    }

    #[test]
    fn the_weight_of_each_file_is_the_one_it_is_used_for() {
        let fonts = Fonts::new().unwrap();
        let weight = |id: FontId| fonts.face(id).weight().to_number();
        for role in [FontRole::Text, FontRole::Title, FontRole::Mono] {
            assert_eq!(weight(arabic_face(role, Weight::Regular)), 400);
            assert_eq!(weight(arabic_face(role, Weight::Semibold)), 700);
            assert_eq!(weight(arabic_face(role, Weight::Bold)), 700);
            assert_eq!(weight(latin_face(role, Weight::Regular)), 400);
            assert_eq!(weight(latin_face(role, Weight::Medium)), 400);
            assert_eq!(weight(latin_face(role, Weight::Bold)), 700);
        }
        assert_eq!(weight(arabic_face(FontRole::Text, Weight::Medium)), 500);
        assert_eq!(weight(arabic_face(FontRole::Title, Weight::Medium)), 400);
    }

    #[test]
    fn font_choice_by_script_role_and_weight() {
        let fonts = Fonts::new().unwrap();
        let pick = |ch, prev, role, weight| fonts.pick(ch, prev, role, weight);
        let (text, title, mono) = (FontRole::Text, FontRole::Title, FontRole::Mono);
        assert_eq!(pick('س', None, text, Weight::Regular), FontId::Arabic);
        assert_eq!(pick('س', None, text, Weight::Medium), FontId::ArabicMedium);
        assert_eq!(pick('س', None, text, Weight::Semibold), FontId::ArabicBold);
        assert_eq!(
            pick('س', None, title, Weight::Bold),
            FontId::ArabicTitleBold
        );
        assert_eq!(pick('س', None, title, Weight::Regular), FontId::ArabicTitle);
        assert_eq!(pick('س', None, mono, Weight::Regular), FontId::ArabicSans);
        assert_eq!(pick('س', None, mono, Weight::Bold), FontId::ArabicSansBold);
        assert_eq!(pick('a', None, text, Weight::Medium), FontId::Latin);
        assert_eq!(pick('a', None, title, Weight::Semibold), FontId::LatinBold);
        assert_eq!(pick('a', None, mono, Weight::Regular), FontId::Mono);
        assert_eq!(pick('a', None, mono, Weight::Bold), FontId::MonoBold);
        assert_eq!(pick('…', None, text, Weight::Regular), FontId::Latin);
    }

    #[test]
    fn digits_are_latin_and_neutrals_follow_their_neighbour() {
        let fonts = Fonts::new().unwrap();
        let (text, regular) = (FontRole::Text, Weight::Regular);
        for digit in '0'..='9' {
            // Also directly after an Arabic letter: 0–9 always come from the Latin face.
            let font = fonts.pick(digit, Some(FontId::Arabic), text, regular);
            assert_eq!(font, FontId::Latin, "{digit}");
            let title = fonts.pick(
                digit,
                Some(FontId::ArabicTitleBold),
                FontRole::Title,
                Weight::Bold,
            );
            assert_eq!(title, FontId::LatinBold);
        }
        // Arabic-Indic digits and Arabic punctuation are Arabic-block characters.
        assert_eq!(fonts.pick('٣', None, text, regular), FontId::Arabic);
        assert_eq!(
            fonts.pick('،', Some(FontId::Latin), text, regular),
            FontId::Arabic
        );
        assert_eq!(
            fonts.pick(' ', Some(FontId::Arabic), text, regular),
            FontId::Arabic
        );
        assert_eq!(
            fonts.pick(' ', Some(FontId::Latin), text, regular),
            FontId::Latin
        );
        assert_eq!(fonts.pick(' ', None, text, regular), FontId::Latin);
    }

    #[test]
    fn fallback_chain_reaches_a_face_that_has_the_character() {
        let fonts = Fonts::new().unwrap();
        // U+0870 (Arabic Extended-B) is missing in the embedded Noto Sans Arabic Regular
        // (the mono companion) but present in Noto Naskh Arabic.
        let rare = '\u{0870}';
        assert!(!fonts.has_glyph(FontId::ArabicSans, rare));
        assert_eq!(
            fonts.pick(rare, None, FontRole::Mono, Weight::Regular),
            FontId::Arabic
        );
        // Arabic punctuation the Kufi face lacks nothing of; Latin punctuation it lacks
        // (parentheses) comes from the Latin face of the same weight.
        assert!(!fonts.has_glyph(FontId::ArabicTitleBold, '('));
        assert_eq!(
            fonts.pick(
                '(',
                Some(FontId::ArabicTitleBold),
                FontRole::Title,
                Weight::Bold
            ),
            FontId::LatinBold
        );
        // A box-drawing character exists only in the mono face.
        assert!(!fonts.has_glyph(FontId::Latin, '┼'));
        assert_eq!(
            fonts.pick('┼', None, FontRole::Text, Weight::Regular),
            FontId::Mono
        );
        // Nothing has it: the first font of the chain draws its "missing" glyph.
        assert_eq!(
            fonts.pick('\u{E000}', None, FontRole::Text, Weight::Bold),
            FontId::LatinBold
        );
    }

    #[test]
    fn every_token_family_is_embedded() {
        assert_eq!(missing_families(), Vec::<&str>::new());
        assert_eq!(Weight::Regular.value(), 400);
        assert_eq!(Weight::Bold.value(), 700);
        assert!(!Weight::Medium.uses_bold_file());
        assert!(TextError::InvalidFont(FontId::Mono)
            .to_string()
            .contains("NotoSansMono"));
    }
}
