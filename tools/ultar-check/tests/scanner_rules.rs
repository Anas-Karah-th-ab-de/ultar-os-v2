//! Rules `paths`, `edition` and `text` on fixture repositories.

mod common;
use common::{at, Fixture};
use ultar_check::msg::Key;
use ultar_check::rules::Rule;

#[test]
fn hardware_paths_outside_hal_are_flagged() {
    let f = Fixture::new();
    f.write(
        "system/services/power/src/lib.rs",
        "pub fn a() -> &'static str {\n    \"/sys/class/power_supply\"\n}\nconst D: &str = \"/dev/input/event0\";\nconst P: &[u8] = b\"/proc/meminfo\";\n",
    );
    assert_eq!(
        f.lines(Rule::Paths),
        [
            "system/services/power/src/lib.rs:2: paths: hardware path outside the HAL: \"/sys/class/power_supply\"",
            "system/services/power/src/lib.rs:4: paths: hardware path outside the HAL: \"/dev/input/event0\"",
            "system/services/power/src/lib.rs:5: paths: hardware path outside the HAL: \"/proc/meminfo\"",
        ]
    );
}

#[test]
fn hal_drivers_and_kernel_may_name_hardware_paths() {
    let f = Fixture::new();
    let source = "pub const A: &str = \"/sys/x\";\npub const B: &str = \"/dev/y\";\n";
    f.write("system/hal/hal-generic-linux/src/lib.rs", source);
    f.write("drivers/kernel/nvme/src/lib.rs", source);
    f.write("kernel/ultar-kern/src/main.rs", source);
    assert!(f.check(&[Rule::Paths]).passed());
    f.write("system/core/ultar-devd/src/lib.rs", source);
    f.write("tools/flash/src/main.rs", source);
    assert_eq!(f.places(Rule::Paths).len(), 4);
}

#[test]
fn comments_tests_and_lookalikes_are_ignored() {
    let f = Fixture::new();
    f.write(
        "apps/files/src/lib.rs",
        r###"// see "/dev/null"
/* block "/sys/a" /* nested "/proc/b" */ still comment "/dev/c" */
/// doc "/dev/d"
pub fn f<'a>(x: &'a str) -> char {
    let _slash = '/';
    let _quote = '"'; // "/dev/e"
    let _ok = ["/system/x", "/devices/y", "sys/", "/dev", "proc"];
    '"'
}

#[cfg(test)]
mod tests {
    const T: &str = "/dev/in-test";
}

#[test]
fn t() {
    let _ = "/sys/in-test";
}
"###,
    );
    f.write("apps/files/tests/it.rs", "const T: &str = \"/dev/zero\";\n");
    f.write(
        "apps/files/src/tests.rs",
        "const T: &str = \"/proc/self\";\n",
    );
    assert_eq!(f.places(Rule::Paths), []);
}

#[test]
fn raw_strings_and_multiline_strings_are_scanned() {
    let f = Fixture::new();
    f.write(
        "platform/ultar-ui/src/lib.rs",
        r####"const A: &str = r"/dev/raw";
const B: &str = r#"quote " then /sys/raw"#;
const C: &str = r##"a "# /proc/x"##;
const D: &str = "first
/dev/second line";
const E: &str = "esc \" /dev/after-escape";
const F: &str = r#"no path "# ; // "/dev/comment-after-raw"
const G: &[u8] = br#"/sys/bytes"#;
"####,
    );
    assert_eq!(
        f.places(Rule::Paths),
        [
            at("platform/ultar-ui/src/lib.rs", 1),
            at("platform/ultar-ui/src/lib.rs", 2),
            at("platform/ultar-ui/src/lib.rs", 3),
            at("platform/ultar-ui/src/lib.rs", 4),
            at("platform/ultar-ui/src/lib.rs", 6),
            at("platform/ultar-ui/src/lib.rs", 8),
        ]
    );
}

#[test]
fn code_after_a_test_module_is_checked_again() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "#[cfg(test)]\nmod tests {\n    const T: &str = \"/dev/t\";\n}\nconst LIVE: &str = \"/dev/live\";\n#[cfg(not(test))]\nconst ALSO: &str = \"/dev/also\";\n",
    );
    assert_eq!(
        f.places(Rule::Paths),
        [at("apps/a/src/lib.rs", 5), at("apps/a/src/lib.rs", 7)]
    );
}

#[test]
fn paths_baseline_ratchet() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "const A: &str = \"/dev/a\";\nconst B: &str = \"/dev/b\";\n",
    );
    f.baseline(&[("paths", "apps/a/src/lib.rs", 2)]);
    let report = f.check(&[Rule::Paths]);
    assert!(report.passed());
    assert_eq!(report.baselined, 2);
    assert!(report.notes.is_empty());

    f.baseline(&[("paths", "apps/a/src/lib.rs", 3)]);
    let report = f.check(&[Rule::Paths]);
    assert!(report.passed());
    assert_eq!(report.notes[0].key, Key::BaselineCanLower);
    assert_eq!(
        report.notes[0].args,
        ["paths", "apps/a/src/lib.rs", "3", "2"]
    );

    f.baseline(&[("paths", "apps/a/src/lib.rs", 1)]);
    let report = f.check(&[Rule::Paths]);
    assert!(!report.passed());
    assert_eq!(report.violations.len(), 2);
}

#[test]
fn edition_queries_are_flagged_outside_profile_and_init() {
    let f = Fixture::new();
    let source = "use ultar_profile::{Capabilities, Edition};\n\npub fn f(e: Edition) -> bool {\n    e == Edition::Phone || matches!(e, Edition::Desktop)\n}\npub fn g(e: ultar_profile::Edition) {}\n";
    f.write("system/services/net/src/lib.rs", source);
    f.write("system/core/ultar-profile/src/lib.rs", source);
    f.write("system/core/ultar-init/src/main.rs", source);
    assert_eq!(
        f.lines(Rule::Edition),
        [
            "system/services/net/src/lib.rs:1: edition: edition query; behaviour must depend on capabilities",
            "system/services/net/src/lib.rs:4: edition: edition query; behaviour must depend on capabilities",
            "system/services/net/src/lib.rs:6: edition: edition query; behaviour must depend on capabilities",
        ]
    );
}

#[test]
fn edition_rule_ignores_unrelated_names_tests_and_text() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "use ultar_profile::Capabilities;\nuse other::Edition;\n// Edition::Phone\nconst S: &str = \"Edition::Phone\";\npub struct EditionInfo;\npub fn f(c: Capabilities, e: Edition) {}\n#[cfg(test)]\nmod tests {\n    use ultar_profile::Edition;\n    fn t() { let _ = Edition::Phone; }\n}\n",
    );
    assert_eq!(f.places(Rule::Edition), []);
    f.write(
        "apps/b/src/lib.rs",
        "pub fn f() { let _ = other::Edition::Server; }\n",
    );
    assert_eq!(f.places(Rule::Edition), [at("apps/b/src/lib.rs", 1)]);
}

#[test]
fn edition_baseline_ratchet() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "fn f() { let _ = Edition::Phone; }\n");
    f.baseline(&[("edition", "apps/a/src/lib.rs", 1)]);
    assert!(f.check(&[Rule::Edition]).passed());
    f.write(
        "apps/a/src/lib.rs",
        "fn f() { let _ = Edition::Phone; }\nfn g() { let _ = Edition::Tv; }\n",
    );
    assert_eq!(f.check(&[Rule::Edition]).violations.len(), 2);
}

#[test]
fn arabic_and_german_literals_are_flagged() {
    let f = Fixture::new();
    f.write(
        "apps/phone/src/lib.rs",
        "pub fn title() -> &'static str {\n    \"الهاتف\"\n}\npub const A: &str = \"Einstellungen öffnen\";\npub const B: &str = \"Straße\";\npub const C: &str = r#\"مرحبا\"#;\n",
    );
    let lines = f.lines(Rule::Text);
    assert_eq!(lines.len(), 4);
    assert_eq!(
        lines[0],
        "apps/phone/src/lib.rs:2: text: hardcoded text: \"الهاتف\" (use an i18n key)"
    );
    assert!(lines[1].starts_with("apps/phone/src/lib.rs:4: text: "));
    assert!(lines[2].starts_with("apps/phone/src/lib.rs:5: text: "));
    assert!(lines[3].starts_with("apps/phone/src/lib.rs:6: text: "));
}

#[test]
fn text_rule_exemptions() {
    let f = Fixture::new();
    f.write(
        "apps/phone/src/lib.rs",
        "/// الهاتف – doc comment\n// größer\n/* Übung */\npub const KEY: &str = \"phone.title\";\npub const ASCII: &str = \"Hello world, plain English\";\npub const CH: char = 'ب';\n#[cfg(test)]\nmod tests {\n    const T: &str = \"اختبار\";\n}\n",
    );
    f.write("apps/phone/src/msg.rs", "pub const AR: &str = \"مرحبا\";\n");
    f.write(
        "apps/phone/src/msg/de.rs",
        "pub const DE: &str = \"Grüße\";\n",
    );
    f.write("apps/phone/tests/it.rs", "const T: &str = \"تجربة\";\n");
    f.write(
        "system/core/ultar-i18n/src/lib.rs",
        "pub const D: &str = \"٠١٢٣\";\n",
    );
    f.write("i18n/ar.ftl", "phone-title = الهاتف\n");
    assert_eq!(f.places(Rule::Text), []);
}

#[test]
fn text_baseline_ratchet() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "const A: &str = \"مرحبا\";\n");
    f.baseline(&[
        ("text", "apps/a/src/lib.rs", 1),
        ("text", "apps/gone/src/lib.rs", 4),
    ]);
    let report = f.check(&[Rule::Text]);
    assert!(report.passed());
    assert_eq!(report.notes.len(), 1);
    assert_eq!(report.notes[0].key, Key::BaselineCanRemove);
    f.write(
        "apps/a/src/lib.rs",
        "const A: &str = \"مرحبا\";\nconst B: &str = \"Tschüss\";\n",
    );
    assert!(!f.check(&[Rule::Text]).passed());
}

#[test]
fn invalid_utf8_sources_do_not_abort_the_run() {
    let f = Fixture::new();
    f.write_bytes("apps/a/src/lib.rs", b"const A: &str = \"/dev/\xff\";\n");
    assert_eq!(f.places(Rule::Paths), [at("apps/a/src/lib.rs", 1)]);
}
