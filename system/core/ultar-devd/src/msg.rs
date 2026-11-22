//! User-visible text of `ultar-devd` and `udevctl`: catalogues in `locales/` (ultar-i18n
//! format, Arabic first) and rendering of bus texts.

use ultar_bus::{Text, TextArg};
use ultar_i18n::{Arg, Lang, ParseError, Translator};

const SOURCES: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// Names of the placeholders that receive the positional arguments of a bus text.
pub const POSITIONAL: [&str; 4] = ["a", "b", "c", "d"];

/// The message catalogue in one language.
pub struct Messages {
    translator: Translator,
    lang: Lang,
}

impl Messages {
    /// Loads the embedded catalogues.
    pub fn load(lang: Lang) -> Result<Self, ParseError> {
        let mut translator = Translator::empty(lang);
        for (source_lang, source) in SOURCES {
            translator.add_source(source_lang, source)?;
        }
        Ok(Self { translator, lang })
    }

    /// The catalogues for `lang`; should an embedded catalogue ever fail to parse (a test
    /// guards against it) the keys themselves are shown.
    pub fn new(lang: Lang) -> Self {
        Self::load(lang).unwrap_or_else(|_| Self {
            translator: Translator::empty(lang),
            lang,
        })
    }

    /// The language given on the command line, else the one of the environment
    /// (`ULTAR_LANG`, `LC_ALL`, `LC_MESSAGES`, `LANG`; default Arabic).
    pub fn choose_lang(option: Option<&str>) -> Lang {
        option.and_then(Lang::parse).unwrap_or_else(Lang::from_env)
    }

    /// The language in use.
    pub fn lang(&self) -> Lang {
        self.lang
    }

    /// The text of a key.
    pub fn t(&self, key: &str) -> String {
        self.translator.t(key).into_owned()
    }

    /// The text of a key with named arguments.
    pub fn t_args(&self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        self.translator.t_args(key, args)
    }

    /// The text of a key that may also belong to the driver package tool (`driverpkg.*`).
    pub fn any_key(&self, key: &str) -> String {
        if self.translator.has(key) {
            return self.t(key);
        }
        let lang = ultar_driverpkg::msg::Lang::from_tag(Some(self.lang.code()));
        ultar_driverpkg::msg::text(lang, key).map_or_else(|| key.to_string(), str::to_string)
    }

    /// Renders a bus text: its positional arguments fill `{a}`, `{b}`, …
    pub fn render(&self, text: &Text) -> String {
        let values: Vec<String> = text
            .args
            .iter()
            .map(|arg| match arg {
                TextArg::Verbatim(value) => value.clone(),
                TextArg::Number(number) => number.to_string(),
                TextArg::Key(key) => self.any_key(key),
            })
            .collect();
        let args: Vec<(&str, Arg<'_>)> = POSITIONAL
            .iter()
            .zip(&values)
            .map(|(name, value)| (*name, Arg::Str(value)))
            .collect();
        if self.translator.has(&text.key) {
            self.translator.t_args(&text.key, &args)
        } else {
            self.any_key(&text.key)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_i18n::check::compare_catalogs;
    use ultar_i18n::Catalog;

    #[test]
    fn catalogues_parse_and_agree() {
        let reference = Catalog::parse(SOURCES[0].1).unwrap();
        assert!(reference.len() > 80);
        for (lang, source) in &SOURCES[1..] {
            let other = Catalog::parse(source).unwrap();
            let comparison = compare_catalogs(&reference, &other);
            assert!(
                comparison.missing.is_empty(),
                "{lang:?}: {:?}",
                comparison.missing
            );
            assert!(
                comparison.extra.is_empty(),
                "{lang:?}: {:?}",
                comparison.extra
            );
            assert!(comparison.placeholders.is_empty(), "{lang:?}");
        }
        for lang in Lang::ALL {
            assert!(Messages::load(lang).is_ok());
        }
    }

    #[test]
    fn every_code_of_the_daemon_has_a_text() {
        use crate::types::{HealthFailure, LoadFailure};
        let messages = Messages::new(Lang::En);
        let mut keys: Vec<String> = Vec::new();
        for failure in LoadFailure::ALL {
            if *failure != LoadFailure::PackageMissing {
                keys.push(format!("devd.reason.load.{}", failure.code()));
            }
        }
        for failure in HealthFailure::ALL {
            keys.push(format!("devd.reason.health.{}", failure.code()));
        }
        for phase in [
            "detected",
            "probing",
            "active",
            "no-driver",
            "failed",
            "removed",
        ] {
            keys.push(format!("devd.phase.{phase}"));
        }
        for code in [
            "devd.error.unknown-device",
            "devd.error.not-a-candidate",
            "devd.error.quarantined",
            "devd.error.already-active",
            "devd.error.nothing-to-roll-back",
            "devd.error.wrong-phase",
            "devd.error.unsupported",
            "devd.error.transition",
            "devd.error.bad-request",
            "devd.store.unavailable",
            "devd.store.corrupt",
            "devd.store.write-failed",
            "devd.start.unknown-kernel",
            "devd.start.hardware",
            crate::bus::sink::NOTE_NO_DRIVER,
        ] {
            keys.push(code.to_string());
        }
        for key in keys {
            assert_ne!(messages.t(&key), key, "no text for {key}");
        }
    }

    #[test]
    fn renders_bus_texts_in_three_languages() {
        let text = Text::key("devd.error.unsupported")
            .with(TextArg::Verbatim("pci:01:00.0".into()))
            .with(TextArg::Verbatim("nvme-alt 1.0.0".into()));
        let english = Messages::new(Lang::En).render(&text);
        assert!(english.contains("pci:01:00.0") && english.contains("nvme-alt 1.0.0"));
        assert!(english.contains("built-in drivers"));
        let arabic = Messages::new(Lang::Ar).render(&text);
        assert!(arabic.contains("النواة") && arabic.contains("pci:01:00.0"));
        let german = Messages::new(Lang::De).render(&text);
        assert!(german.contains("eingebauten Treiber"));
        // A key argument is translated, also from the driver package table; unknown keys stay.
        let nested = Text::key("devd.notice.depot-unreadable")
            .with(TextArg::Key("driverpkg.index.unsigned".into()));
        let rendered = Messages::new(Lang::En).render(&nested);
        assert!(!rendered.contains("driverpkg.index.unsigned"), "{rendered}");
        let unknown = Text::key("driver.nvme.name");
        assert_eq!(Messages::new(Lang::En).render(&unknown), "driver.nvme.name");
        let number = Text::key("devd.cli.added").with(TextArg::Number(7));
        assert!(Messages::new(Lang::De).render(&number).ends_with('7'));
        assert_eq!(Messages::choose_lang(Some("de")), Lang::De);
        assert_eq!(Messages::new(Lang::De).lang(), Lang::De);
    }
}
