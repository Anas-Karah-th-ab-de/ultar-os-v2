//! Labels: every user-visible text is a catalog key; values that are data (resolutions,
//! device names, numbers with units) are isolated so they keep their own direction.

use crate::text::isolate;
use std::borrow::Cow;
use ultar_i18n::{Arg, Translator};

/// An argument of a catalog message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LabelArg {
    /// Text; it is direction-isolated when inserted.
    Text(String),
    /// A whole number (drives plural selection).
    Int(i64),
}

/// A text shown by a widget.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Label {
    /// A catalog key with optional arguments.
    Key {
        /// The i18n key.
        key: String,
        /// Named arguments of the message.
        args: Vec<(String, LabelArg)>,
    },
    /// A value that is not translated (already stored direction-isolated).
    Data(String),
}

impl Label {
    /// A label from a catalog key.
    pub fn key(key: impl Into<String>) -> Label {
        Label::Key {
            key: key.into(),
            args: Vec::new(),
        }
    }

    /// A label from a catalog key with text arguments.
    pub fn key_with(key: impl Into<String>, args: &[(&str, &str)]) -> Label {
        Label::Key {
            key: key.into(),
            args: args
                .iter()
                .map(|(name, value)| ((*name).to_string(), LabelArg::Text(isolate(value))))
                .collect(),
        }
    }

    /// A label from a catalog key with one number argument.
    pub fn key_count(key: impl Into<String>, name: &str, count: i64) -> Label {
        Label::Key {
            key: key.into(),
            args: vec![(name.to_string(), LabelArg::Int(count))],
        }
    }

    /// A label showing a data value such as `3840 × 2160` or a device name.
    pub fn data(value: &str) -> Label {
        Label::Data(if value.is_empty() {
            String::new()
        } else {
            isolate(value)
        })
    }

    /// The catalog key, if the label has one.
    pub fn key_name(&self) -> Option<&str> {
        match self {
            Label::Key { key, .. } => Some(key),
            Label::Data(_) => None,
        }
    }

    /// The text in the translator's language.
    pub fn resolve<'a>(&'a self, tr: &'a Translator) -> Cow<'a, str> {
        match self {
            Label::Data(value) => Cow::Borrowed(value),
            Label::Key { key, args } if args.is_empty() => tr.t(key),
            Label::Key { key, args } => {
                let list: Vec<(&str, Arg<'_>)> = args
                    .iter()
                    .map(|(name, value)| {
                        let arg = match value {
                            LabelArg::Text(text) => Arg::Str(text),
                            LabelArg::Int(n) => Arg::Int(*n),
                        };
                        (name.as_str(), arg)
                    })
                    .collect();
                Cow::Owned(tr.t_args(key, &list))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::Lang;

    #[test]
    fn labels_resolve_through_the_catalog() {
        let mut tr = Translator::empty(Lang::Ar);
        tr.add_source(
            Lang::Ar,
            "a = مرحبا\nb = الدقة {value}\nc = {n ->\n    [one] ملف\n   *[other] {n} ملفات\n}\n",
        )
        .unwrap();
        tr.add_source(Lang::En, "a = Hello\n").unwrap();
        assert_eq!(Label::key("a").resolve(&tr), "مرحبا");
        assert_eq!(Label::key("a").key_name(), Some("a"));
        let with = Label::key_with("b", &[("value", "60 Hz")])
            .resolve(&tr)
            .into_owned();
        assert_eq!(with, "الدقة \u{2068}60 Hz\u{2069}");
        assert_eq!(Label::key_count("c", "n", 5).resolve(&tr), "5 ملفات");
        assert_eq!(
            Label::data("Dell 24").resolve(&tr),
            "\u{2068}Dell 24\u{2069}"
        );
        assert_eq!(Label::data("").resolve(&tr), "");
        assert_eq!(Label::data("x").key_name(), None);
        assert_eq!(Label::key("missing.key").resolve(&tr), "missing.key");
        tr.set_lang(Lang::En);
        assert_eq!(Label::key("a").resolve(&tr), "Hello");
    }
}
