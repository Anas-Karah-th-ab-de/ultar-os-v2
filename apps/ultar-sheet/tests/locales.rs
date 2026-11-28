//! The three catalogs agree (keys, placeholders), and every text key the code uses exists.

use std::path::Path;
use ultar_i18n::check::compare_catalogs;
use ultar_i18n::Catalog;

fn catalog(src: &str) -> Catalog {
    Catalog::parse(src).expect("catalog parses")
}

fn sources() -> [(&'static str, Catalog); 3] {
    [
        ("ar", catalog(include_str!("../locales/ar.ftl"))),
        ("en", catalog(include_str!("../locales/en.ftl"))),
        ("de", catalog(include_str!("../locales/de.ftl"))),
    ]
}

#[test]
fn arabic_english_and_german_have_the_same_keys_and_placeholders() {
    let [(_, ar), (_, en), (_, de)] = sources();
    for (name, other) in [("en", &en), ("de", &de)] {
        let c = compare_catalogs(&ar, other);
        assert!(
            c.is_clean(),
            "{name} differs from ar: {} problems",
            c.problem_count()
        );
    }
    assert!(ar.len() > 100);
}

fn keys_in(dir: &Path, out: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).expect("dir") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            keys_in(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).expect("read");
            for part in text.split('"').skip(1).step_by(2) {
                let is_key = (part.starts_with("sheet.") || part.starts_with("app.sheet."))
                    && part
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_');
                if is_key && !part.ends_with('.') {
                    out.push(part.to_owned());
                }
            }
        }
    }
}

#[test]
fn every_key_the_code_uses_is_translated() {
    let [(_, ar), ..] = sources();
    let mut keys = Vec::new();
    keys_in(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut keys,
    );
    assert!(keys.len() > 80, "found {} keys", keys.len());
    let missing: Vec<_> = keys.iter().filter(|k| !ar.contains(k)).collect();
    assert!(missing.is_empty(), "keys without a text: {missing:?}");
}
