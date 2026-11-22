//! Fails if the shipped catalogs drift apart: every key of `ar.ftl` must exist in `en.ftl` and
//! `de.ftl` and the other way round, with the same placeholders and all needed plural forms.

use ultar_i18n::check::{compare_catalogs, incomplete_plurals};
use ultar_i18n::{embedded, missing_keys, placeholder_mismatches, Lang};

#[test]
fn no_key_is_missing_in_any_language() {
    let reference = embedded::catalog(Lang::Ar).expect("ar.ftl parses");
    for lang in [Lang::En, Lang::De] {
        let other = embedded::catalog(lang).expect("catalog parses");
        let missing = missing_keys(&reference, &other);
        assert!(
            missing.is_empty(),
            "{lang}.ftl lacks keys of ar.ftl: {missing:?}"
        );
        let extra = missing_keys(&other, &reference);
        assert!(
            extra.is_empty(),
            "ar.ftl lacks keys of {lang}.ftl: {extra:?}"
        );
    }
}

#[test]
fn placeholders_match_the_arabic_reference() {
    let reference = embedded::catalog(Lang::Ar).expect("ar.ftl parses");
    for lang in [Lang::En, Lang::De] {
        let other = embedded::catalog(lang).expect("catalog parses");
        let mismatches = placeholder_mismatches(&reference, &other);
        assert!(mismatches.is_empty(), "{lang}.ftl: {mismatches:?}");
        assert!(compare_catalogs(&reference, &other).is_clean());
    }
}

#[test]
fn plural_messages_have_all_forms_of_their_language() {
    for lang in Lang::ALL {
        let catalog = embedded::catalog(lang).expect("catalog parses");
        let gaps = incomplete_plurals(&catalog, lang);
        assert!(gaps.is_empty(), "{lang}.ftl: {gaps:?}");
    }
}

#[test]
fn base_set_is_large_enough_and_covers_the_required_groups() {
    let reference = embedded::catalog(Lang::Ar).expect("ar.ftl parses");
    assert!(reference.len() >= 150, "only {} keys", reference.len());
    let groups = [
        ("action.", 30),
        ("state.", 15),
        ("installer.", 25),
        ("settings.", 25),
        ("hw.class.", 15),
        ("hw.state.", 5),
        ("error.", 15),
        ("edition.", 3),
        ("form.", 5),
        ("unit.", 8),
        ("count.", 5),
        ("cal.", 50),
    ];
    for (prefix, minimum) in groups {
        let count = reference.keys_with_prefix(prefix).count();
        assert!(count >= minimum, "{prefix}* has only {count} keys");
    }
}

#[test]
fn catalogs_use_western_digits_only() {
    for lang in Lang::ALL {
        let source = embedded::source(lang);
        let offending = source
            .chars()
            .find(|c| ultar_i18n::numbers::western_digit(*c).is_some());
        assert_eq!(offending, None, "{lang}.ftl contains a non-Western digit");
    }
}

#[test]
fn arabic_texts_are_arabic_and_translations_are_not_left_empty() {
    let arabic = embedded::catalog(Lang::Ar).expect("ar.ftl parses");
    for prefix in [
        "action.",
        "state.",
        "installer.",
        "settings.",
        "error.",
        "hw.state.",
    ] {
        for key in arabic.keys_with_prefix(prefix) {
            let text = arabic.get(key).expect("key exists").format(Lang::Ar, &[]);
            let allowed_latin = ["settings.vpn", "action.ok"];
            assert!(
                ultar_i18n::contains_rtl(&text) || allowed_latin.contains(&key),
                "{key} has no Arabic text: {text}"
            );
        }
    }
    for lang in Lang::ALL {
        let catalog = embedded::catalog(lang).expect("catalog parses");
        for (key, message) in catalog.iter() {
            assert!(
                !message.format(lang, &[]).trim().is_empty(),
                "{lang}: {key} is empty"
            );
        }
    }
}
