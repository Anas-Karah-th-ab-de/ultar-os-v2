//! Catalogs of the app (`locales/*.ftl`, Arabic first) and the conversion of service texts
//! (bus [`Text`], positional arguments) into toolkit labels.

use std::path::Path;
use ultar_bus::{Text, TextArg};
use ultar_i18n::{Lang, Translator};
use ultar_ui::widgets::Label;

/// The app's own catalogs.
pub const CATALOGS: [(Lang, &str); 3] = [
    (Lang::Ar, include_str!("../locales/ar.ftl")),
    (Lang::En, include_str!("../locales/en.ftl")),
    (Lang::De, include_str!("../locales/de.ftl")),
];

/// Names of positional arguments of service texts (`{a}`, `{b}`, …).
const POSITIONAL: [&str; 6] = ["a", "b", "c", "d", "e", "f"];
/// Most catalog files read from the system catalog folder.
const MAX_EXTRA_FILES: usize = 64;
/// Largest catalog file read.
const MAX_EXTRA_BYTES: u64 = 1 << 20;

/// A translator with the app's catalogs and, when given, the catalogs of the services
/// (`<dir>/<service>/<lang>.ftl` – settingsd, devd, updater) installed on the system.
pub fn translator(lang: Lang, services: Option<&Path>) -> Translator {
    let mut tr = Translator::new(lang);
    for (l, source) in CATALOGS {
        // The embedded catalogs are checked by the tests; a broken one leaves keys visible.
        let _ = tr.add_source(l, source);
    }
    if let Some(dir) = services {
        add_service_catalogs(&mut tr, dir);
    }
    tr.set_lang(lang);
    tr
}

/// Reads `<dir>/*/<lang>.ftl` for every language; unreadable or broken files are skipped
/// (their keys then show as keys, nothing else breaks).
pub fn add_service_catalogs(tr: &mut Translator, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten().take(MAX_EXTRA_FILES) {
        for lang in Lang::ALL {
            let file = entry.path().join(format!("{}.ftl", lang.code()));
            let small = std::fs::metadata(&file).is_ok_and(|m| m.len() <= MAX_EXTRA_BYTES);
            if !small {
                continue;
            }
            if let Ok(source) = std::fs::read_to_string(&file) {
                let _ = tr.add_source(lang, &source);
            }
        }
    }
}

/// A service text as a toolkit label: positional arguments become `{a}`, `{b}`, …; an
/// argument that is itself a key is translated first.
pub fn label(text: &Text, tr: &Translator) -> Label {
    let values: Vec<String> = text
        .args
        .iter()
        .map(|arg| match arg {
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

/// Shorthand for a catalog key label.
pub fn k(key: &str) -> Label {
    Label::key(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_parse_and_have_the_same_keys() {
        let mut keys = Vec::new();
        for (lang, source) in CATALOGS {
            let catalog =
                ultar_i18n::Catalog::parse(source).unwrap_or_else(|e| panic!("{lang:?}: {e:?}"));
            let mut k: Vec<String> = catalog.keys().map(str::to_owned).collect();
            k.sort();
            keys.push(k);
        }
        assert_eq!(keys[0], keys[1], "ar/en differ");
        assert_eq!(keys[0], keys[2], "ar/de differ");
        assert!(keys[0].len() > 100);
    }

    #[test]
    fn service_text_becomes_label_with_positional_arguments() {
        let tr = translator(Lang::En, None);
        let text = Text::key("devd.x")
            .with(TextArg::Verbatim("pci:00:02.0".into()))
            .with(TextArg::Number(3))
            .with(TextArg::Key("app.settings.area.wifi".into()));
        match label(&text, &tr) {
            Label::Key { key, args } => {
                assert_eq!(key, "devd.x");
                assert_eq!(args.len(), 3);
                assert_eq!(args[0].0, "a");
                assert_eq!(args[2].0, "c");
            }
            Label::Data(_) => panic!("expected key"),
        }
    }

    #[test]
    fn service_catalogs_are_read_from_a_folder() {
        let dir = std::env::temp_dir().join(format!("ultar-settings-i18n-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("svc")).unwrap();
        std::fs::write(dir.join("svc/en.ftl"), "svc.hello = Hello\n").unwrap();
        std::fs::write(dir.join("svc/ar.ftl"), "svc.hello = مرحبا\n").unwrap();
        let tr = translator(Lang::En, Some(&dir));
        assert_eq!(tr.t("svc.hello"), "Hello");
        let tr = translator(Lang::Ar, Some(&dir));
        assert_eq!(tr.t("svc.hello"), "مرحبا");
        let tr = translator(Lang::Ar, Some(&dir.join("missing")));
        assert!(!tr.has("svc.hello"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
