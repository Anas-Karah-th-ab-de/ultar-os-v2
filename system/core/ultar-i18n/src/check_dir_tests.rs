//! Tests of the directory checker.

use super::*;
use crate::error::ParseErrorKind;

fn sources(items: &[(&str, &str)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(name, content)| (name.to_string(), content.to_string()))
        .collect()
}

const AR_PLURAL: &str = "n = {n -> [zero] ٠ [one] ١ [two] ٢ [few] ق [many] ك *[other] {n}}\n";

#[test]
fn clean_set_has_no_problems() {
    let report = check_sources(&sources(&[
        ("en.ftl", "a = A {x}\nn = {n -> [one] one *[other] {n}}\n"),
        ("ar.ftl", &format!("a = أ {{x}}\n{AR_PLURAL}")),
        ("de.ftl", "a = A {x}\nn = {n -> [one] eins *[other] {n}}\n"),
    ]))
    .unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    assert_eq!(report.files, ["ar.ftl", "de.ftl", "en.ftl"]);
}

#[test]
fn reports_every_kind_of_problem() {
    let report = check_sources(&sources(&[
        (
            "ar.ftl",
            "a = أ {x}\nb = ب\nn = {n -> [one] ١ *[other] {n}}\n",
        ),
        ("de.ftl", "a = A {y}\nc = C\nn = {n -> *[other] {n}}\n"),
        ("en.ftl", "a = {broken\n"),
    ]))
    .unwrap();
    let kinds: Vec<&str> = report
        .problems
        .iter()
        .map(|p| match p {
            Problem::Parse { .. } => "parse",
            Problem::Missing { .. } => "missing",
            Problem::Extra { .. } => "extra",
            Problem::Placeholders { .. } => "placeholders",
            Problem::Plurals { .. } => "plurals",
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "plurals",
            "missing",
            "extra",
            "placeholders",
            "plurals",
            "parse"
        ]
    );
    assert!(!report.is_clean());
    assert!(
        matches!(&report.problems[1], Problem::Missing { file, key } if file == "de.ftl" && key == "b")
    );
    assert!(matches!(&report.problems[5], Problem::Parse { file, error }
        if file == "en.ftl" && error.kind == ParseErrorKind::UnclosedBrace));
}

#[test]
fn broken_reference_is_reported_once() {
    let report = check_sources(&sources(&[("ar.ftl", "oops"), ("en.ftl", "a = A\n")])).unwrap();
    assert_eq!(report.problems.len(), 1);
    assert!(matches!(&report.problems[0], Problem::Parse { file, .. } if file == "ar.ftl"));
}

#[test]
fn missing_reference_yields_none() {
    assert_eq!(check_sources(&sources(&[("en.ftl", "a = A\n")])), None);
    assert_eq!(check_sources(&[]), None);
}

#[test]
fn files_with_unknown_language_skip_the_plural_check() {
    let report = check_sources(&sources(&[
        ("ar.ftl", AR_PLURAL),
        ("fr.ftl", "n = {n -> *[other] {n}}\n"),
    ]))
    .unwrap();
    assert!(report.is_clean());
}

#[test]
fn problems_are_described_in_each_language() {
    let report = check_sources(&sources(&[
        (
            "ar.ftl",
            "a = أ {x}\nb = ب\nn = {n -> [one] ١ *[other] {n}}\n",
        ),
        ("de.ftl", "a = A {y}\nc = C\nn = {n -> *[other] {n}}\n"),
        ("en.ftl", "a = {broken\n"),
    ]))
    .unwrap();
    let en = Translator::new(Lang::En);
    let texts: Vec<String> = report.problems.iter().map(|p| p.describe(&en)).collect();
    assert_eq!(
        texts[0],
        "ar.ftl: key “n” lacks plural forms: zero, two, few, many"
    );
    assert_eq!(texts[1], "de.ftl: key “b” is missing");
    assert_eq!(
        texts[2],
        "de.ftl: key “c” is not in the reference file ar.ftl"
    );
    assert_eq!(
        texts[3],
        "de.ftl: placeholders of key “a” differ from the reference (missing: x; unexpected: y)"
    );
    assert_eq!(texts[5], "en.ftl: line 1: an opening brace is never closed");
    let ar = Translator::new(Lang::Ar);
    assert_eq!(
        report.problems[1].describe(&ar),
        "de.ftl: المفتاح «b» مفقود"
    );
    assert_eq!(list(&[]), "-");
}

#[test]
fn directory_errors() {
    let missing = Path::new("/nonexistent-ultar-i18n-dir");
    let error = check_directory(missing).unwrap_err();
    assert!(matches!(
        &error,
        CheckError::Io {
            kind: io::ErrorKind::NotFound,
            ..
        }
    ));
    let en = Translator::new(Lang::En);
    assert_eq!(
        error.describe(&en),
        "Could not read /nonexistent-ultar-i18n-dir"
    );
    assert!(error
        .to_string()
        .starts_with("cannot read /nonexistent-ultar-i18n-dir"));

    let base = std::env::temp_dir().join(format!("ultar-i18n-unit-{}", std::process::id()));
    std::fs::create_dir_all(&base).unwrap();
    let empty = check_directory(&base).unwrap_err();
    assert_eq!(empty, CheckError::NoCatalogs { dir: base.clone() });
    assert!(empty.describe(&en).starts_with("No .ftl files in "));
    assert!(empty.to_string().starts_with("no .ftl files in "));

    std::fs::write(base.join("en.ftl"), "a = A\n").unwrap();
    std::fs::write(base.join("notes.txt"), "ignored").unwrap();
    let no_reference = check_directory(&base).unwrap_err();
    assert_eq!(no_reference, CheckError::NoReference { dir: base.clone() });
    assert!(no_reference
        .describe(&en)
        .starts_with("The reference file ar.ftl is missing in "));
    assert!(no_reference.to_string().starts_with("ar.ftl missing in "));

    std::fs::write(base.join("ar.ftl"), "a = أ\n").unwrap();
    let report = check_directory(&base).unwrap();
    assert!(report.is_clean());
    assert_eq!(report.files, ["ar.ftl", "en.ftl"]);
    std::fs::remove_dir_all(&base).unwrap();
}

#[test]
fn shipped_locales_directory_is_clean() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("locales");
    let report = check_directory(&dir).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    assert_eq!(report.files, ["ar.ftl", "de.ftl", "en.ftl"]);
}
