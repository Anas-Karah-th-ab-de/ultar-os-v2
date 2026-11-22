//! Languages, text direction and logical layout sides.

use core::fmt;
use core::str::FromStr;

/// A language supported by Ultar OS. Arabic is the reference language and the default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    /// Arabic (Modern Standard Arabic) – reference language.
    #[default]
    Ar,
    /// English.
    En,
    /// German.
    De,
}

/// Base direction of text and layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Right-to-left (Arabic).
    Rtl,
    /// Left-to-right (English, German).
    Ltr,
}

/// A logical layout side. UI code uses these instead of left/right.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    /// The side where a line of text begins.
    Start,
    /// The side where a line of text ends.
    End,
}

/// A physical side of the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PhysicalSide {
    /// The left edge.
    Left,
    /// The right edge.
    Right,
}

/// Error returned when a text does not name a supported language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnknownLangError;

impl fmt::Display for UnknownLangError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("unknown language")
    }
}

impl core::error::Error for UnknownLangError {}

impl UnknownLangError {
    /// Message key that describes this error in the catalogs.
    pub const fn message_key(self) -> &'static str {
        "i18n.error.unknown_lang"
    }
}

impl Lang {
    /// All languages, reference language first.
    pub const ALL: [Lang; 3] = [Lang::Ar, Lang::En, Lang::De];

    /// Parses a language tag or POSIX locale name such as `ar`, `ar_SY.UTF-8`, `en-US` or `de_DE@euro`.
    ///
    /// Returns `None` for anything else (including `C` and `POSIX`).
    pub fn parse(text: &str) -> Option<Lang> {
        let cleaned = text.trim().trim_matches(|c| c == '"' || c == '\'');
        let end = cleaned
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(cleaned.len());
        let code = &cleaned[..end];
        const NAMES: [(&str, Lang); 7] = [
            ("ar", Lang::Ar),
            ("ara", Lang::Ar),
            ("en", Lang::En),
            ("eng", Lang::En),
            ("de", Lang::De),
            ("deu", Lang::De),
            ("ger", Lang::De),
        ];
        NAMES
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(code))
            .map(|(_, lang)| *lang)
    }

    /// Two-letter ISO 639-1 code (`ar`, `en`, `de`).
    pub const fn code(self) -> &'static str {
        match self {
            Lang::Ar => "ar",
            Lang::En => "en",
            Lang::De => "de",
        }
    }

    /// Default POSIX locale name handed to child processes and compatibility layers.
    pub const fn locale(self) -> &'static str {
        match self {
            Lang::Ar => "ar_SY.UTF-8",
            Lang::En => "en_US.UTF-8",
            Lang::De => "de_DE.UTF-8",
        }
    }

    /// The language's own name, for language pickers.
    pub const fn native_name(self) -> &'static str {
        match self {
            Lang::Ar => "العربية",
            Lang::En => "English",
            Lang::De => "Deutsch",
        }
    }

    /// Base direction of the language.
    pub const fn direction(self) -> Direction {
        match self {
            Lang::Ar => Direction::Rtl,
            Lang::En | Lang::De => Direction::Ltr,
        }
    }

    /// Stable index (0 = Arabic, 1 = English, 2 = German) for per-language arrays.
    pub const fn index(self) -> usize {
        match self {
            Lang::Ar => 0,
            Lang::En => 1,
            Lang::De => 2,
        }
    }

    /// Returns the first value that names a supported language.
    ///
    /// Callers pass candidate settings in priority order (for example `ULTAR_LANG`, `LC_ALL`,
    /// `LC_MESSAGES`, `LANG`, then the stored system setting).
    pub fn from_candidates<'a, I>(candidates: I) -> Option<Lang>
    where
        I: IntoIterator<Item = &'a str>,
    {
        candidates.into_iter().find_map(Lang::parse)
    }

    /// Detects the language from `ULTAR_LANG`, `LC_ALL`, `LC_MESSAGES` and `LANG` (in this order).
    ///
    /// Falls back to Arabic, the reference language.
    #[cfg(feature = "std")]
    pub fn from_env() -> Lang {
        ["ULTAR_LANG", "LC_ALL", "LC_MESSAGES", "LANG"]
            .iter()
            .filter_map(|name| std::env::var(name).ok())
            .find_map(|value| Lang::parse(&value))
            .unwrap_or_default()
    }
}

impl FromStr for Lang {
    type Err = UnknownLangError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        Lang::parse(text).ok_or(UnknownLangError)
    }
}

impl fmt::Display for Lang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

/// Base direction of a language (free-function form of [`Lang::direction`]).
pub const fn direction(lang: Lang) -> Direction {
    lang.direction()
}

impl Direction {
    /// `true` for right-to-left.
    pub const fn is_rtl(self) -> bool {
        matches!(self, Direction::Rtl)
    }

    /// The opposite direction.
    pub const fn opposite(self) -> Direction {
        match self {
            Direction::Rtl => Direction::Ltr,
            Direction::Ltr => Direction::Rtl,
        }
    }

    /// Value of the HTML/CSS `dir` attribute (`rtl` or `ltr`), used by the web administration.
    pub const fn html_dir(self) -> &'static str {
        match self {
            Direction::Rtl => "rtl",
            Direction::Ltr => "ltr",
        }
    }
}

impl Side {
    /// Resolves the logical side to a physical side for the given direction.
    pub const fn resolve(self, direction: Direction) -> PhysicalSide {
        match (self, direction) {
            (Side::Start, Direction::Ltr) | (Side::End, Direction::Rtl) => PhysicalSide::Left,
            (Side::Start, Direction::Rtl) | (Side::End, Direction::Ltr) => PhysicalSide::Right,
        }
    }

    /// Resolves the logical side for the base direction of a language.
    pub const fn resolve_for(self, lang: Lang) -> PhysicalSide {
        self.resolve(lang.direction())
    }

    /// The opposite logical side.
    pub const fn opposite(self) -> Side {
        match self {
            Side::Start => Side::End,
            Side::End => Side::Start,
        }
    }
}

impl PhysicalSide {
    /// Converts a physical side back to the logical side for the given direction.
    pub const fn to_logical(self, direction: Direction) -> Side {
        match (self, direction) {
            (PhysicalSide::Left, Direction::Ltr) | (PhysicalSide::Right, Direction::Rtl) => {
                Side::Start
            }
            (PhysicalSide::Left, Direction::Rtl) | (PhysicalSide::Right, Direction::Ltr) => {
                Side::End
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_locales() {
        assert_eq!(Lang::parse("ar"), Some(Lang::Ar));
        assert_eq!(Lang::parse("ar_SY.UTF-8"), Some(Lang::Ar));
        assert_eq!(Lang::parse("AR-sy"), Some(Lang::Ar));
        assert_eq!(Lang::parse(" \"en\" "), Some(Lang::En));
        assert_eq!(Lang::parse("en_US"), Some(Lang::En));
        assert_eq!(Lang::parse("de_DE@euro"), Some(Lang::De));
        assert_eq!(Lang::parse("deu"), Some(Lang::De));
        assert_eq!(Lang::parse("C"), None);
        assert_eq!(Lang::parse("POSIX"), None);
        assert_eq!(Lang::parse(""), None);
        assert_eq!(Lang::parse("arabic"), None);
        assert_eq!(Lang::parse("fr_FR.UTF-8"), None);
    }

    #[test]
    fn from_str_and_display() {
        assert_eq!("de".parse::<Lang>(), Ok(Lang::De));
        assert_eq!("xx".parse::<Lang>(), Err(UnknownLangError));
        assert_eq!(UnknownLangError.message_key(), "i18n.error.unknown_lang");
        assert_eq!(alloc::format!("{}", Lang::Ar), "ar");
        assert_eq!(alloc::format!("{UnknownLangError}"), "unknown language");
    }

    #[test]
    fn codes_names_and_indices() {
        assert_eq!(Lang::default(), Lang::Ar);
        assert_eq!(Lang::ALL[0], Lang::Ar);
        for (i, lang) in Lang::ALL.iter().enumerate() {
            assert_eq!(lang.index(), i);
            assert_eq!(Lang::parse(lang.code()), Some(*lang));
            assert_eq!(Lang::parse(lang.locale()), Some(*lang));
            assert!(!lang.native_name().is_empty());
        }
    }

    #[test]
    fn candidates_in_priority_order() {
        assert_eq!(
            Lang::from_candidates(["C", "", "de_DE", "en"]),
            Some(Lang::De)
        );
        assert_eq!(Lang::from_candidates(["C", "POSIX"]), None);
        assert_eq!(Lang::from_candidates([]), None);
    }

    #[test]
    fn directions() {
        assert_eq!(direction(Lang::Ar), Direction::Rtl);
        assert_eq!(direction(Lang::En), Direction::Ltr);
        assert_eq!(direction(Lang::De), Direction::Ltr);
        assert!(Direction::Rtl.is_rtl());
        assert!(!Direction::Ltr.is_rtl());
        assert_eq!(Direction::Rtl.opposite(), Direction::Ltr);
        assert_eq!(Direction::Ltr.opposite(), Direction::Rtl);
        assert_eq!(Direction::Rtl.html_dir(), "rtl");
        assert_eq!(Direction::Ltr.html_dir(), "ltr");
    }

    #[test]
    fn logical_sides_resolve() {
        assert_eq!(Side::Start.resolve(Direction::Ltr), PhysicalSide::Left);
        assert_eq!(Side::Start.resolve(Direction::Rtl), PhysicalSide::Right);
        assert_eq!(Side::End.resolve(Direction::Ltr), PhysicalSide::Right);
        assert_eq!(Side::End.resolve(Direction::Rtl), PhysicalSide::Left);
        assert_eq!(Side::Start.resolve_for(Lang::Ar), PhysicalSide::Right);
        assert_eq!(Side::Start.resolve_for(Lang::De), PhysicalSide::Left);
        assert_eq!(Side::Start.opposite(), Side::End);
        assert_eq!(Side::End.opposite(), Side::Start);
        for dir in [Direction::Ltr, Direction::Rtl] {
            for side in [Side::Start, Side::End] {
                assert_eq!(side.resolve(dir).to_logical(dir), side);
            }
        }
    }
}
