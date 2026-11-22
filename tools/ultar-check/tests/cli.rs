//! Rule `budget`, the `all` / `baseline` commands and the binary itself.

mod common;
use common::Fixture;
use std::path::Path;
use std::process::{Command, Output};
use ultar_check::error::CheckError;
use ultar_check::msg::Key;
use ultar_check::rules::Rule;

const BUDGETS: &str = "[binaries]\nultar-init = 100\nultar-ctl = 50\nultar-devd = 10\n";

fn run_bin(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ultar-check"))
        .arg("--root")
        .arg(root)
        .args(args)
        .env("ULTAR_LANG", "en")
        .output()
        .unwrap()
}

fn stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).unwrap()
}

#[test]
fn budget_without_table_or_binaries_is_skipped_with_notes() {
    let f = Fixture::new();
    let report = f.check(&[Rule::Budget]);
    assert!(report.passed());
    assert_eq!(report.notes[0].key, Key::BudgetNoConfig);

    f.write("tools/budgets.toml", BUDGETS);
    let report = f.check(&[Rule::Budget]);
    assert!(report.passed());
    let keys: Vec<Key> = report.notes.iter().map(|n| n.key).collect();
    assert_eq!(keys, [Key::BudgetNotBuilt; 3]);
    assert_eq!(report.notes[0].args, ["ultar-ctl"]);
}

#[test]
fn budget_checks_host_and_cross_release_binaries() {
    let f = Fixture::new();
    f.write("tools/budgets.toml", BUDGETS);
    f.write_bytes("target/release/ultar-init", &[0u8; 100]);
    f.write_bytes(
        "target/aarch64-unknown-linux-musl/release/ultar-init",
        &[0u8; 101],
    );
    f.write_bytes("target/release/ultar-ctl", &[0u8; 60]);
    f.write_bytes("target/debug/ultar-devd", &[0u8; 999]);
    let report = f.check(&[Rule::Budget]);
    let lines: Vec<String> = report
        .violations
        .iter()
        .map(|v| v.to_line(ultar_check::msg::Lang::En))
        .collect();
    assert_eq!(
        lines,
        [
            "ultar-ctl:0: budget: binary release/ultar-ctl is 60 bytes (budget 50)",
            "ultar-init:0: budget: binary aarch64-unknown-linux-musl/release/ultar-init is 101 bytes (budget 100)",
        ]
    );
    assert_eq!(report.notes.len(), 1);
    assert_eq!(report.notes[0].args, ["ultar-devd"]);
}

#[test]
fn budget_honours_target_dir_and_baseline() {
    let f = Fixture::new();
    f.write("tools/budgets.toml", BUDGETS);
    f.write_bytes("out/release/ultar-ctl", &[0u8; 60]);
    let mut options = f.options(&[Rule::Budget]);
    options.target_dir = Some(f.root().join("out"));
    assert_eq!(ultar_check::execute(&options).unwrap().violations.len(), 1);

    f.baseline(&[("budget", "ultar-ctl", 1)]);
    let report = ultar_check::execute(&options).unwrap();
    assert!(report.passed());
    assert_eq!(report.baselined, 1);
}

#[test]
fn broken_budget_table_is_an_error() {
    let f = Fixture::new();
    f.write("tools/budgets.toml", "[binaries]\nultar-init = \"big\"\n");
    let error = ultar_check::execute(&f.options(&[Rule::Budget])).unwrap_err();
    assert!(matches!(error, CheckError::Toml { ref path, .. } if path == "tools/budgets.toml"));
}

#[test]
fn baseline_command_output_makes_the_repository_pass() {
    let f = Fixture::new();
    f.krate("misc/stray", "");
    f.write(
        "apps/a/src/lib.rs",
        "fn f(x: Option<u8>) { x.unwrap(); x.unwrap(); }\nconst P: &str = \"/dev/x\";\n",
    );
    assert!(!f.check(&Rule::ALL).passed());
    let generated = ultar_check::baseline_toml(&f.baseline_command()).unwrap();
    assert!(generated.contains("rule = \"unwrap\"\npath = \"apps/a/src/lib.rs\"\ncount = 2\n"));
    assert!(generated.contains("rule = \"layers\"\npath = \"misc/stray/Cargo.toml\"\ncount = 1\n"));
    f.write("tools/check-baseline.toml", &generated);
    let report = f.check(&Rule::ALL);
    assert!(report.passed());
    assert_eq!(report.baselined, 4);
}

#[test]
fn explicit_baseline_path_and_no_baseline_flag() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "fn f(x: Option<u8>) { x.unwrap(); }\n");
    f.write(
        "elsewhere/base.toml",
        "[[entry]]\nrule = \"unwrap\"\npath = \"apps/a/src/lib.rs\"\ncount = 1\n",
    );
    let mut options = f.options(&[Rule::Unwrap]);
    assert!(!ultar_check::execute(&options).unwrap().passed());
    options.baseline = Some(f.root().join("elsewhere/base.toml"));
    assert!(ultar_check::execute(&options).unwrap().passed());
    options.use_baseline = false;
    assert!(!ultar_check::execute(&options).unwrap().passed());
}

#[test]
fn missing_root_is_an_io_error() {
    let f = Fixture::new();
    let mut options = f.options(&[Rule::Paths]);
    options.root = f.root().join("does-not-exist");
    assert!(matches!(
        ultar_check::execute(&options).unwrap_err(),
        CheckError::Io { .. }
    ));
}

#[test]
fn binary_exits_zero_on_a_clean_repository() {
    let f = Fixture::new();
    f.krate("apps/a", "");
    f.write("apps/a/src/lib.rs", "pub fn f() -> u8 { 1 }\n");
    let output = run_bin(f.root(), &["all"]);
    assert_eq!(output.status.code(), Some(0), "{}", stdout(&output));
    assert!(stdout(&output).ends_with("ultar-check: ok (0 baselined, 0 warnings)\n"));
    assert!(stdout(&output).contains("note: budget: tools/budgets.toml not found, skipped"));
}

#[test]
fn binary_exits_one_and_lists_violations() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "fn f(x: Option<u8>) { x.unwrap(); }\nconst P: &str = \"/dev/x\";\n",
    );
    let output = run_bin(f.root(), &["all"]);
    assert_eq!(output.status.code(), Some(1));
    let text = stdout(&output);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines[0],
        "apps/a/src/lib.rs:2: paths: hardware path outside the HAL: \"/dev/x\""
    );
    assert_eq!(
        lines[1],
        "apps/a/src/lib.rs:1: unwrap: `.unwrap()` outside test code"
    );
    assert!(text.contains("FAILED - 2 violations"));

    let single = run_bin(f.root(), &["paths"]);
    assert_eq!(single.status.code(), Some(1));
    assert_eq!(stdout(&single).lines().count(), 2);
    assert_eq!(run_bin(f.root(), &["edition"]).status.code(), Some(0));
}

#[test]
fn binary_json_output_is_machine_readable() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "fn f(x: Option<u8>) { x.unwrap(); }\n");
    let output = run_bin(f.root(), &["unwrap", "--format", "json"]);
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_str(&stdout(&output)).unwrap();
    assert_eq!(value["passed"], false);
    assert_eq!(value["violations"][0]["rule"], "unwrap");
    assert_eq!(value["violations"][0]["path"], "apps/a/src/lib.rs");
    assert_eq!(value["violations"][0]["line"], 1);
    assert_eq!(
        value["violations"][0]["detail"],
        "`.unwrap()` outside test code"
    );
}

#[test]
fn binary_baseline_and_ratchet_round_trip() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "fn f(x: Option<u8>) { x.unwrap(); }\n");
    let generated = run_bin(f.root(), &["baseline"]);
    assert_eq!(generated.status.code(), Some(0));
    f.write("tools/check-baseline.toml", &stdout(&generated));
    assert_eq!(run_bin(f.root(), &["all"]).status.code(), Some(0));
    assert_eq!(
        run_bin(f.root(), &["all", "--no-baseline"]).status.code(),
        Some(1)
    );
    f.write(
        "apps/a/src/lib.rs",
        "fn f(x: Option<u8>) { x.unwrap(); x.unwrap(); }\n",
    );
    let grown = run_bin(f.root(), &["all"]);
    assert_eq!(grown.status.code(), Some(1));
    assert!(
        stdout(&grown).contains("note: unwrap: apps/a/src/lib.rs: 2 violations, baseline allows 1")
    );
}

#[test]
fn binary_usage_and_config_errors_exit_two() {
    let f = Fixture::new();
    let output = run_bin(f.root(), &[]);
    assert_eq!(output.status.code(), Some(2));
    assert_eq!(
        stderr(&output),
        "ultar-check: no command given (try --help)\n"
    );
    assert_eq!(run_bin(f.root(), &["nope"]).status.code(), Some(2));
    assert_eq!(
        run_bin(f.root(), &["all", "--format", "xml"]).status.code(),
        Some(2)
    );

    f.write(
        "tools/check-baseline.toml",
        "[[entry]]\nrule = \"nope\"\npath = \"a\"\ncount = 1\n",
    );
    let output = run_bin(f.root(), &["all"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(stderr(&output).contains("unknown rule `nope`"));
    assert!(stdout(&output).is_empty());
}

#[test]
fn binary_help_is_available_in_three_languages() {
    let f = Fixture::new();
    let english = run_bin(f.root(), &["--help"]);
    assert_eq!(english.status.code(), Some(0));
    assert!(stdout(&english).starts_with("usage: ultar-check"));
    assert!(stdout(&run_bin(f.root(), &["--help", "--lang", "de"])).starts_with("Aufruf:"));
    let arabic = stdout(&run_bin(f.root(), &["--help", "--lang", "ar"]));
    assert!(arabic.contains("ultar-check") && !arabic.starts_with("usage"));
}

#[test]
fn the_crate_passes_its_own_source_checks() {
    let f = Fixture::new();
    let crate_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for dir in ["src", "src/scan", "src/rules", "tests", "tests/common"] {
        for entry in std::fs::read_dir(crate_dir.join(dir)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "rs") {
                let name = path.file_name().unwrap().to_str().unwrap();
                let text = std::fs::read_to_string(&path).unwrap();
                f.write(&format!("tools/ultar-check/{dir}/{name}"), &text);
            }
        }
    }
    f.write(
        "tools/ultar-check/Cargo.toml",
        &std::fs::read_to_string(crate_dir.join("Cargo.toml")).unwrap(),
    );
    let report = f.check(&Rule::ALL);
    let lines: Vec<String> = report
        .violations
        .iter()
        .map(|v| v.to_line(ultar_check::msg::Lang::En))
        .collect();
    assert_eq!(lines, Vec::<String>::new());
}
