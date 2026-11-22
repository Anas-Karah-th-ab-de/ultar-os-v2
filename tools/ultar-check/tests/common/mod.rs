//! Shared helpers: fixture repositories in temporary directories.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use ultar_check::cli::{Command, Options};
use ultar_check::msg::Lang;
use ultar_check::report::Report;
use ultar_check::rules::Rule;

static NEXT_ID: AtomicU32 = AtomicU32::new(0);

/// A throw-away repository tree, removed on drop.
pub struct Fixture {
    root: PathBuf,
}

impl Fixture {
    pub fn new() -> Fixture {
        let id = NEXT_ID.fetch_add(1, Ordering::SeqCst);
        let name = format!("ultar-check-test-{}-{id}", std::process::id());
        let root = std::env::temp_dir().join(name);
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        Fixture { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Writes a file (parents are created).
    pub fn write(&self, relative: &str, content: &str) -> &Fixture {
        self.write_bytes(relative, content.as_bytes())
    }

    pub fn write_bytes(&self, relative: &str, content: &[u8]) -> &Fixture {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
        self
    }

    /// Writes a minimal crate manifest with the given `[dependencies]` body.
    pub fn krate(&self, dir: &str, dependencies: &str) -> &Fixture {
        let name = dir.rsplit('/').next().unwrap();
        let text = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.7.0\"\nedition = \"2021\"\n\n[dependencies]\n{dependencies}"
        );
        self.write(&format!("{dir}/Cargo.toml"), &text)
    }

    /// Writes baseline entries to the default baseline file.
    pub fn baseline(&self, entries: &[(&str, &str, u32)]) -> &Fixture {
        let mut text = String::new();
        for (rule, path, count) in entries {
            text.push_str(&format!(
                "[[entry]]\nrule = \"{rule}\"\npath = \"{path}\"\ncount = {count}\n\n"
            ));
        }
        self.write("tools/check-baseline.toml", &text)
    }

    pub fn options(&self, rules: &[Rule]) -> Options {
        let mut options = Options::for_rules(&self.root, rules);
        options.lang = Lang::En;
        options
    }

    /// Runs the given rules with the default baseline location.
    pub fn check(&self, rules: &[Rule]) -> Report {
        ultar_check::execute(&self.options(rules)).unwrap()
    }

    /// Runs one rule and returns `path:line: rule: detail` lines of the violations.
    pub fn lines(&self, rule: Rule) -> Vec<String> {
        self.check(&[rule])
            .violations
            .iter()
            .map(|f| f.to_line(Lang::En))
            .collect()
    }

    /// Runs one rule and returns `(path, line)` of the violations.
    pub fn places(&self, rule: Rule) -> Vec<(String, u32)> {
        self.check(&[rule])
            .violations
            .iter()
            .map(|f| (f.path.clone(), f.line))
            .collect()
    }

    pub fn baseline_command(&self) -> Options {
        let mut options = self.options(&[]);
        options.command = Command::Baseline;
        options
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Shorthand for expected `(path, line)` pairs.
pub fn at(path: &str, line: u32) -> (String, u32) {
    (path.to_string(), line)
}
