//! Catalogs of the installer (`locales/*.ftl`, Arabic first) and service texts → labels.

use ultar_bus::{Text, TextArg};
use ultar_i18n::{Lang, Translator};
use ultar_ui::widgets::Label;

/// The installer's catalogs.
pub const CATALOGS: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

const POSITIONAL: [&str; 6] = ["a", "b", "c", "d", "e", "f"];

/// A translator with the installer's catalogs.
pub fn translator(lang: Lang) -> Translator {
    let mut tr = Translator::new(lang);
    for (l, source) in CATALOGS {
        let _ = tr.add_source(l, source);
    }
    tr.set_lang(lang);
    tr
}

/// A service text as a label (`{a}`, `{b}` … for positional arguments).
pub fn label(text: &Text, tr: &Translator) -> Label {
    let values: Vec<String> = text
        .args
        .iter()
        .map(|a| match a {
            TextArg::Verbatim(v) => v.clone(),
            TextArg::Number(n) => n.to_string(),
            TextArg::Key(k) => tr.t(k).into_owned(),
        })
        .collect();
    let args: Vec<(&str, &str)> = POSITIONAL
        .iter()
        .zip(&values)
        .map(|(n, v)| (*n, v.as_str()))
        .collect();
    Label::key_with(text.key.clone(), &args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_have_the_same_keys() {
        let keys: Vec<Vec<String>> = CATALOGS
            .iter()
            .map(|(_, s)| {
                let c = ultar_i18n::Catalog::parse(s).unwrap();
                let mut k: Vec<String> = c.keys().map(str::to_owned).collect();
                k.sort();
                k
            })
            .collect();
        assert_eq!(keys[0], keys[1]);
        assert_eq!(keys[0], keys[2]);
    }

    #[test]
    fn positional_arguments() {
        let tr = translator(Lang::En);
        let t = Text::key("x").with(TextArg::Number(2));
        assert!(matches!(label(&t, &tr), Label::Key { args, .. } if args[0].0 == "a"));
    }
}
