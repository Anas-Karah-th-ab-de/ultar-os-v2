//! The compose table: two-key sequences and what they produce. Data, not code – common
//! Latin accents and symbols plus Arabic-relevant letters (hamza carriers, madda) and the
//! rial sign. The first character is usually the "dead" accent, the second the letter; a
//! few run the other way (`ss` → ß, `ae` → æ). Sequences are matched in order.

/// (sequence of two characters, result).
pub const TABLE: &[(&str, &str)] = &[
    // Acute.
    ("'a", "á"),
    ("'e", "é"),
    ("'i", "í"),
    ("'o", "ó"),
    ("'u", "ú"),
    ("'y", "ý"),
    ("'A", "Á"),
    ("'E", "É"),
    ("'I", "Í"),
    ("'O", "Ó"),
    ("'U", "Ú"),
    ("'Y", "Ý"),
    // Grave.
    ("`a", "à"),
    ("`e", "è"),
    ("`i", "ì"),
    ("`o", "ò"),
    ("`u", "ù"),
    ("`A", "À"),
    ("`E", "È"),
    ("`I", "Ì"),
    ("`O", "Ò"),
    ("`U", "Ù"),
    // Circumflex.
    ("^a", "â"),
    ("^e", "ê"),
    ("^i", "î"),
    ("^o", "ô"),
    ("^u", "û"),
    ("^A", "Â"),
    ("^E", "Ê"),
    ("^I", "Î"),
    ("^O", "Ô"),
    ("^U", "Û"),
    // Tilde.
    ("~a", "ã"),
    ("~n", "ñ"),
    ("~o", "õ"),
    ("~A", "Ã"),
    ("~N", "Ñ"),
    ("~O", "Õ"),
    // Diaeresis (the German letters are escaped, as in keymap.rs: data, not UI text).
    ("\"a", "\u{E4}"),
    ("\"e", "ë"),
    ("\"i", "ï"),
    ("\"o", "\u{F6}"),
    ("\"u", "\u{FC}"),
    ("\"y", "ÿ"),
    ("\"A", "\u{C4}"),
    ("\"E", "Ë"),
    ("\"I", "Ï"),
    ("\"O", "\u{D6}"),
    ("\"U", "\u{DC}"),
    // Cedilla, ligatures and sharp s.
    (",c", "ç"),
    (",C", "Ç"),
    ("ss", "\u{DF}"),
    ("ae", "æ"),
    ("AE", "Æ"),
    ("oe", "œ"),
    ("OE", "Œ"),
    // Symbols.
    ("oc", "©"),
    ("or", "®"),
    ("tm", "™"),
    ("sm", "℠"),
    ("c/", "¢"),
    ("/c", "¢"),
    ("l-", "£"),
    ("-l", "£"),
    ("y=", "¥"),
    ("=y", "¥"),
    ("c=", "€"),
    ("=c", "€"),
    ("so", "§"),
    ("p!", "¶"),
    ("!p", "¶"),
    ("!!", "¡"),
    ("??", "¿"),
    ("+-", "±"),
    ("-+", "±"),
    ("xx", "×"),
    (":-", "÷"),
    ("-:", "÷"),
    ("..", "…"),
    ("->", "→"),
    ("<-", "←"),
    ("oo", "°"),
    ("''", "´"),
    ("\"\"", "¨"),
    ("~~", "˜"),
    (",,", "¸"),
    // Arabic: hamza carriers and madda (hamza U+0621 with alef U+0627, waw U+0648, yeh U+064A).
    ("\u{621}\u{627}", "\u{623}"),
    ("\u{621}\u{648}", "\u{624}"),
    ("\u{621}\u{64A}", "\u{626}"),
    ("~\u{627}", "\u{622}"),
    // Arabic: the rial sign (reh U+0631, yeh U+064A).
    ("\u{631}\u{64A}", "\u{FDFC}"),
];

/// What the sequence `a` then `b` produces, if the table has it.
pub fn lookup(a: char, b: char) -> Option<&'static str> {
    TABLE.iter().find_map(|(seq, out)| {
        let mut cs = seq.chars();
        (cs.next() == Some(a) && cs.next() == Some(b) && cs.next().is_none()).then_some(*out)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_is_two_characters_and_produces_something() {
        for (seq, out) in TABLE {
            assert_eq!(seq.chars().count(), 2, "{seq}");
            assert!(!out.is_empty(), "{seq}");
        }
    }

    #[test]
    fn latin_accents_and_symbols() {
        assert_eq!(lookup('\'', 'e'), Some("é"));
        assert_eq!(lookup('\'', 'E'), Some("É"));
        assert_eq!(lookup('"', 'o'), Some("ö"));
        assert_eq!(lookup('~', 'n'), Some("ñ"));
        assert_eq!(lookup('`', 'a'), Some("à"));
        assert_eq!(lookup('^', 'u'), Some("û"));
        assert_eq!(lookup(',', 'c'), Some("ç"));
        assert_eq!(lookup('s', 's'), Some("ß"));
        assert_eq!(lookup('a', 'e'), Some("æ"));
        assert_eq!(lookup('o', 'c'), Some("©"));
        assert_eq!(lookup('c', '='), Some("€"));
        assert_eq!(lookup('.', '.'), Some("…"));
    }

    #[test]
    fn arabic_letters_and_symbol() {
        assert_eq!(lookup('ء', 'ا'), Some("أ"));
        assert_eq!(lookup('ء', 'و'), Some("ؤ"));
        assert_eq!(lookup('ء', 'ي'), Some("ئ"));
        assert_eq!(lookup('~', 'ا'), Some("آ"));
        assert_eq!(lookup('ر', 'ي'), Some("﷼"));
    }

    #[test]
    fn unknown_sequences_produce_nothing() {
        assert_eq!(lookup('q', 'z'), None);
        assert_eq!(lookup('\'', 'b'), None);
        assert_eq!(lookup('ا', 'ء'), None, "order matters");
    }
}
