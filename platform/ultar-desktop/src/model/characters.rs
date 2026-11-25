//! The characters source of the overview search (`search.source.characters`): a small
//! table of emoji, symbols and the Arabic letters, each with an English and a real Arabic
//! name. Typing a name (in either language, Arabic without regard to vowel marks and letter
//! variants when `search.arabic_variants` is on) offers the character; choosing it puts the
//! character into the search field. No service is involved – the table ships with the shell.

use ultar_i18n::{arabic, Lang};

/// One character the search offers, with its name in English and Arabic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Character {
    /// The character itself (what choosing it inserts).
    pub ch: &'static str,
    /// Its English name (words to match).
    pub en: &'static str,
    /// Its Arabic name (real Arabic, the reference).
    pub ar: &'static str,
}

impl Character {
    /// The name in `lang` (German falls back to English: the table carries ar and en).
    pub fn name(self, lang: Lang) -> &'static str {
        match lang {
            Lang::Ar => self.ar,
            _ => self.en,
        }
    }
}

/// The table: the Arabic letters, then common emoji and symbols. Small on purpose – this
/// is a picker of what people search for by name, not a full Unicode database.
pub const TABLE: &[Character] = &[
    // The Arabic letters (isolated form), by their names.
    Character { ch: "ا", en: "alif", ar: "ألف" },
    Character { ch: "ب", en: "ba", ar: "باء" },
    Character { ch: "ت", en: "ta", ar: "تاء" },
    Character { ch: "ث", en: "tha", ar: "ثاء" },
    Character { ch: "ج", en: "jim", ar: "جيم" },
    Character { ch: "ح", en: "ha", ar: "حاء" },
    Character { ch: "خ", en: "kha", ar: "خاء" },
    Character { ch: "د", en: "dal", ar: "دال" },
    Character { ch: "ذ", en: "dhal", ar: "ذال" },
    Character { ch: "ر", en: "ra", ar: "راء" },
    Character { ch: "ز", en: "zay", ar: "زاي" },
    Character { ch: "س", en: "sin", ar: "سين" },
    Character { ch: "ش", en: "shin", ar: "شين" },
    Character { ch: "ص", en: "sad", ar: "صاد" },
    Character { ch: "ض", en: "dad", ar: "ضاد" },
    Character { ch: "ط", en: "ta", ar: "طاء" },
    Character { ch: "ظ", en: "za", ar: "ظاء" },
    Character { ch: "ع", en: "ayn", ar: "عين" },
    Character { ch: "غ", en: "ghayn", ar: "غين" },
    Character { ch: "ف", en: "fa", ar: "فاء" },
    Character { ch: "ق", en: "qaf", ar: "قاف" },
    Character { ch: "ك", en: "kaf", ar: "كاف" },
    Character { ch: "ل", en: "lam", ar: "لام" },
    Character { ch: "م", en: "mim", ar: "ميم" },
    Character { ch: "ن", en: "nun", ar: "نون" },
    Character { ch: "ه", en: "ha", ar: "هاء" },
    Character { ch: "و", en: "waw", ar: "واو" },
    Character { ch: "ي", en: "ya", ar: "ياء" },
    // Emoji and symbols.
    Character { ch: "😀", en: "grinning face", ar: "وجه مبتسم" },
    Character { ch: "😂", en: "face with tears of joy", ar: "وجه بدموع الفرح" },
    Character { ch: "❤", en: "red heart", ar: "قلب أحمر" },
    Character { ch: "⭐", en: "star", ar: "نجمة" },
    Character { ch: "✓", en: "check mark", ar: "علامة صح" },
    Character { ch: "✗", en: "cross mark", ar: "علامة خطأ" },
    Character { ch: "→", en: "right arrow", ar: "سهم إلى اليمين" },
    Character { ch: "←", en: "left arrow", ar: "سهم إلى اليسار" },
    Character { ch: "↑", en: "up arrow", ar: "سهم إلى الأعلى" },
    Character { ch: "↓", en: "down arrow", ar: "سهم إلى الأسفل" },
    Character { ch: "★", en: "black star", ar: "نجمة مصمتة" },
    Character { ch: "♪", en: "music note", ar: "نوتة موسيقية" },
    Character { ch: "☀", en: "sun", ar: "شمس" },
    Character { ch: "☂", en: "umbrella", ar: "مظلة" },
    Character { ch: "✉", en: "envelope", ar: "ظرف رسالة" },
    Character { ch: "☎", en: "telephone", ar: "هاتف" },
    Character { ch: "✈", en: "airplane", ar: "طائرة" },
    Character { ch: "⚙", en: "gear", ar: "ترس" },
    Character { ch: "🔒", en: "lock", ar: "قفل" },
    Character { ch: "🔑", en: "key", ar: "مفتاح" },
    Character { ch: "📁", en: "folder", ar: "مجلد" },
    Character { ch: "📄", en: "document", ar: "مستند" },
    Character { ch: "🖼", en: "picture", ar: "صورة" },
    Character { ch: "🎵", en: "music", ar: "موسيقى" },
    Character { ch: "🌙", en: "crescent moon", ar: "هلال" },
    Character { ch: "☺", en: "smiling face", ar: "وجه مبتسم" },
    Character { ch: "،", en: "arabic comma", ar: "فاصلة عربية" },
    Character { ch: "؟", en: "arabic question mark", ar: "علامة استفهام عربية" },
    Character { ch: "٭", en: "arabic star", ar: "نجمة عربية" },
    Character { ch: "﷽", en: "basmala", ar: "بسملة" },
];

/// The entry whose character is exactly `ch`, if the table has it.
pub fn find(ch: &str) -> Option<&'static Character> {
    TABLE.iter().find(|c| c.ch == ch)
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
    TABLE
        .iter()
        .filter(|c| {
            c.ch == query
                || matches(c.en, query, variants)
                || matches(c.ar, query, variants)
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
        for c in TABLE {
            assert!(!c.ch.is_empty() && !c.en.is_empty() && !c.ar.is_empty());
            assert_eq!(find(c.ch).map(|f| f.ch), Some(c.ch));
            assert_eq!(c.name(Lang::Ar), c.ar);
            assert_eq!(c.name(Lang::En), c.en);
            assert_eq!(c.name(Lang::De), c.en);
        }
        assert!(find("not-a-character").is_none());
    }
}
