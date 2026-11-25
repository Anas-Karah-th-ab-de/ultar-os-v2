//! The characters source of the overview search (`search.source.characters`): a small
//! table of emoji, symbols and the Arabic letters, each with an English and a real Arabic
//! name. Typing a name (in either language, Arabic without regard to vowel marks and letter
//! variants when `search.arabic_variants` is on) offers the character; choosing it puts the
//! character into the search field. No service is involved – the table ships with the shell.
//!
//! The names are user-visible text, so they live in `assets/characters.toml` (data, like the
//! Store's catalogue), not as literals in the code; this module only parses and searches it.

use serde::Deserialize;
use std::sync::OnceLock;
use ultar_i18n::{arabic, Lang};

/// One character the search offers, with its name in English and Arabic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Character {
    /// The character itself (what choosing it inserts).
    pub ch: String,
    /// Its English name (words to match).
    pub en: String,
    /// Its Arabic name (real Arabic, the reference).
    pub ar: String,
}

impl Character {
    /// The name in `lang` (German falls back to English: the table carries ar and en).
    pub fn name(&self, lang: Lang) -> &str {
        match lang {
            Lang::Ar => &self.ar,
            _ => &self.en,
        }
    }
}

#[derive(Deserialize)]
struct Entry {
    ch: String,
    en: String,
    ar: String,
}

#[derive(Deserialize)]
struct Table {
    character: Vec<Entry>,
}

/// The table that ships with the shell.
const SOURCE: &str = include_str!("../../assets/characters.toml");

/// The parsed table (once; a damaged file leaves nothing offered rather than stopping the
/// shell). Small on purpose – a picker of what people search for by name, not all of Unicode.
pub fn table() -> &'static [Character] {
    static TABLE: OnceLock<Vec<Character>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let parsed: Table = toml::from_str(SOURCE).unwrap_or(Table {
            character: Vec::new(),
        });
        parsed
            .character
            .into_iter()
            .map(|e| Character {
                ch: e.ch,
                en: e.en,
                ar: e.ar,
            })
            .collect()
    })
}

/// The entry whose character is exactly `ch`, if the table has it.
pub fn find(ch: &str) -> Option<&'static Character> {
    table().iter().find(|c| c.ch == ch)
}

/// `true` when `text` matches `query` (case-insensitive; Arabic also without vowel marks
/// and letter variants when `variants`).
fn matches(text: &str, query: &str, variants: bool) -> bool {
    (variants && arabic::search_matches(text, query))
        || text.to_lowercase().contains(&query.to_lowercase())
}

/// The characters whose name (English or Arabic) or the character itself matches `query`.
/// An empty query offers nothing. The caller bounds how many it shows.
pub fn search(query: &str, variants: bool) -> Vec<&'static Character> {
    let query = query.trim();
    if query.is_empty() {
        return Vec::new();
    }
    table()
        .iter()
        .filter(|c| {
            c.ch == query || matches(&c.en, query, variants) || matches(&c.ar, query, variants)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_found_in_both_languages() {
        // English name.
        assert!(search("heart", false).iter().any(|c| c.ch == "❤"));
        assert!(search("arrow", false).iter().any(|c| c.ch == "→"));
        // Real Arabic name.
        assert!(search("نجمة", false).iter().any(|c| c.ch == "⭐"));
        assert!(search("قلب", false).iter().any(|c| c.ch == "❤"));
        // The Arabic letters by name.
        assert!(search("alif", false).iter().any(|c| c.ch == "ا"));
        assert!(search("ألف", false).iter().any(|c| c.ch == "ا"));
        // The character itself.
        assert!(search("⭐", false).iter().any(|c| c.ch == "⭐"));
        // Nothing for an empty or blank query.
        assert!(search("", false).is_empty());
        assert!(search("   ", false).is_empty());
    }

    #[test]
    fn arabic_variants_ignore_vowel_marks() {
        // With vowel marks on the name: only the variants match finds it.
        let marked = "نَجْمَة";
        assert!(search(marked, true).iter().any(|c| c.ch == "⭐"));
        assert!(!search(marked, false).iter().any(|c| c.ch == "⭐"));
    }

    #[test]
    fn every_entry_has_both_names_and_a_character() {
        assert!(!table().is_empty(), "the shipped table parses");
        for c in table() {
            assert!(!c.ch.is_empty() && !c.en.is_empty() && !c.ar.is_empty());
            assert_eq!(find(&c.ch).map(|f| f.ch.as_str()), Some(c.ch.as_str()));
            assert_eq!(c.name(Lang::Ar), c.ar);
            assert_eq!(c.name(Lang::En), c.en);
            assert_eq!(c.name(Lang::De), c.en);
        }
        assert!(find("not-a-character").is_none());
    }
}
