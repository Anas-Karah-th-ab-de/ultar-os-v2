//! The translator: looks up messages with the fallback chain *requested language → Arabic → key*.

use alloc::borrow::Cow;
use alloc::string::String;

use crate::catalog::Catalog;
use crate::embedded;
use crate::error::ParseError;
use crate::lang::{Direction, Lang, PhysicalSide, Side};
use crate::pattern::{Arg, Message};

/// Translates message keys into text of the current language.
///
/// A translator holds one catalog per language. [`Translator::new`] starts with the embedded base
/// catalogs; apps and services add their own with [`Translator::add_namespaced_source`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Translator {
    lang: Lang,
    catalogs: [Catalog; 3],
}

impl Translator {
    /// Creates a translator for `lang` with the embedded base catalogs of all languages.
    pub fn new(lang: Lang) -> Translator {
        let mut translator = Translator::empty(lang);
        for each in Lang::ALL {
            // The embedded catalogs are verified by the crate's tests; a failure leaves the
            // language empty and the fallback chain still yields Arabic or the key.
            translator.catalogs[each.index()] = embedded::catalog(each).unwrap_or_default();
        }
        translator
    }

    /// Creates a translator without any messages (for tests and special tools).
    pub fn empty(lang: Lang) -> Translator {
        Translator {
            lang,
            catalogs: Default::default(),
        }
    }

    /// The current language.
    pub const fn lang(&self) -> Lang {
        self.lang
    }

    /// Switches the current language; catalogs stay loaded, so this is cheap at run time.
    pub fn set_lang(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// Base direction of the current language.
    pub const fn direction(&self) -> Direction {
        self.lang.direction()
    }

    /// Resolves a logical layout side for the current language.
    pub const fn side(&self, side: Side) -> PhysicalSide {
        side.resolve(self.lang.direction())
    }

    /// The catalog of a language.
    pub fn catalog(&self, lang: Lang) -> &Catalog {
        &self.catalogs[lang.index()]
    }

    /// Merges a catalog into the messages of `lang`; existing keys are replaced.
    pub fn add_catalog(&mut self, lang: Lang, catalog: Catalog) {
        self.catalogs[lang.index()].merge(catalog);
    }

    /// Parses `source` and merges it into the messages of `lang`.
    pub fn add_source(&mut self, lang: Lang, source: &str) -> Result<(), ParseError> {
        self.add_catalog(lang, Catalog::parse(source)?);
        Ok(())
    }

    /// Parses an app catalog and merges it under `namespace` (`title` → `<namespace>.title`).
    pub fn add_namespaced_source(
        &mut self,
        lang: Lang,
        namespace: &str,
        source: &str,
    ) -> Result<(), ParseError> {
        self.add_catalog(lang, Catalog::parse_namespaced(namespace, source)?);
        Ok(())
    }

    /// Finds the message for `key`: first in the current language, then in Arabic.
    ///
    /// Returns the language in which it was found, so plural rules match the text.
    pub fn lookup(&self, key: &str) -> Option<(Lang, &Message)> {
        [self.lang, Lang::Ar]
            .into_iter()
            .find_map(|lang| self.catalog(lang).get(key).map(|message| (lang, message)))
    }

    /// `true` if `key` can be translated (in the current language or in Arabic).
    pub fn has(&self, key: &str) -> bool {
        self.lookup(key).is_some()
    }

    /// Translates `key`. Falls back to Arabic, then to the key itself.
    ///
    /// Messages without placeholders are returned without allocating.
    pub fn t<'a>(&'a self, key: &'a str) -> Cow<'a, str> {
        match self.lookup(key) {
            None => Cow::Borrowed(key),
            Some((lang, message)) => match message.as_static_text() {
                Some(text) => Cow::Borrowed(text),
                None => Cow::Owned(message.format(lang, &[])),
            },
        }
    }

    /// Translates `key` and fills in the arguments. Falls back to Arabic, then to the key itself.
    ///
    /// Numbers select the plural form of the language in which the message was found.
    pub fn t_args(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        match self.lookup(key) {
            None => String::from(key),
            Some((lang, message)) => message.format(lang, args),
        }
    }

    /// Like [`Translator::t`], but `None` if the key is unknown everywhere.
    pub fn try_t<'a>(&'a self, key: &'a str) -> Option<Cow<'a, str>> {
        self.has(key).then(|| self.t(key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Translator {
        let mut tr = Translator::empty(Lang::De);
        tr.add_source(
            Lang::Ar,
            "a = ألف\nonly_ar = عربي فقط\nn = {n -> [two] اثنان *[other] {n}}\n",
        )
        .unwrap();
        tr.add_source(Lang::De, "a = A-de\nhi = Hallo {name}\n")
            .unwrap();
        tr.add_source(Lang::En, "a = A-en\nonly_en = English only\n")
            .unwrap();
        tr
    }

    #[test]
    fn fallback_chain_is_requested_then_arabic_then_key() {
        let tr = sample();
        assert_eq!(tr.t("a"), "A-de");
        assert_eq!(tr.t("only_ar"), "عربي فقط");
        assert_eq!(tr.t("only_en"), "only_en");
        assert_eq!(tr.t("missing.key"), "missing.key");
        assert_eq!(
            tr.t_args("missing.key", &[("x", Arg::Int(1))]),
            "missing.key"
        );
        assert!(tr.has("only_ar"));
        assert!(!tr.has("only_en"));
        assert_eq!(tr.try_t("only_en"), None);
        assert_eq!(tr.try_t("a").as_deref(), Some("A-de"));
    }

    #[test]
    fn fallback_uses_plural_rules_of_the_found_language() {
        let tr = sample();
        assert_eq!(tr.lookup("n").map(|(lang, _)| lang), Some(Lang::Ar));
        assert_eq!(tr.t_args("n", &[("n", Arg::Int(2))]), "اثنان");
        assert_eq!(tr.t_args("n", &[("n", Arg::Int(7))]), "7");
    }

    #[test]
    fn static_messages_do_not_allocate() {
        let tr = sample();
        assert!(matches!(tr.t("a"), Cow::Borrowed(_)));
        assert!(matches!(tr.t("nope"), Cow::Borrowed(_)));
        assert!(matches!(tr.t("hi"), Cow::Owned(_)));
        assert_eq!(tr.t("hi"), "Hallo {name}");
        assert_eq!(tr.t_args("hi", &[("name", Arg::Str("Anas"))]), "Hallo Anas");
    }

    #[test]
    fn language_switch_and_direction() {
        let mut tr = sample();
        assert_eq!(tr.lang(), Lang::De);
        assert_eq!(tr.direction(), Direction::Ltr);
        assert_eq!(tr.side(Side::Start), PhysicalSide::Left);
        tr.set_lang(Lang::Ar);
        assert_eq!(tr.t("a"), "ألف");
        assert_eq!(tr.direction(), Direction::Rtl);
        assert_eq!(tr.side(Side::Start), PhysicalSide::Right);
        tr.set_lang(Lang::En);
        assert_eq!(tr.t("a"), "A-en");
    }

    #[test]
    fn app_catalogs_are_namespaced_and_override() {
        let mut tr = Translator::new(Lang::En);
        tr.add_namespaced_source(Lang::En, "apps.notes", "title = Notes\n")
            .unwrap();
        tr.add_namespaced_source(
            Lang::Ar,
            "apps.notes",
            "title = الملاحظات\nempty = لا ملاحظات\n",
        )
        .unwrap();
        assert_eq!(tr.t("apps.notes.title"), "Notes");
        assert_eq!(tr.t("apps.notes.empty"), "لا ملاحظات");
        assert_eq!(tr.t("action.cancel"), "Cancel");
        tr.add_source(Lang::En, "action.cancel = Abort\n").unwrap();
        assert_eq!(tr.t("action.cancel"), "Abort");
        assert!(tr.add_source(Lang::En, "broken").is_err());
        assert!(tr
            .add_namespaced_source(Lang::En, "bad ns", "a = b")
            .is_err());
        assert!(tr.catalog(Lang::Ar).contains("apps.notes.empty"));
    }

    #[test]
    fn default_translator_is_arabic_and_empty() {
        let tr = Translator::default();
        assert_eq!(tr.lang(), Lang::Ar);
        assert!(tr.catalog(Lang::Ar).is_empty());
        assert_eq!(Translator::empty(Lang::Ar), tr);
    }
}
