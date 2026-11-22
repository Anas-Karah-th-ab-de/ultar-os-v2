//! Runs the `ultar-i18n-check` binary the way CI does.

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

fn run(args: &[&str], lang: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ultar-i18n-check"))
        .args(args)
        .env("ULTAR_LANG", lang)
        .env_remove("LC_ALL")
        .env_remove("LC_MESSAGES")
        .env_remove("LANG")
        .output()
        .expect("binary runs")
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ultar-i18n-bin-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn shipped_catalogs_pass() {
    let locales = concat!(env!("CARGO_MANIFEST_DIR"), "/locales");
    let output = run(&[locales], "en");
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).contains("Files checked: ar.ftl, de.ftl, en.ftl"));
    assert!(stdout(&output).contains("The catalogs are complete and consistent."));
    let arabic = run(&[locales], "ar");
    assert_eq!(arabic.status.code(), Some(0));
    assert!(stdout(&arabic).contains("ملفات الترجمة مكتملة ومتطابقة."));
}

#[test]
fn problems_give_exit_code_one() {
    let dir = temp_dir("problems");
    fs::write(dir.join("ar.ftl"), "a = أ {x}\nb = ب\n").unwrap();
    fs::write(dir.join("en.ftl"), "a = A\nc = C\n").unwrap();
    fs::write(dir.join("de.ftl"), "a = {kaputt\n").unwrap();
    let output = run(&[dir.to_str().unwrap()], "en");
    let text = stdout(&output);
    assert_eq!(output.status.code(), Some(1), "{text}");
    assert!(text.contains("en.ftl: key “b” is missing"));
    assert!(text.contains("en.ftl: key “c” is not in the reference file ar.ftl"));
    assert!(text.contains("(missing: x; unexpected: -)"));
    assert!(text.contains("de.ftl: line 1: an opening brace is never closed"));
    assert!(text.contains("4 problems found"));
    let german = run(&[dir.to_str().unwrap()], "de");
    assert!(stdout(&german).contains("4 Probleme gefunden"));
    let arabic = run(&[dir.to_str().unwrap()], "ar");
    assert!(stdout(&arabic).contains("عُثر على 4 مشكلات"));
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn usage_and_directory_errors_give_exit_code_two() {
    let none = run(&[], "en");
    assert_eq!(none.status.code(), Some(2));
    assert!(stderr(&none).contains("Usage: ultar-i18n-check <directory>"));
    let many = run(&["a", "b"], "en");
    assert_eq!(many.status.code(), Some(2));
    let missing = run(&["/nonexistent-ultar-i18n-dir"], "en");
    assert_eq!(missing.status.code(), Some(2));
    assert!(stderr(&missing).contains("Could not read /nonexistent-ultar-i18n-dir"));

    let empty = temp_dir("empty");
    let output = run(&[empty.to_str().unwrap()], "en");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("No .ftl files in"));
    fs::write(empty.join("en.ftl"), "a = A\n").unwrap();
    let output = run(&[empty.to_str().unwrap()], "en");
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("The reference file ar.ftl is missing in"));
    fs::remove_dir_all(&empty).unwrap();
}

#[test]
fn help_prints_usage_and_default_language_is_arabic() {
    let help = run(&["--help"], "de");
    assert_eq!(help.status.code(), Some(0));
    assert!(stdout(&help).contains("Aufruf: ultar-i18n-check <Ordner>"));
    let fallback = run(&["-h"], "xx");
    assert_eq!(fallback.status.code(), Some(0));
    assert!(stdout(&fallback).contains("الاستخدام: ultar-i18n-check"));
}
