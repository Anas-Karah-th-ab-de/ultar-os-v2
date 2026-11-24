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

/// The catalogs of the services whose texts the app shows (panel titles, setting labels and
/// choices of settingsd, driver texts of devd, account errors of accountd, firmware updates),
/// built in: an app's cell sees no service catalog folder, and a page must never show a raw key.
pub const SERVICE_CATALOGS: [(&str, [(Lang, &str); 3]); 4] = [
    (
        "settingsd",
        [
            (
                Lang::Ar,
                include_str!("../../../system/core/ultar-settingsd/locales/ar.ftl"),
            ),
            (
                Lang::En,
                include_str!("../../../system/core/ultar-settingsd/locales/en.ftl"),
            ),
            (
                Lang::De,
                include_str!("../../../system/core/ultar-settingsd/locales/de.ftl"),
            ),
        ],
    ),
    (
        "devd",
        [
            (
                Lang::Ar,
                include_str!("../../../system/core/ultar-devd/locales/ar.ftl"),
            ),
            (
                Lang::En,
                include_str!("../../../system/core/ultar-devd/locales/en.ftl"),
            ),
            (
                Lang::De,
                include_str!("../../../system/core/ultar-devd/locales/de.ftl"),
            ),
        ],
    ),
    (
        "accountd",
        [
            (
                Lang::Ar,
                include_str!("../../../system/services/ultar-accountd/locales/ar.ftl"),
            ),
            (
                Lang::En,
                include_str!("../../../system/services/ultar-accountd/locales/en.ftl"),
            ),
            (
                Lang::De,
                include_str!("../../../system/services/ultar-accountd/locales/de.ftl"),
            ),
        ],
    ),
    (
        "update",
        [
            (
                Lang::Ar,
                include_str!("../../../system/core/ultar-update/locales/ar.ftl"),
            ),
            (
                Lang::En,
                include_str!("../../../system/core/ultar-update/locales/en.ftl"),
            ),
            (
                Lang::De,
                include_str!("../../../system/core/ultar-update/locales/de.ftl"),
            ),
        ],
    ),
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
    for (_, catalogs) in SERVICE_CATALOGS {
        for (l, source) in catalogs {
            let _ = tr.add_source(l, source);
        }
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

    fn keys_of(source: &str) -> std::collections::BTreeSet<String> {
        ultar_i18n::Catalog::parse(source)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .keys()
            .map(str::to_owned)
            .collect()
    }

    /// Every built-in service catalog parses and has the same keys in ar, en and de.
    #[test]
    fn service_catalogs_are_complete_in_every_language() {
        for (service, catalogs) in SERVICE_CATALOGS {
            let sets: Vec<_> = catalogs.iter().map(|(_, s)| keys_of(s)).collect();
            assert_eq!(sets[0], sets[1], "{service}: ar/en differ");
            assert_eq!(sets[0], sets[2], "{service}: ar/de differ");
        }
    }

    /// Every key the app or a service it shows can produce has a text in every language:
    /// keys written in the app's sources (`"app.settings.…"`, `"settings.…"`, `"brand.…"`) and
    /// every `"accountd.…"` key of the account service (its refusals and link errors).
    #[test]
    fn no_key_is_shown_raw() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut sources = Vec::new();
        let mut dirs = vec![root.join("src")];
        while let Some(dir) = dirs.pop() {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    sources.push(std::fs::read_to_string(&path).unwrap());
                }
            }
        }
        let refusals = root.join("../../system/services/ultar-accountd/src/refusal.rs");
        sources.push(std::fs::read_to_string(refusals).unwrap());
        let mut wanted = std::collections::BTreeSet::new();
        for text in &sources {
            for piece in text.split('"').skip(1).step_by(2) {
                let prefix_ok = ["app.settings.", "accountd."]
                    .iter()
                    .any(|p| piece.starts_with(p));
                let plain = piece
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"._-".contains(&b));
                // keys built at run time end with '.' (a prefix): checked through their parts
                if prefix_ok && plain && !piece.ends_with('.') && piece.matches('.').count() >= 1 {
                    wanted.insert(piece.to_owned());
                }
            }
        }
        for lang in Lang::ALL {
            let tr = translator(lang, None);
            let missing: Vec<&String> = wanted.iter().filter(|k| tr.t(k) == k.as_str()).collect();
            assert!(
                missing.is_empty(),
                "{lang:?}: keys without text: {missing:?}"
            );
        }
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
