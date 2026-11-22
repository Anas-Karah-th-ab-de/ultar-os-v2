//! Rule `budget`: size budgets of built release binaries (plan §3.6, §7 rule 0b).

use super::{Outcome, Rule};
use crate::error::CheckError;
use crate::finding::{Finding, Message};
use crate::msg::Key;
use crate::repo;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Repository-relative location of the budget table.
pub const CONFIG_PATH: &str = "tools/budgets.toml";

/// Cargo profile directory whose binaries are measured.
const PROFILE_DIR: &str = "release";

/// The budget table: binary name to maximum size in bytes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budgets {
    /// Maximum size in bytes per binary name.
    #[serde(default)]
    pub binaries: BTreeMap<String, u64>,
}

impl Budgets {
    /// Parses a budget table; `path` is used in error messages.
    pub fn parse(path: &str, text: &str) -> Result<Budgets, CheckError> {
        toml::from_str(text).map_err(|source| CheckError::Toml {
            path: path.to_string(),
            source,
        })
    }
}

/// Compares built binaries below `target_dir` with the budget table.
///
/// Binaries are looked up in `<target>/release/` and `<target>/<triple>/release/`.
/// A missing table or a binary that is not built is skipped with a note.
pub fn check(root: &Path, target_dir: &Path) -> Result<Outcome, CheckError> {
    let mut outcome = Outcome::default();
    let Some(text) = repo::read_optional(&root.join(CONFIG_PATH))? else {
        outcome
            .notes
            .push(Message::new(Key::BudgetNoConfig, &[CONFIG_PATH]));
        return Ok(outcome);
    };
    let budgets = Budgets::parse(CONFIG_PATH, &text)?;
    let profile_dirs = profile_dirs(target_dir);
    for (name, max_bytes) in &budgets.binaries {
        let built: Vec<(PathBuf, u64)> = profile_dirs
            .iter()
            .map(|dir| dir.join(name))
            .filter_map(|path| file_size(&path).map(|size| (path, size)))
            .collect();
        if built.is_empty() {
            outcome
                .notes
                .push(Message::new(Key::BudgetNotBuilt, &[name]));
        }
        for (path, size) in built.into_iter().filter(|(_, size)| size > max_bytes) {
            let shown = path
                .strip_prefix(target_dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let args = [shown, size.to_string(), max_bytes.to_string()];
            let message = Message::new(Key::BudgetExceeded, &args);
            outcome
                .findings
                .push(Finding::error(Rule::Budget, name, 0, message));
        }
    }
    Ok(outcome)
}

/// `<target>/release` plus `<target>/<triple>/release` for every sub directory, sorted.
fn profile_dirs(target_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![target_dir.join(PROFILE_DIR)];
    let mut per_triple: Vec<PathBuf> = fs::read_dir(target_dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path().join(PROFILE_DIR))
        .filter(|path| path.is_dir())
        .collect();
    per_triple.sort();
    dirs.append(&mut per_triple);
    dirs
}

fn file_size(path: &Path) -> Option<u64> {
    let metadata = fs::metadata(path).ok()?;
    metadata.is_file().then_some(metadata.len())
}
