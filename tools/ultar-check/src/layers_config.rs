//! Area and dependency rule table for the `layers` check (`tools/check-layers.toml`).

use crate::error::{CheckError, ConfigIssue};
use serde::Deserialize;

/// Repository-relative location of the optional configuration file.
pub const CONFIG_PATH: &str = "tools/check-layers.toml";

/// Built-in rule table, used when the repository has no configuration file.
pub const DEFAULT_CONFIG: &str = include_str!("../defaults/check-layers.toml");

/// One area of the repository (plan §3.0).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Area {
    /// Area name used by allow entries and in messages.
    pub name: String,
    /// Path prefix of the area folder, ending in `/`.
    pub prefix: String,
    /// Layer rank; a dependency on a higher rank is "upward".
    pub rank: u32,
    /// Whether crates of the area may depend on each other.
    #[serde(default)]
    pub internal: bool,
    /// Whether crates of the area may depend on anything.
    #[serde(default)]
    pub unrestricted: bool,
}

/// One allowed dependency direction.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Allow {
    /// Area of the depending crate.
    pub from: String,
    /// Area of the dependency.
    pub to: String,
    /// Allowed target crate directories; empty means the whole area.
    #[serde(default)]
    pub crates: Vec<String>,
}

/// The complete rule table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LayersConfig {
    /// Path prefixes that are not checked.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// Areas of the repository.
    #[serde(default)]
    pub area: Vec<Area>,
    /// Allowed dependencies between areas.
    #[serde(default)]
    pub allow: Vec<Allow>,
}

/// Verdict on one dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The dependency is allowed.
    Allowed,
    /// Not allowed, and the target is in a higher layer.
    Upward,
    /// Not allowed for another reason.
    Forbidden,
}

impl LayersConfig {
    /// Parses and validates a rule table; `path` is used in error messages.
    pub fn parse(path: &str, text: &str) -> Result<LayersConfig, CheckError> {
        let config: LayersConfig = toml::from_str(text).map_err(|source| CheckError::Toml {
            path: path.to_string(),
            source,
        })?;
        config.validate(path)?;
        Ok(config)
    }

    /// The built-in rule table.
    pub fn builtin() -> Result<LayersConfig, CheckError> {
        LayersConfig::parse("<builtin check-layers.toml>", DEFAULT_CONFIG)
    }

    fn validate(&self, path: &str) -> Result<(), CheckError> {
        let fail = |issue| {
            Err(CheckError::Config {
                path: path.to_string(),
                issue,
            })
        };
        for (i, area) in self.area.iter().enumerate() {
            if self.area.iter().skip(i + 1).any(|a| a.name == area.name) {
                return fail(ConfigIssue::DuplicateArea(area.name.clone()));
            }
        }
        for allow in &self.allow {
            for name in [&allow.from, &allow.to] {
                if !self.area.iter().any(|a| &a.name == name) {
                    return fail(ConfigIssue::UnknownArea(name.clone()));
                }
            }
        }
        Ok(())
    }

    /// Whether a crate directory is excluded from the check.
    pub fn is_excluded(&self, dir: &str) -> bool {
        self.exclude.iter().any(|p| is_at_or_below(dir, p))
    }

    /// The area a crate directory belongs to (longest matching prefix).
    pub fn area_of(&self, dir: &str) -> Option<&Area> {
        let with_slash = format!("{dir}/");
        self.area
            .iter()
            .filter(|a| with_slash.starts_with(&a.prefix))
            .max_by_key(|a| a.prefix.len())
    }

    /// Decides whether a crate in `from` may depend on the crate at `target_dir` in `to`.
    pub fn verdict(&self, from: &Area, to: &Area, target_dir: &str) -> Verdict {
        if from.unrestricted || (from.name == to.name && from.internal) {
            return Verdict::Allowed;
        }
        let allowed = self.allow.iter().any(|allow| {
            allow.from == from.name
                && allow.to == to.name
                && (allow.crates.is_empty()
                    || allow.crates.iter().any(|c| is_at_or_below(target_dir, c)))
        });
        if allowed {
            Verdict::Allowed
        } else if to.rank > from.rank {
            Verdict::Upward
        } else {
            Verdict::Forbidden
        }
    }
}

/// Whether `dir` equals `prefix` or lies below it (trailing slashes ignored).
fn is_at_or_below(dir: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    dir == prefix
        || dir
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verdict(config: &LayersConfig, from: &str, to: &str) -> Verdict {
        let from_area = config.area_of(from).unwrap();
        let to_area = config.area_of(to).unwrap();
        config.verdict(from_area, to_area, to)
    }

    #[test]
    fn builtin_table_is_valid_and_covers_all_areas() {
        let config = LayersConfig::builtin().unwrap();
        let names: Vec<&str> = config.area.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "kernel",
                "drivers",
                "system-core",
                "system-hal",
                "system-services",
                "platform",
                "apps",
                "external",
                "tools"
            ]
        );
    }

    #[test]
    fn area_lookup_uses_folder_prefixes() {
        let config = LayersConfig::builtin().unwrap();
        assert_eq!(
            config.area_of("system/core/ultar-bus").unwrap().name,
            "system-core"
        );
        assert_eq!(
            config.area_of("system/hal/hal-x/sub").unwrap().name,
            "system-hal"
        );
        assert_eq!(config.area_of("kernel").unwrap().name, "kernel");
        assert!(config.area_of("system/ultar-x").is_none());
        assert!(config.area_of("kernelx/a").is_none());
        assert!(config.area_of("").is_none());
        assert!(config.area_of("devices/a30").is_none());
    }

    fn assert_verdicts(cases: &[(&str, &str, Verdict)]) {
        let config = LayersConfig::builtin().unwrap();
        for (from, to, expected) in cases {
            assert_eq!(verdict(&config, from, to), *expected, "{from} -> {to}");
        }
    }

    #[test]
    fn builtin_directions_for_apps_and_platform() {
        use Verdict::{Allowed, Forbidden, Upward};
        assert_verdicts(&[
            ("apps/phone", "platform/app-api", Allowed),
            ("apps/phone", "platform/ultar-ui", Allowed),
            ("apps/phone", "platform/ultar-compositor", Forbidden),
            ("apps/phone", "system/core/ultar-bus", Allowed),
            ("apps/phone", "system/core/ultar-init", Forbidden),
            ("apps/phone", "system/hal/ultar-hal", Forbidden),
            ("apps/phone", "apps/camera", Forbidden),
            ("apps/phone", "tools/ultar-check", Upward),
            ("external/telegram", "platform/app-api", Allowed),
            ("external/telegram", "system/core/ultar-init", Forbidden),
            ("platform/shell-mobile", "platform/ultar-ui", Allowed),
            ("platform/ultar-ui", "system/core/ultar-i18n", Allowed),
            ("platform/ultar-ui", "system/hal/ultar-hal", Allowed),
            ("platform/ultar-ui", "system/hal/hal-exynos7885", Forbidden),
            ("platform/ultar-ui", "apps/phone", Upward),
            ("tools/ultar-check", "apps/phone", Allowed),
        ]);
    }

    #[test]
    fn builtin_directions_for_system_drivers_and_kernel() {
        use Verdict::{Allowed, Forbidden, Upward};
        assert_verdicts(&[
            ("system/services/net", "system/core/ultar-bus", Allowed),
            ("system/services/net", "system/hal/ultar-hal", Allowed),
            (
                "system/services/net",
                "system/hal/hal-generic-linux",
                Forbidden,
            ),
            ("system/services/net", "system/services/audio", Forbidden),
            ("system/services/net", "platform/ultar-ui", Upward),
            (
                "system/hal/hal-generic-linux",
                "system/hal/ultar-hal",
                Allowed,
            ),
            (
                "system/hal/hal-generic-linux",
                "system/core/ultar-log",
                Allowed,
            ),
            (
                "system/hal/hal-generic-linux",
                "system/hal/hal-exynos7885",
                Forbidden,
            ),
            ("system/core/ultar-init", "system/core/ultar-bus", Allowed),
            ("system/core/ultar-init", "system/hal/ultar-hal", Upward),
            (
                "drivers/kernel/nvme",
                "system/core/ultar-driverpkg",
                Allowed,
            ),
            ("drivers/kernel/nvme", "system/core/ultar-bus", Upward),
            ("drivers/depot", "drivers/kernel/nvme", Allowed),
            ("kernel/ultar-kern", "drivers/kernel/nvme", Allowed),
            ("kernel/ultar-kern", "drivers/depot", Upward),
            ("kernel/ultar-kern", "system/core/ultar-bus", Upward),
            ("kernel/ultar-kern", "kernel/abi", Allowed),
        ]);
    }

    #[test]
    fn invalid_tables_are_rejected() {
        let duplicate =
            "[[area]]\nname='a'\nprefix='a/'\nrank=1\n[[area]]\nname='a'\nprefix='b/'\nrank=2\n";
        let error = LayersConfig::parse("t.toml", duplicate).unwrap_err();
        assert!(matches!(
            error,
            CheckError::Config { issue: ConfigIssue::DuplicateArea(ref n), .. } if n == "a"
        ));
        let unknown = "[[area]]\nname='a'\nprefix='a/'\nrank=1\n[[allow]]\nfrom='a'\nto='moon'\n";
        let error = LayersConfig::parse("t.toml", unknown).unwrap_err();
        assert!(matches!(
            error,
            CheckError::Config { issue: ConfigIssue::UnknownArea(ref n), .. } if n == "moon"
        ));
        let typo = "[[area]]\nname='a'\nprefix='a/'\nrank=1\nintrenal=true\n";
        assert!(matches!(
            LayersConfig::parse("t.toml", typo).unwrap_err(),
            CheckError::Toml { .. }
        ));
    }

    #[test]
    fn excludes_match_whole_path_segments() {
        let config = LayersConfig::parse("t.toml", "exclude = ['legacy/', 'apps/old']\n").unwrap();
        assert!(config.is_excluded("legacy/x"));
        assert!(config.is_excluded("apps/old"));
        assert!(!config.is_excluded("apps/older"));
        assert!(!config.is_excluded("legacyx"));
    }
}
