//! Rule `layers` on fixture repositories.

mod common;
use common::{at, Fixture};
use ultar_check::error::{CheckError, ConfigIssue};
use ultar_check::msg::Key;
use ultar_check::rules::Rule;

fn clean_repo() -> Fixture {
    let f = Fixture::new();
    f.write("Cargo.toml", "[workspace]\nmembers = []\n");
    f.krate("system/core/ultar-bus", "serde = \"1\"\n");
    f.krate("system/core/ultar-i18n", "");
    f.krate(
        "system/core/ultar-init",
        "ultar-bus = { path = \"../ultar-bus\" }\n",
    );
    f.krate(
        "system/hal/ultar-hal",
        "ultar-bus = { path = \"../../core/ultar-bus\" }\n",
    );
    f.krate(
        "system/hal/hal-generic-linux",
        "ultar-hal = { path = \"../ultar-hal\" }\n",
    );
    f.krate(
        "system/services/net",
        "ultar-hal = { path = \"../../hal/ultar-hal\" }\nultar-bus = { path = \"../../core/ultar-bus\" }\n",
    );
    f.krate(
        "platform/ultar-ui",
        "ultar-i18n = { path = \"../../system/core/ultar-i18n\" }\n",
    );
    f.krate(
        "platform/app-api",
        "ultar-ui = { path = \"../ultar-ui\" }\n",
    );
    f.krate(
        "apps/phone",
        "app-api = { path = \"../../platform/app-api\" }\nultar-bus = { path = \"../../system/core/ultar-bus\" }\n",
    );
    f.krate("drivers/kernel/nvme", "");
    f.krate(
        "kernel/ultar-kern",
        "nvme = { path = \"../../drivers/kernel/nvme\" }\n",
    );
    f.krate(
        "tools/ultar-check",
        "phone = { path = \"../../apps/phone\" }\n",
    );
    f
}

#[test]
fn allowed_directions_pass() {
    let f = clean_repo();
    let report = f.check(&[Rule::Layers]);
    assert!(report.passed(), "{:?}", report.violations);
    assert!(report.warnings.is_empty());
}

#[test]
fn empty_repository_passes() {
    assert!(Fixture::new().check(&[Rule::Layers]).passed());
}

#[test]
fn app_must_not_use_hal_or_other_platform_crates() {
    let f = clean_repo();
    f.krate("platform/ultar-compositor", "");
    f.krate(
        "apps/camera",
        "ultar-hal = { path = \"../../system/hal/ultar-hal\" }\ncompositor = { path = \"../../platform/ultar-compositor\" }\nphone = { path = \"../phone\" }\n",
    );
    let lines = f.lines(Rule::Layers);
    assert_eq!(
        lines,
        [
            "apps/camera/Cargo.toml:7: layers: forbidden dependency `ultar-hal` -> `system/hal/ultar-hal` (area apps)",
            "apps/camera/Cargo.toml:8: layers: forbidden dependency `compositor` -> `platform/ultar-compositor` (area apps)",
            "apps/camera/Cargo.toml:9: layers: forbidden dependency `phone` -> `apps/phone` (area apps)",
        ]
    );
}

#[test]
fn upward_dependencies_are_named_as_such() {
    let f = clean_repo();
    f.krate(
        "system/core/ultar-devd",
        "ui = { path = \"../../../platform/ultar-ui\" }\n",
    );
    f.krate(
        "kernel/extra",
        "bus = { path = \"../../system/core/ultar-bus\" }\n",
    );
    let report = f.check(&[Rule::Layers]);
    let keys: Vec<Key> = report.violations.iter().map(|v| v.message.key).collect();
    assert_eq!(keys, [Key::LayerUpward, Key::LayerUpward]);
    assert_eq!(report.violations[0].path, "kernel/extra/Cargo.toml");
}

#[test]
fn backends_and_services_are_isolated_from_each_other() {
    let f = clean_repo();
    f.krate(
        "system/hal/hal-exynos7885",
        "generic = { path = \"../hal-generic-linux\" }\n",
    );
    f.krate(
        "system/services/audio",
        "net = { path = \"../net\" }\nbackend = { path = \"../../hal/hal-generic-linux\" }\n",
    );
    assert_eq!(
        f.places(Rule::Layers),
        [
            at("system/hal/hal-exynos7885/Cargo.toml", 7),
            at("system/services/audio/Cargo.toml", 7),
            at("system/services/audio/Cargo.toml", 8),
        ]
    );
}

#[test]
fn crate_outside_any_area_is_reported() {
    let f = clean_repo();
    f.krate("misc/thing", "");
    f.krate("system/loose", "");
    f.write(
        "Cargo.toml",
        "[package]\nname = \"root\"\nversion = \"0.7.0\"\n",
    );
    let lines = f.lines(Rule::Layers);
    assert_eq!(
        lines,
        [
            "Cargo.toml:1: layers: crate `.` is not inside any area folder",
            "misc/thing/Cargo.toml:1: layers: crate `misc/thing` is not inside any area folder",
            "system/loose/Cargo.toml:1: layers: crate `system/loose` is not inside any area folder",
        ]
    );
}

#[test]
fn dependency_into_a_non_area_folder_or_outside_the_repo_fails() {
    let f = clean_repo();
    f.krate("devices/a30", "");
    f.krate(
        "apps/clock",
        "a30 = { path = \"../../devices/a30\" }\nelsewhere = { path = \"../../../elsewhere\" }\n",
    );
    let report = f.check(&[Rule::Layers]);
    let found: Vec<(&str, Key)> = report
        .violations
        .iter()
        .map(|v| (v.path.as_str(), v.message.key))
        .collect();
    assert_eq!(
        found,
        [
            ("apps/clock/Cargo.toml", Key::LayerForbidden),
            ("apps/clock/Cargo.toml", Key::LayerOutsideRepo),
            ("devices/a30/Cargo.toml", Key::LayerNotInArea),
        ]
    );
}

#[test]
fn dev_build_and_target_dependencies_are_checked_too() {
    let f = clean_repo();
    f.write(
        "apps/notes/Cargo.toml",
        "[package]\nname = \"notes\"\n\n[dev-dependencies]\nnet = { path = \"../../system/services/net\" }\n\n[build-dependencies.init]\npath = \"../../system/core/ultar-init\"\n\n[target.'cfg(unix)'.dependencies]\nhal = { path = \"../../system/hal/ultar-hal\" }\n",
    );
    assert_eq!(
        f.places(Rule::Layers),
        [
            at("apps/notes/Cargo.toml", 5),
            at("apps/notes/Cargo.toml", 7),
            at("apps/notes/Cargo.toml", 11),
        ]
    );
}

#[test]
fn workspace_inherited_path_dependencies_are_resolved() {
    let f = clean_repo();
    f.write(
        "Cargo.toml",
        "[workspace]\nmembers = []\n[workspace.dependencies]\nultar-init = { path = \"system/core/ultar-init\" }\nultar-bus = { path = \"system/core/ultar-bus\" }\nserde = \"1\"\n",
    );
    f.krate(
        "apps/store",
        "ultar-bus = { workspace = true }\nultar-init.workspace = true\nserde = { workspace = true }\n",
    );
    assert_eq!(f.places(Rule::Layers), [at("apps/store/Cargo.toml", 8)]);
}

#[test]
fn build_output_and_hidden_folders_are_ignored() {
    let f = clean_repo();
    f.krate("target/package/stray", "");
    f.krate(".cache/stray", "");
    f.krate("apps/phone/vendor/stray", "");
    assert!(f.check(&[Rule::Layers]).passed());
}

#[test]
fn repository_config_replaces_the_builtin_table() {
    let f = Fixture::new();
    f.write(
        "tools/check-layers.toml",
        "exclude = ['legacy']\n[[area]]\nname = 'low'\nprefix = 'low/'\nrank = 1\ninternal = true\n[[area]]\nname = 'high'\nprefix = 'high/'\nrank = 2\n[[allow]]\nfrom = 'high'\nto = 'low'\ncrates = ['low/api']\n",
    );
    f.krate("low/api", "");
    f.krate("low/impl", "api = { path = \"../api\" }\n");
    f.krate("high/a", "api = { path = \"../../low/api\" }\n");
    f.krate("legacy/old", "a = { path = \"../../high/a\" }\n");
    assert!(f.check(&[Rule::Layers]).passed());
    f.krate(
        "high/b",
        "x = { path = \"../../low/impl\" }\ny = { path = \"../a\" }\n",
    );
    f.krate("low/bad", "a = { path = \"../../high/a\" }\n");
    f.krate("apps/phone", "");
    let report = f.check(&[Rule::Layers]);
    let found: Vec<(String, Key)> = report
        .violations
        .iter()
        .map(|v| (format!("{}:{}", v.path, v.line), v.message.key))
        .collect();
    assert_eq!(
        found,
        [
            ("apps/phone/Cargo.toml:1".to_string(), Key::LayerNotInArea),
            ("high/b/Cargo.toml:7".to_string(), Key::LayerForbidden),
            ("high/b/Cargo.toml:8".to_string(), Key::LayerForbidden),
            ("low/bad/Cargo.toml:7".to_string(), Key::LayerUpward),
        ]
    );
}

#[test]
fn broken_inputs_are_errors_not_violations() {
    let f = clean_repo();
    f.write("apps/broken/Cargo.toml", "[package\n");
    let error = ultar_check::execute(&f.options(&[Rule::Layers])).unwrap_err();
    assert!(matches!(error, CheckError::Toml { ref path, .. } if path == "apps/broken/Cargo.toml"));

    let f = clean_repo();
    f.write(
        "tools/check-layers.toml",
        "[[allow]]\nfrom = 'a'\nto = 'b'\n",
    );
    let error = ultar_check::execute(&f.options(&[Rule::Layers])).unwrap_err();
    assert!(matches!(
        error,
        CheckError::Config {
            issue: ConfigIssue::UnknownArea(_),
            ..
        }
    ));
}

#[test]
fn baseline_ratchet_applies_to_manifests() {
    let f = clean_repo();
    f.krate(
        "apps/camera",
        "ultar-hal = { path = \"../../system/hal/ultar-hal\" }\n",
    );
    f.baseline(&[("layers", "apps/camera/Cargo.toml", 1)]);
    let report = f.check(&[Rule::Layers]);
    assert!(report.passed());
    assert_eq!(report.baselined, 1);

    f.krate(
        "apps/camera",
        "ultar-hal = { path = \"../../system/hal/ultar-hal\" }\nnet = { path = \"../../system/services/net\" }\n",
    );
    let report = f.check(&[Rule::Layers]);
    assert_eq!(report.violations.len(), 2);
    assert_eq!(report.notes[0].key, Key::BaselineExceeded);
}
