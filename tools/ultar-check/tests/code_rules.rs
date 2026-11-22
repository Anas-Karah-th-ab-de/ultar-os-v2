//! Rules `size`, `unsafe`, `unwrap`, `logical-layout` and `linux-calls` on fixtures.

mod common;
use common::{at, Fixture};
use ultar_check::msg::Key;
use ultar_check::rules::Rule;

fn lines(count: usize) -> String {
    "// filler\n".repeat(count)
}

fn function(name: &str, body_lines: usize) -> String {
    format!(
        "fn {name}() {{\n{}}}\n",
        "    let _ = 1;\n".repeat(body_lines)
    )
}

#[test]
fn file_length_limits() {
    let f = Fixture::new();
    f.write("apps/a/src/ok.rs", &lines(400));
    f.write("apps/a/src/warn.rs", &lines(401));
    f.write("apps/a/src/edge.rs", &lines(600));
    f.write("apps/a/src/long.rs", &lines(601));
    let report = f.check(&[Rule::Size]);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, "apps/a/src/long.rs");
    assert_eq!(report.violations[0].message.args, ["601", "600"]);
    let warned: Vec<&str> = report.warnings.iter().map(|w| w.path.as_str()).collect();
    assert_eq!(warned, ["apps/a/src/edge.rs", "apps/a/src/warn.rs"]);
    assert!(!report.passed());
}

#[test]
fn warnings_alone_do_not_fail() {
    let f = Fixture::new();
    f.write("apps/a/src/warn.rs", &lines(450));
    let report = f.check(&[Rule::Size]);
    assert!(report.passed());
    assert_eq!(report.warnings[0].message.key, Key::FileLong);
}

#[test]
fn function_length_limit() {
    let f = Fixture::new();
    let source = format!(
        "{}{}trait T {{\n    fn declared(&self);\n}}\ntype F = fn(u8) -> u8;\n",
        function("fits", 58),
        function("too_long", 59)
    );
    f.write("system/core/ultar-init/src/lib.rs", &source);
    assert_eq!(
        f.lines(Rule::Size),
        ["system/core/ultar-init/src/lib.rs:61: size: function `too_long` too long: 61 lines (max 60)"]
    );
}

#[test]
fn function_length_handles_signatures_strings_and_nesting() {
    let f = Fixture::new();
    let filler = "    let _ = 1;\n".repeat(60);
    let source = format!(
        "fn outer<T>(\n    a: [u8; 2],\n) -> T\nwhere\n    T: Default,\n{{\n    let _s = \"}} not a brace\";\n    let _c = '}}';\n    // }}\n    fn inner() {{}}\n{filler}    T::default()\n}}\nfn after() {{}}\n"
    );
    f.write("apps/a/src/lib.rs", &source);
    let report = f.check(&[Rule::Size]);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].line, 1);
    assert_eq!(report.violations[0].message.args, ["outer", "72", "60"]);
}

#[test]
fn struct_field_limit() {
    let f = Fixture::new();
    let fields = |n: usize| -> String {
        (0..n)
            .map(|i| format!("    pub f{i}: Vec<(u8, u8)>,\n"))
            .collect()
    };
    let tuple: String = (0..16).map(|_| "u8, ").collect();
    let source = format!(
        "pub struct Fits {{\n{}}}\npub struct Big {{\n{}}}\npub struct Tuple({tuple});\nenum E {{ A {{ a: u8 }} }}\n",
        fields(15),
        fields(16)
    );
    f.write("apps/a/src/lib.rs", &source);
    assert_eq!(
        f.lines(Rule::Size),
        [
            "apps/a/src/lib.rs:18: size: struct `Big` has 16 fields (max 15)",
            "apps/a/src/lib.rs:36: size: struct `Tuple` has 16 fields (max 15)",
        ]
    );
}

#[test]
fn size_baseline_ratchet() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", &function("long_one", 80));
    f.baseline(&[("size", "apps/a/src/lib.rs", 1)]);
    assert!(f.check(&[Rule::Size]).passed());
    f.write(
        "apps/a/src/lib.rs",
        &format!("{}{}", function("long_one", 80), function("long_two", 80)),
    );
    assert_eq!(f.check(&[Rule::Size]).violations.len(), 2);
    f.write("apps/a/src/lib.rs", &function("short", 3));
    let report = f.check(&[Rule::Size]);
    assert!(report.passed());
    assert_eq!(report.notes[0].key, Key::BaselineCanRemove);
}

#[test]
fn unsafe_needs_a_safety_comment_nearby() {
    let f = Fixture::new();
    f.write(
        "system/hal/hal-generic-linux/src/lib.rs",
        "pub fn a(p: *const u8) -> u8 {\n    // SAFETY: caller guarantees a valid pointer.\n    unsafe { *p }\n}\npub fn b(p: *const u8) -> u8 {\n    unsafe { *p }\n}\n// SAFETY: too far away.\n\n\n\nunsafe impl Send for X {}\n/* SAFETY: block comment,\n   two lines */\nunsafe fn c() {}\nfn d() { let _ = unsafe { 1 }; } // SAFETY: same line\n\n\n\n// safety: wrong case\nunsafe trait T {}\n",
    );
    assert_eq!(
        f.lines(Rule::Unsafe),
        [
            "system/hal/hal-generic-linux/src/lib.rs:6: unsafe: `unsafe` without a `// SAFETY:` comment in the 3 lines before",
            "system/hal/hal-generic-linux/src/lib.rs:12: unsafe: `unsafe` without a `// SAFETY:` comment in the 3 lines before",
            "system/hal/hal-generic-linux/src/lib.rs:21: unsafe: `unsafe` without a `// SAFETY:` comment in the 3 lines before",
        ]
    );
}

#[test]
fn unsafe_window_is_exactly_three_lines() {
    let f = Fixture::new();
    f.write("kernel/k/src/a.rs", "// SAFETY: ok\n\n\nunsafe fn a() {}\n");
    f.write(
        "kernel/k/src/b.rs",
        "// SAFETY: far\n\n\n\nunsafe fn b() {}\n",
    );
    assert_eq!(f.places(Rule::Unsafe), [at("kernel/k/src/b.rs", 5)]);
}

#[test]
fn unsafe_is_forbidden_outside_kernel_drivers_hal() {
    let f = Fixture::new();
    let source = "// SAFETY: justified, but the area is wrong.\nunsafe fn a() {}\n// unsafe in a comment\nconst S: &str = \"unsafe\";\n#![forbid(unsafe_code)]\n";
    f.write("apps/a/src/lib.rs", source);
    f.write("system/core/ultar-bus/src/lib.rs", source);
    f.write("drivers/kernel/nvme/src/lib.rs", source);
    f.write("kernel/ultar-kern/src/lib.rs", source);
    assert_eq!(
        f.lines(Rule::Unsafe),
        [
            "apps/a/src/lib.rs:2: unsafe: `unsafe` is not allowed in this area",
            "system/core/ultar-bus/src/lib.rs:2: unsafe: `unsafe` is not allowed in this area",
        ]
    );
}

#[test]
fn unsafe_baseline_ratchet() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "unsafe fn a() {}\n");
    f.baseline(&[("unsafe", "apps/a/src/lib.rs", 1)]);
    assert!(f.check(&[Rule::Unsafe]).passed());
    f.write("apps/a/src/lib.rs", "unsafe fn a() {}\nunsafe fn b() {}\n");
    assert!(!f.check(&[Rule::Unsafe]).passed());
}

#[test]
fn panicking_shortcuts_are_flagged() {
    let f = Fixture::new();
    f.write(
        "system/services/net/src/lib.rs",
        "fn f(x: Option<u8>, r: Result<u8, ()>) -> u8 {\n    let a = x.unwrap();\n    let b = r.expect(\"must work\");\n    if a > b { panic!(\"boom\"); }\n    todo!()\n}\nfn g() { unimplemented!(\"later\") }\nfn h(x: Option<u8>) -> u8 { x\n    .unwrap () }\n",
    );
    assert_eq!(
        f.lines(Rule::Unwrap),
        [
            "system/services/net/src/lib.rs:2: unwrap: `.unwrap()` outside test code",
            "system/services/net/src/lib.rs:3: unwrap: `.expect(` outside test code",
            "system/services/net/src/lib.rs:4: unwrap: `panic!` outside test code",
            "system/services/net/src/lib.rs:5: unwrap: `todo!` outside test code",
            "system/services/net/src/lib.rs:7: unwrap: `unimplemented!` outside test code",
            "system/services/net/src/lib.rs:9: unwrap: `.unwrap()` outside test code",
        ]
    );
}

#[test]
fn unwrap_rule_ignores_safe_variants_tests_and_text() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "fn f(x: Option<u8>) -> u8 {\n    // x.unwrap()\n    let _s = \"x.unwrap() panic!()\";\n    let _p = std::panic::catch_unwind(|| 1);\n    x.unwrap_or(0) + x.unwrap_or_default() + x.unwrap_or_else(|| 1)\n}\nfn expect(x: u8) -> u8 { x }\nfn unwrap() {}\n#![deny(clippy::unwrap_used, clippy::expect_used)]\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn t() { Some(1).unwrap(); panic!(\"x\"); }\n}\n#[test]\nfn top() { None::<u8>.expect(\"x\"); }\n",
    );
    f.write(
        "apps/a/tests/it.rs",
        "fn t() { Some(1).unwrap(); todo!() }\n",
    );
    f.write("apps/a/benches/b.rs", "fn b() { Some(1).unwrap(); }\n");
    assert_eq!(f.places(Rule::Unwrap), []);
}

#[test]
fn unwrap_baseline_ratchet() {
    let f = Fixture::new();
    f.write(
        "apps/a/src/lib.rs",
        "fn f(x: Option<u8>) { x.unwrap(); x.unwrap(); }\n",
    );
    f.write("apps/b/src/lib.rs", "fn f(x: Option<u8>) { x.unwrap(); }\n");
    f.baseline(&[("unwrap", "apps/a/src/lib.rs", 2)]);
    let report = f.check(&[Rule::Unwrap]);
    assert_eq!(report.baselined, 2);
    assert_eq!(report.violations.len(), 1);
    assert_eq!(report.violations[0].path, "apps/b/src/lib.rs");
    f.write("apps/a/src/lib.rs", "fn f(x: Option<u8>) { x.unwrap(); }\n");
    let report = f.check(&[Rule::Unwrap]);
    assert_eq!(report.notes[0].key, Key::BaselineCanLower);
}

#[test]
fn physical_sides_are_flagged_in_platform_and_apps() {
    let f = Fixture::new();
    let source = "pub struct Insets {\n    pub left: u32,\n    pub paddingRight: u32,\n    pub start: u32,\n}\nfn f(i: &Insets) -> u32 { i.left + i.left }\nconst MARGIN_LEFT: u32 = 1;\nfn leftover() -> u32 { let brightness = 1; brightness }\nlet s = \"left\"; // left in a comment\n";
    f.write("platform/shell-desktop/src/lib.rs", source);
    f.write("apps/phone/src/lib.rs", "fn align_right() {}\n");
    f.write("system/services/net/src/lib.rs", source);
    assert_eq!(
        f.lines(Rule::LogicalLayout),
        [
            "apps/phone/src/lib.rs:1: logical-layout: identifier `align_right` uses left/right; use start/end or mark the line `// physical`",
            "platform/shell-desktop/src/lib.rs:2: logical-layout: identifier `left` uses left/right; use start/end or mark the line `// physical`",
            "platform/shell-desktop/src/lib.rs:3: logical-layout: identifier `paddingRight` uses left/right; use start/end or mark the line `// physical`",
            "platform/shell-desktop/src/lib.rs:6: logical-layout: identifier `left` uses left/right; use start/end or mark the line `// physical`",
            "platform/shell-desktop/src/lib.rs:7: logical-layout: identifier `MARGIN_LEFT` uses left/right; use start/end or mark the line `// physical`",
        ]
    );
}

#[test]
fn physical_marker_and_tests_are_exempt() {
    let f = Fixture::new();
    f.write(
        "platform/ultar-compositor/src/lib.rs",
        "pub struct Rect {\n    pub left: i32, // physical\n    pub right: i32, /* physical screen edge */\n    // physical\n    pub top_left: i32,\n}\n#[cfg(test)]\nmod tests {\n    fn t() { let left = 1; let _ = left; }\n}\n",
    );
    assert_eq!(
        f.places(Rule::LogicalLayout),
        [at("platform/ultar-compositor/src/lib.rs", 5)]
    );
}

#[test]
fn logical_layout_baseline_ratchet() {
    let f = Fixture::new();
    f.write("apps/a/src/lib.rs", "fn f() { let left = 1; }\n");
    f.baseline(&[("logical-layout", "apps/a/src/lib.rs", 1)]);
    assert!(f.check(&[Rule::LogicalLayout]).passed());
    f.write(
        "apps/a/src/lib.rs",
        "fn f() { let left = 1; }\nfn g() { let right = 1; }\n",
    );
    assert!(!f.check(&[Rule::LogicalLayout]).passed());
}

#[test]
fn external_program_calls_are_counted() {
    let f = Fixture::new();
    f.write(
        "system/services/net/src/lib.rs",
        "use std::process::Command;\nfn f(tool: &str) {\n    Command::new(\"modprobe\").arg(\"wifi\");\n    std::process::Command::new(\"/usr/sbin/ip\");\n    Command::new(\"sh\").arg(\"-c\");\n    Command::new(\"ultar-ctl\");\n    Command::new(tool);\n    Command::new(\n        \"apt-get\",\n    );\n    // Command::new(\"dpkg\")\n    let _s = \"Command::new(\\\"nft\\\")\";\n}\n#[cfg(test)]\nmod tests {\n    fn t() { std::process::Command::new(\"iw\"); }\n}\n",
    );
    f.write(
        "tools/flash/src/main.rs",
        "fn f() { std::process::Command::new(\"cargo\"); }\n",
    );
    assert_eq!(
        f.lines(Rule::LinuxCalls),
        [
            "system/services/net/src/lib.rs:3: linux-calls: call of external program `modprobe`",
            "system/services/net/src/lib.rs:4: linux-calls: call of external program `ip`",
            "system/services/net/src/lib.rs:5: linux-calls: call of external program `sh`",
            "system/services/net/src/lib.rs:8: linux-calls: call of external program `apt-get`",
        ]
    );
}

#[test]
fn linux_calls_reduction_list_ratchet() {
    let f = Fixture::new();
    let call = |tool: &str| format!("fn {tool}() {{ std::process::Command::new(\"{tool}\"); }}\n");
    f.write(
        "system/services/net/src/lib.rs",
        &format!("{}{}", call("iw"), call("ip")),
    );
    f.baseline(&[("linux-calls", "system/services/net/src/lib.rs", 2)]);
    let report = f.check(&[Rule::LinuxCalls]);
    assert!(report.passed());
    assert_eq!(report.baselined, 2);

    f.write(
        "system/services/net/src/lib.rs",
        &format!("{}{}{}", call("iw"), call("ip"), call("nft")),
    );
    let report = f.check(&[Rule::LinuxCalls]);
    assert_eq!(report.violations.len(), 3);
    assert_eq!(
        report.notes[0].args,
        ["linux-calls", "system/services/net/src/lib.rs", "3", "2"]
    );

    f.write("system/services/net/src/lib.rs", &call("iw"));
    let report = f.check(&[Rule::LinuxCalls]);
    assert!(report.passed());
    assert_eq!(report.notes[0].key, Key::BaselineCanLower);
    assert_eq!(
        report.notes[0].args,
        ["linux-calls", "system/services/net/src/lib.rs", "2", "1"]
    );
}
