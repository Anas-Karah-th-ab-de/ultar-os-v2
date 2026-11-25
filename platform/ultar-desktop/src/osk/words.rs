//! Word suggestions and autocorrect of the on-screen keyboard (`keyboard.suggestions`,
//! `keyboard.autocorrect`): small frequency-ordered word lists per language, shipped as data
//! (`assets/osk-words-*.txt`), matched without case and – for Arabic – without the
//! differences of hamza carriers, alef maqsura and taa marbuta. Nothing is learned or stored.

use ultar_ime::Lang;

/// At most this many suggestions are offered.
pub const MAX_SUGGESTIONS: usize = 3;
/// Words longer than this are not corrected (the lists hold everyday words).
const MAX_CORRECT_LEN: usize = 24;

const EN: &str = include_str!("../../assets/osk-words-en.txt");
const DE: &str = include_str!("../../assets/osk-words-de.txt");
const AR: &str = include_str!("../../assets/osk-words-ar.txt");

/// The word list of a language, most frequent first.
pub fn words(lang: Lang) -> impl Iterator<Item = &'static str> {
    let text = match lang {
        Lang::En => EN,
        Lang::De => DE,
        Lang::Ar => AR,
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
}

/// The form two words are compared in: lower case; Arabic hamza carriers, alef maqsura and
/// taa marbuta folded; tashkil and tatweel dropped.
pub fn fold(word: &str) -> String {
    word.chars()
        .filter(|c| !matches!(c, '\u{064B}'..='\u{0652}' | '\u{0640}'))
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'أ' | 'إ' | 'آ' | 'ٱ' => 'ا',
            'ى' => 'ي',
            'ة' => 'ه',
            'ؤ' => 'و',
            'ئ' => 'ي',
            other => other,
        })
        .collect()
}

/// Up to [`MAX_SUGGESTIONS`] words of `lang` that begin with `prefix` (not the prefix itself),
/// most frequent first. An empty prefix offers nothing.
pub fn suggest(lang: Lang, prefix: &str) -> Vec<&'static str> {
    let key = fold(prefix);
    if key.is_empty() {
        return Vec::new();
    }
    let mut out: Vec<&'static str> = Vec::new();
    for word in words(lang) {
        let folded = fold(word);
        if folded.starts_with(&key) && folded != key && !out.contains(&word) {
            out.push(word);
            if out.len() == MAX_SUGGESTIONS {
                break;
            }
        }
    }
    out
}

/// The correction of `word` when it is not a known word and exactly one edit (a letter
/// replaced, added, dropped or two neighbours swapped) away from a known one – the most
/// frequent such word. Case of the first letter is kept. `None`: leave it as typed.
pub fn correct(lang: Lang, word: &str) -> Option<String> {
    let n = word.chars().count();
    if !(2..=MAX_CORRECT_LEN).contains(&n) || word.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let key = fold(word);
    if words(lang).any(|w| fold(w) == key) {
        return None;
    }
    let found = words(lang).find(|w| one_edit(&key, &fold(w)))?;
    Some(keep_case(word, found))
}

/// The first letter of `typed` was upper case: so is the correction's.
fn keep_case(typed: &str, word: &str) -> String {
    let upper = typed.chars().next().is_some_and(char::is_uppercase);
    let mut chars = word.chars();
    match (upper, chars.next()) {
        (true, Some(first)) => first.to_uppercase().chain(chars).collect(),
        _ => word.to_owned(),
    }
}

/// `a` and `b` differ by one replaced, added or dropped letter, or two swapped neighbours.
fn one_edit(a: &str, b: &str) -> bool {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a == b || a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let start = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    if a.len() == b.len() {
        let swapped = start + 1 < a.len()
            && a[start] == b[start + 1]
            && a[start + 1] == b[start]
            && a[start + 2..] == b[start + 2..];
        return swapped || a[start + 1..] == b[start + 1..];
    }
    let (long, short) = if a.len() > b.len() {
        (&a, &b)
    } else {
        (&b, &a)
    };
    long[start + 1..] == short[start..]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_follow_the_prefix_in_every_language() {
        assert_eq!(suggest(Lang::En, "tha"), vec!["that", "than", "thanks"]);
        assert!(suggest(Lang::En, "").is_empty());
        assert!(suggest(Lang::De, "dan").contains(&"danke"));
        // Arabic: the hamza of the typed letters does not matter.
        assert!(
            suggest(Lang::Ar, "اريد").is_empty(),
            "the whole word is no suggestion"
        );
        assert!(suggest(Lang::Ar, "مرح").contains(&"مرحبا"));
        assert!(suggest(Lang::Ar, "ال").len() <= MAX_SUGGESTIONS);
    }

    #[test]
    fn known_words_stay_and_one_typo_is_corrected() {
        assert_eq!(correct(Lang::En, "the"), None);
        assert_eq!(correct(Lang::En, "teh").as_deref(), Some("the"));
        assert_eq!(correct(Lang::En, "helo").as_deref(), Some("hello"));
        assert_eq!(correct(Lang::En, "Thanls").as_deref(), Some("Thanks"));
        assert_eq!(correct(Lang::De, "dnake").as_deref(), Some("danke"));
        assert_eq!(correct(Lang::Ar, "شكرل").as_deref(), Some("شكرا"));
        // Far from every word, a number, or one letter: left as typed.
        assert_eq!(correct(Lang::En, "xqzvw"), None);
        assert_eq!(correct(Lang::En, "2024"), None);
        assert_eq!(correct(Lang::En, "q"), None);
    }

    #[test]
    fn arabic_folding_ignores_hamza_and_tashkil() {
        assert_eq!(fold("أَحْمَد"), fold("احمد"));
        assert_eq!(fold("مدرسة"), fold("مدرسه"));
        assert_eq!(fold("Hello"), "hello");
    }
}
