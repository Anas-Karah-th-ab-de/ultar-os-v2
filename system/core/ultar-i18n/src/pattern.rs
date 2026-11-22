//! Parsed messages: text, placeholders and selections, and how they are rendered.

use alloc::collections::BTreeSet;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::fmt::Write;

use crate::lang::Lang;
use crate::numbers;
use crate::plural::{self, PluralCategory};

/// A value passed to a message placeholder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg<'a> {
    /// Text, inserted as it is. In selections it is matched against named variants.
    Str(&'a str),
    /// A whole number, written with Western digits and without grouping. Drives plural selection.
    Int(i64),
    /// A whole number written with the language's digit grouping (`1,234,567`). Drives plural selection.
    Grouped(i64),
}

impl<'a> From<&'a str> for Arg<'a> {
    fn from(value: &'a str) -> Self {
        Arg::Str(value)
    }
}

macro_rules! arg_from_int {
    ($($ty:ty),*) => {$(
        impl From<$ty> for Arg<'_> {
            fn from(value: $ty) -> Self {
                Arg::Int(i64::from(value))
            }
        }
    )*};
}
arg_from_int!(i8, i16, i32, i64, u8, u16, u32);

impl Arg<'_> {
    /// The numeric value, if the argument is a number.
    pub const fn as_int(&self) -> Option<i64> {
        match self {
            Arg::Int(n) | Arg::Grouped(n) => Some(*n),
            Arg::Str(_) => None,
        }
    }

    /// Appends the argument to `out` as it appears in a message of language `lang`.
    pub fn write_to(&self, lang: Lang, out: &mut String) {
        match self {
            Arg::Str(text) => out.push_str(text),
            Arg::Int(n) => {
                // Writing to a String cannot fail.
                let _ = write!(out, "{n}");
            }
            Arg::Grouped(n) => out.push_str(&numbers::format_int(*n, lang)),
        }
    }
}

/// The key of one variant of a selection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VariantKey {
    /// A plural category such as `[few]`.
    Plural(PluralCategory),
    /// An exact number such as `[0]`; wins over plural categories.
    Number(i64),
    /// A name such as `[wifi]`, matched against text arguments.
    Name(String),
}

impl VariantKey {
    /// Interprets the text between `[` and `]`.
    pub fn from_marker(text: &str) -> VariantKey {
        if let Some(category) = PluralCategory::from_keyword(text) {
            VariantKey::Plural(category)
        } else if let Ok(number) = text.parse::<i64>() {
            VariantKey::Number(number)
        } else {
            VariantKey::Name(text.to_string())
        }
    }
}

/// One variant of a selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Variant {
    /// The key that selects this variant.
    pub key: VariantKey,
    /// The text of this variant.
    pub pattern: Vec<Part>,
}

/// A selection `{name -> [key] text … *[key] text}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Select {
    /// The argument that drives the selection.
    pub variable: String,
    /// All variants in source order.
    pub variants: Vec<Variant>,
    /// Index of the default variant (marked with `*`).
    pub default: usize,
}

/// One piece of a message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    /// Literal text.
    Text(String),
    /// A placeholder `{name}`.
    Placeholder(String),
    /// A selection.
    Select(Select),
}

/// A parsed message of a catalog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    parts: Vec<Part>,
}

impl Message {
    /// Wraps already parsed parts.
    pub fn from_parts(parts: Vec<Part>) -> Message {
        Message { parts }
    }

    /// The parts of the message.
    pub fn parts(&self) -> &[Part] {
        &self.parts
    }

    /// The text of the message if it has neither placeholders nor selections.
    pub fn as_static_text(&self) -> Option<&str> {
        match self.parts.as_slice() {
            [] => Some(""),
            [Part::Text(text)] => Some(text),
            _ => None,
        }
    }

    /// Names of all arguments the message uses (placeholders and selection variables), sorted.
    pub fn placeholders(&self) -> BTreeSet<&str> {
        let mut names = BTreeSet::new();
        collect_placeholders(&self.parts, &mut names);
        names
    }

    /// Plural categories that appear as variant keys anywhere in the message.
    pub fn plural_categories(&self) -> BTreeSet<PluralCategory> {
        let mut found = BTreeSet::new();
        collect_plural_categories(&self.parts, &mut found);
        found
    }

    /// Renders the message for `lang` with `args` and appends it to `out`.
    ///
    /// A placeholder without a matching argument is written as `{name}` so the gap stays visible.
    pub fn format_into(&self, lang: Lang, args: &[(&str, Arg<'_>)], out: &mut String) {
        format_parts(&self.parts, lang, args, out);
    }

    /// Renders the message for `lang` with `args`.
    pub fn format(&self, lang: Lang, args: &[(&str, Arg<'_>)]) -> String {
        let mut out = String::new();
        self.format_into(lang, args, &mut out);
        out
    }
}

fn collect_placeholders<'a>(parts: &'a [Part], names: &mut BTreeSet<&'a str>) {
    for part in parts {
        match part {
            Part::Text(_) => {}
            Part::Placeholder(name) => {
                names.insert(name.as_str());
            }
            Part::Select(select) => {
                names.insert(select.variable.as_str());
                for variant in &select.variants {
                    collect_placeholders(&variant.pattern, names);
                }
            }
        }
    }
}

fn collect_plural_categories(parts: &[Part], found: &mut BTreeSet<PluralCategory>) {
    for part in parts {
        if let Part::Select(select) = part {
            for variant in &select.variants {
                if let VariantKey::Plural(category) = variant.key {
                    found.insert(category);
                }
                collect_plural_categories(&variant.pattern, found);
            }
        }
    }
}

fn find_arg<'a, 'b>(args: &'b [(&str, Arg<'a>)], name: &str) -> Option<&'b Arg<'a>> {
    args.iter()
        .find(|(key, _)| *key == name)
        .map(|(_, arg)| arg)
}

fn format_parts(parts: &[Part], lang: Lang, args: &[(&str, Arg<'_>)], out: &mut String) {
    for part in parts {
        match part {
            Part::Text(text) => out.push_str(text),
            Part::Placeholder(name) => match find_arg(args, name) {
                Some(arg) => arg.write_to(lang, out),
                None => {
                    out.push('{');
                    out.push_str(name);
                    out.push('}');
                }
            },
            Part::Select(select) => {
                if let Some(variant) =
                    choose_variant(select, lang, find_arg(args, &select.variable))
                {
                    format_parts(&variant.pattern, lang, args, out);
                }
            }
        }
    }
}

fn choose_variant<'s>(
    select: &'s Select,
    lang: Lang,
    arg: Option<&Arg<'_>>,
) -> Option<&'s Variant> {
    let by_key = |wanted: &VariantKey| select.variants.iter().find(|v| &v.key == wanted);
    let chosen = match arg {
        Some(Arg::Str(text)) => by_key(&VariantKey::from_marker(text)),
        Some(number) => number.as_int().and_then(|n| {
            by_key(&VariantKey::Number(n))
                .or_else(|| by_key(&VariantKey::Plural(plural::category(lang, n))))
        }),
        None => None,
    };
    chosen.or_else(|| select.variants.get(select.default))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn message(source: &str) -> Message {
        let catalog = Catalog::parse(&alloc::format!("m = {source}")).unwrap();
        catalog.get("m").unwrap().clone()
    }

    #[test]
    fn arg_conversions_and_rendering() {
        assert_eq!(Arg::from("x"), Arg::Str("x"));
        assert_eq!(Arg::from(5u8), Arg::Int(5));
        assert_eq!(Arg::from(-5i32), Arg::Int(-5));
        assert_eq!(Arg::from(5i64), Arg::Int(5));
        assert_eq!(Arg::Str("x").as_int(), None);
        assert_eq!(Arg::Grouped(7).as_int(), Some(7));
        let mut out = String::new();
        Arg::Int(-1234567).write_to(Lang::Ar, &mut out);
        out.push(' ');
        Arg::Grouped(1234567).write_to(Lang::De, &mut out);
        out.push(' ');
        Arg::Str("نص").write_to(Lang::Ar, &mut out);
        assert_eq!(out, "-1234567 1.234.567 نص");
    }

    #[test]
    fn variant_keys_from_markers() {
        assert_eq!(
            VariantKey::from_marker("few"),
            VariantKey::Plural(PluralCategory::Few)
        );
        assert_eq!(VariantKey::from_marker("0"), VariantKey::Number(0));
        assert_eq!(VariantKey::from_marker("-3"), VariantKey::Number(-3));
        assert_eq!(
            VariantKey::from_marker("wifi"),
            VariantKey::Name("wifi".to_string())
        );
    }

    #[test]
    fn static_text_and_placeholders() {
        let plain = message("Hello");
        assert_eq!(plain.as_static_text(), Some("Hello"));
        assert!(plain.placeholders().is_empty());
        assert_eq!(Message::from_parts(Vec::new()).as_static_text(), Some(""));
        let with = message("{b} and {a} and {n -> [one] {c} *[other] x}");
        assert_eq!(with.as_static_text(), None);
        assert_eq!(
            with.placeholders().into_iter().collect::<Vec<_>>(),
            ["a", "b", "c", "n"]
        );
        assert_eq!(with.parts().len(), 5);
    }

    #[test]
    fn missing_argument_stays_visible() {
        assert_eq!(message("Hi {name}!").format(Lang::En, &[]), "Hi {name}!");
        assert_eq!(
            message("Hi {name}!").format(Lang::En, &[("name", Arg::Str("A"))]),
            "Hi A!"
        );
    }

    #[test]
    fn plural_selection_per_language() {
        let m = message("{n -> [zero] Z [one] O [two] T [few] F [many] M *[other] X{n}}");
        let render = |lang, n| m.format(lang, &[("n", Arg::Int(n))]);
        assert_eq!(render(Lang::Ar, 0), "Z");
        assert_eq!(render(Lang::Ar, 1), "O");
        assert_eq!(render(Lang::Ar, 2), "T");
        assert_eq!(render(Lang::Ar, 5), "F");
        assert_eq!(render(Lang::Ar, 15), "M");
        assert_eq!(render(Lang::Ar, 100), "X100");
        assert_eq!(render(Lang::En, 0), "X0");
        assert_eq!(render(Lang::En, 1), "O");
        assert_eq!(render(Lang::De, 2), "X2");
        assert_eq!(m.plural_categories().len(), 6);
    }

    #[test]
    fn exact_number_wins_and_names_match_text() {
        let m = message("{n -> [0] none [one] single *[other] {n} things}");
        assert_eq!(m.format(Lang::En, &[("n", Arg::Int(0))]), "none");
        assert_eq!(m.format(Lang::En, &[("n", Arg::Int(1))]), "single");
        assert_eq!(
            m.format(Lang::En, &[("n", Arg::Grouped(12345))]),
            "12,345 things"
        );
        let kind = message("{kind -> [wifi] Wi-Fi [lan] LAN *[other] ?}");
        assert_eq!(kind.format(Lang::En, &[("kind", Arg::Str("lan"))]), "LAN");
        assert_eq!(kind.format(Lang::En, &[("kind", Arg::Str("zzz"))]), "?");
        assert_eq!(kind.format(Lang::En, &[]), "?");
        assert_eq!(kind.format(Lang::En, &[("kind", Arg::Int(3))]), "?");
    }
}
