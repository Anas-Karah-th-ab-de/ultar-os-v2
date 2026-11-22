//! Reading `Cargo.toml` files: packages and their path dependencies.

use crate::error::CheckError;
use std::collections::BTreeMap;
use toml::{Table, Value};

/// Tables of a manifest that hold dependencies.
const DEPENDENCY_TABLES: [&str; 3] = ["dependencies", "dev-dependencies", "build-dependencies"];

/// A dependency on another crate of the repository, given by path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDependency {
    /// Dependency name as written in the manifest.
    pub name: String,
    /// Normalized repository-relative directory, `None` if it leaves the repository.
    pub target: Option<String>,
    /// Path exactly as written (or as inherited from the workspace).
    pub raw_path: String,
    /// 1-based line of the dependency in the manifest (1 if not found).
    pub line: u32,
}

/// One parsed `Cargo.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// Repository-relative path of the manifest.
    pub path: String,
    /// Repository-relative directory of the crate (`""` for the root).
    pub dir: String,
    /// Whether the manifest defines a package (not only a workspace).
    pub is_package: bool,
    /// Path dependencies of the package.
    pub dependencies: Vec<PathDependency>,
}

/// Paths of `[workspace.dependencies]` entries, keyed by dependency name.
pub type WorkspacePaths = BTreeMap<String, String>;

/// Parses TOML text, attributing errors to `path`.
pub fn parse_toml(path: &str, text: &str) -> Result<Table, CheckError> {
    text.parse::<Table>().map_err(|source| CheckError::Toml {
        path: path.to_string(),
        source,
    })
}

/// Extracts the path entries of `[workspace.dependencies]` from the root manifest.
pub fn workspace_paths(root_manifest: &Table) -> WorkspacePaths {
    let mut paths = WorkspacePaths::new();
    let dependencies = root_manifest
        .get("workspace")
        .and_then(Value::as_table)
        .and_then(|w| w.get("dependencies"))
        .and_then(Value::as_table);
    for (name, value) in dependencies.into_iter().flatten() {
        if let Some(path) = value.get("path").and_then(Value::as_str) {
            paths.insert(name.clone(), path.to_string());
        }
    }
    paths
}

/// Parses one manifest; `workspace` resolves `workspace = true` dependencies.
pub fn parse_manifest(
    path: &str,
    text: &str,
    workspace: &WorkspacePaths,
) -> Result<Manifest, CheckError> {
    let table = parse_toml(path, text)?;
    let dir = path
        .rsplit_once('/')
        .map(|(dir, _)| dir.to_string())
        .unwrap_or_default();
    let mut dependencies = Vec::new();
    for dependency_table in dependency_tables(&table) {
        for (name, value) in dependency_table {
            let inherited = value.get("workspace").and_then(Value::as_bool) == Some(true);
            let (base, raw_path) = match value.get("path").and_then(Value::as_str) {
                Some(raw) => (dir.as_str(), raw),
                None => match workspace.get(name).filter(|_| inherited) {
                    Some(raw) => ("", raw.as_str()),
                    None => continue,
                },
            };
            dependencies.push(PathDependency {
                name: name.clone(),
                target: normalize(base, raw_path),
                raw_path: raw_path.to_string(),
                line: dependency_line(text, name),
            });
        }
    }
    Ok(Manifest {
        path: path.to_string(),
        dir,
        is_package: table.contains_key("package"),
        dependencies,
    })
}

fn dependency_tables(manifest: &Table) -> Vec<&Table> {
    let mut holders = vec![manifest];
    let targets = manifest.get("target").and_then(Value::as_table);
    holders.extend(
        targets
            .into_iter()
            .flatten()
            .filter_map(|(_, per_target)| per_target.as_table()),
    );
    let mut tables = Vec::new();
    for holder in holders {
        for key in DEPENDENCY_TABLES {
            if let Some(found) = holder.get(key).and_then(Value::as_table) {
                tables.push(found);
            }
        }
    }
    tables
}

/// Joins `base` and `relative` lexically; `None` if the result leaves the repository.
pub fn normalize(base: &str, relative: &str) -> Option<String> {
    let unified = relative.replace('\\', "/");
    if unified.starts_with('/') || unified.contains(':') {
        return None;
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in unified.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    Some(parts.join("/"))
}

/// Finds the line that declares dependency `name` (best effort, 1 if unknown).
fn dependency_line(text: &str, name: &str) -> u32 {
    let header_end = format!(".{name}]");
    let position = text.lines().position(|line| {
        let trimmed = line.trim_start();
        let key_line = trimmed
            .strip_prefix(name)
            .or_else(|| strip_quoted(trimmed, name))
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with('=') || rest.starts_with('.'));
        key_line || (trimmed.starts_with('[') && trimmed.trim_end().ends_with(&header_end))
    });
    position
        .and_then(|index| u32::try_from(index + 1).ok())
        .unwrap_or(1)
}

fn strip_quoted<'a>(line: &'a str, name: &str) -> Option<&'a str> {
    line.strip_prefix('"')?
        .strip_prefix(name)?
        .strip_prefix('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_resolves_dots_and_rejects_escapes() {
        assert_eq!(
            normalize("apps/a", "../../platform/ui").as_deref(),
            Some("platform/ui")
        );
        assert_eq!(
            normalize("apps/a", "./sub//x/").as_deref(),
            Some("apps/a/sub/x")
        );
        assert_eq!(
            normalize("", "system/core/bus").as_deref(),
            Some("system/core/bus")
        );
        assert_eq!(normalize("apps/a", "..\\b").as_deref(), Some("apps/b"));
        assert_eq!(normalize("apps/a", "../../../x"), None);
        assert_eq!(normalize("apps/a", "/abs"), None);
        assert_eq!(normalize("apps/a", "C:/x"), None);
    }

    #[test]
    fn all_dependency_tables_are_read() {
        let text = r#"
[package]
name = "a"

[dependencies]
serde = "1"
one = { path = "../one" }

[dev-dependencies.two]
path = "../two"

[build-dependencies]
"three" = { path = "../three", version = "1" }

[target.'cfg(unix)'.dependencies]
four = { path = "../four" }
"#;
        let manifest = parse_manifest("apps/a/Cargo.toml", text, &WorkspacePaths::new()).unwrap();
        assert!(manifest.is_package);
        assert_eq!(manifest.dir, "apps/a");
        let mut seen: Vec<(String, String, u32)> = manifest
            .dependencies
            .iter()
            .map(|d| (d.name.clone(), d.target.clone().unwrap(), d.line))
            .collect();
        seen.sort();
        let expected = [
            ("four", "apps/four", 16),
            ("one", "apps/one", 7),
            ("three", "apps/three", 13),
            ("two", "apps/two", 9),
        ];
        assert_eq!(seen.len(), expected.len());
        for (got, want) in seen.iter().zip(expected) {
            assert_eq!((got.0.as_str(), got.1.as_str(), got.2), want);
        }
    }

    #[test]
    fn workspace_inheritance_resolves_from_the_root() {
        let root = parse_toml(
            "Cargo.toml",
            "[workspace.dependencies]\nbus = { path = \"system/core/ultar-bus\" }\nserde = \"1\"\n",
        )
        .unwrap();
        let workspace = workspace_paths(&root);
        assert_eq!(workspace.len(), 1);
        let text = "[package]\nname = \"a\"\n[dependencies]\nbus = { workspace = true }\nserde.workspace = true\nbus2 = { workspace = false }\n";
        let manifest = parse_manifest("apps/a/Cargo.toml", text, &workspace).unwrap();
        assert_eq!(manifest.dependencies.len(), 1);
        let dependency = &manifest.dependencies[0];
        assert_eq!(dependency.target.as_deref(), Some("system/core/ultar-bus"));
        assert_eq!(dependency.line, 4);
    }

    #[test]
    fn workspace_only_manifest_is_not_a_package() {
        let manifest = parse_manifest(
            "Cargo.toml",
            "[workspace]\nmembers = []\n",
            &WorkspacePaths::new(),
        )
        .unwrap();
        assert!(!manifest.is_package);
        assert_eq!(manifest.dir, "");
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn invalid_toml_is_a_typed_error() {
        let error = parse_manifest("x/Cargo.toml", "[package", &WorkspacePaths::new()).unwrap_err();
        assert!(matches!(error, CheckError::Toml { ref path, .. } if path == "x/Cargo.toml"));
    }
}
