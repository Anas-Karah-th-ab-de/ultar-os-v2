//! Rule `layers`: every crate in one area, dependencies only in allowed directions
//! (plan §3.0, §7 rules 0 and 1).

use super::Rule;
use crate::error::CheckError;
use crate::finding::{Finding, Message};
use crate::layers_config::{LayersConfig, Verdict, CONFIG_PATH};
use crate::manifest::{self, Manifest, PathDependency};
use crate::msg::Key;
use crate::repo;
use std::path::Path;

/// Checks all manifests of the repository against the area rule table.
pub fn check(root: &Path) -> Result<Vec<Finding>, CheckError> {
    let config = load_config(root)?;
    let manifests = load_manifests(root)?;
    let mut findings = Vec::new();
    for manifest in manifests.iter().filter(|m| m.is_package) {
        if !config.is_excluded(&manifest.dir) {
            check_manifest(&config, manifest, &mut findings);
        }
    }
    Ok(findings)
}

fn load_config(root: &Path) -> Result<LayersConfig, CheckError> {
    match repo::read_optional(&root.join(CONFIG_PATH))? {
        Some(text) => LayersConfig::parse(CONFIG_PATH, &text),
        None => LayersConfig::builtin(),
    }
}

fn load_manifests(root: &Path) -> Result<Vec<Manifest>, CheckError> {
    let workspace = match repo::read_optional(&root.join(repo::MANIFEST_NAME))? {
        Some(text) => manifest::workspace_paths(&manifest::parse_toml(repo::MANIFEST_NAME, &text)?),
        None => manifest::WorkspacePaths::new(),
    };
    let mut manifests = Vec::new();
    for path in repo::walk(root)?.manifests {
        let text = repo::read_text(root, &path)?;
        manifests.push(manifest::parse_manifest(&path, &text, &workspace)?);
    }
    Ok(manifests)
}

fn check_manifest(config: &LayersConfig, manifest: &Manifest, findings: &mut Vec<Finding>) {
    let Some(area) = config.area_of(&manifest.dir) else {
        let shown = if manifest.dir.is_empty() {
            "."
        } else {
            manifest.dir.as_str()
        };
        let message = Message::new(Key::LayerNotInArea, &[shown]);
        findings.push(Finding::error(Rule::Layers, &manifest.path, 1, message));
        return;
    };
    for dependency in &manifest.dependencies {
        let key = dependency_problem(config, area, dependency);
        let message = match key {
            None => continue,
            Some(Key::LayerOutsideRepo) => Message::new(
                Key::LayerOutsideRepo,
                &[&dependency.name, &dependency.raw_path],
            ),
            Some(key) => {
                let target = dependency.target.as_deref().unwrap_or(&dependency.raw_path);
                Message::new(key, &[&dependency.name, target, area.name.as_str()])
            }
        };
        let finding = Finding::error(Rule::Layers, &manifest.path, dependency.line, message);
        findings.push(finding);
    }
}

/// Message key for a dependency that is not allowed, `None` if it is fine.
fn dependency_problem(
    config: &LayersConfig,
    area: &crate::layers_config::Area,
    dependency: &PathDependency,
) -> Option<Key> {
    let Some(target) = dependency.target.as_deref() else {
        return (!area.unrestricted).then_some(Key::LayerOutsideRepo);
    };
    if area.unrestricted {
        return None;
    }
    let Some(target_area) = config.area_of(target) else {
        return Some(Key::LayerForbidden);
    };
    match config.verdict(area, target_area, target) {
        Verdict::Allowed => None,
        Verdict::Upward => Some(Key::LayerUpward),
        Verdict::Forbidden => Some(Key::LayerForbidden),
    }
}
