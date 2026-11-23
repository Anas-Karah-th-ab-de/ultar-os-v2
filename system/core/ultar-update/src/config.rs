//! The daemon's configuration file (`key=value` lines, `#` comments).
//!
//! The file is part of the system image (it states what the image *is*: device, edition,
//! version, rollback index) plus the device's set-up (paths, server, who is administrator).
//! Unknown keys and repeated single keys are errors: a typo must not silently switch a
//! protection off.

use crate::engine::{EngineConfig, DEFAULT_RESERVE, DEFAULT_TRIAL_TRIES};
use crate::manifest::{valid_ident, Role};
use crate::service::Access;
use crate::verify::{DeviceIdentity, DEFAULT_EXPIRY_TOLERANCE};
use crate::version::Version;
use std::fmt;
use std::path::PathBuf;

/// What is wrong with a configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigErrorKind {
    /// A line is not `key=value`.
    Syntax,
    /// The key is not known.
    UnknownKey,
    /// A key that may appear once appears again.
    Repeated,
    /// The value is not valid for its key.
    Value,
    /// A required key is missing (line 0).
    Missing(&'static str),
}

/// A configuration error with its line (1-based; 0 for missing keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    /// Line number.
    pub line: usize,
    /// What is wrong.
    pub kind: ConfigErrorKind,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            ConfigErrorKind::Syntax => f.write_str("syntax"),
            ConfigErrorKind::UnknownKey => f.write_str("unknown key"),
            ConfigErrorKind::Repeated => f.write_str("repeated key"),
            ConfigErrorKind::Value => f.write_str("invalid value"),
            ConfigErrorKind::Missing(key) => write!(f, "missing {key}"),
        }
    }
}

impl std::error::Error for ConfigError {}

/// Everything the daemon needs to know.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// Engine configuration.
    pub engine: EngineConfig,
    /// Where the slots and the slot table live.
    pub boot: BootStore,
    /// Payload roles of a slot.
    pub roles: Vec<Role>,
    /// Update server, when the device updates online.
    pub server: Option<String>,
    /// File with the session token for gated channels.
    pub token_file: Option<PathBuf>,
    /// Permissions.
    pub access: Access,
    /// Services that must answer on the bus before a new release is confirmed.
    pub health_services: Vec<String>,
    /// How long to wait for those services after a start, in seconds.
    pub health_wait_seconds: u32,
    /// Pause between automatic checks, in minutes.
    pub check_interval_minutes: u32,
}

/// Which boot control backend the daemon uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BootStore {
    /// `boot_dir=`: the file-based boot control ([`crate::filectl::FileBootControl`]).
    Files(PathBuf),
    /// `partition_dir=` + `running_slot_file=`: raw partitions of the A/B layout
    /// ([`crate::partctl::PartitionBootControl`]).
    Partitions {
        /// Directory with one node per partition, named by its GPT name.
        dir: PathBuf,
        /// File in which the kernel names the running slot (`a`, `b`, or `-`).
        running_slot_file: PathBuf,
    },
}

#[derive(Default)]
struct Raw {
    single: Vec<(&'static str, String)>,
    admins: Vec<String>,
    offline_roots: Vec<PathBuf>,
    health_services: Vec<String>,
}

const SINGLE_KEYS: [&str; 20] = [
    "image_time",
    "device",
    "arch",
    "edition",
    "version",
    "rollback_index",
    "roles",
    "boot_dir",
    "partition_dir",
    "running_slot_file",
    "state_dir",
    "keys_dir",
    "server",
    "token_file",
    "network_identity",
    "trial_tries",
    "reserve_mib",
    "expiry_tolerance_hours",
    "health_wait_seconds",
    "check_interval_minutes",
];

impl Raw {
    fn get(&self, key: &str) -> Option<&str> {
        self.single
            .iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.as_str())
    }

    fn required(&self, key: &'static str) -> Result<&str, ConfigError> {
        self.get(key).ok_or(ConfigError {
            line: 0,
            kind: ConfigErrorKind::Missing(key),
        })
    }

    fn number<T: std::str::FromStr>(
        &self,
        key: &'static str,
        default: T,
    ) -> Result<T, ConfigError> {
        match self.get(key) {
            None => Ok(default),
            Some(text) => text.parse().map_err(|_| value_error(0)),
        }
    }
}

fn value_error(line: usize) -> ConfigError {
    ConfigError {
        line,
        kind: ConfigErrorKind::Value,
    }
}

fn read(text: &str) -> Result<Raw, ConfigError> {
    let mut raw = Raw::default();
    for (index, line) in text.lines().enumerate() {
        let line_no = index + 1;
        let error = |kind| ConfigError {
            line: line_no,
            kind,
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(error(ConfigErrorKind::Syntax))?;
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            return Err(error(ConfigErrorKind::Value));
        }
        match key {
            "admin" => raw.admins.push(value.to_string()),
            "offline_root" => raw.offline_roots.push(PathBuf::from(value)),
            "health_service" => raw.health_services.push(value.to_string()),
            _ => {
                let known = SINGLE_KEYS.iter().find(|k| **k == key);
                let known = known.ok_or(error(ConfigErrorKind::UnknownKey))?;
                if raw.get(key).is_some() {
                    return Err(error(ConfigErrorKind::Repeated));
                }
                raw.single.push((known, value.to_string()));
            }
        }
    }
    Ok(raw)
}

/// The services whose answer decides whether a new release is confirmed. There is no
/// default: with nothing to ask, every start would count as healthy, the rollback floor
/// would rise and the old slot be retired as soon as the daemon runs. An image that really
/// has nothing to check must say so with the single entry `none`.
fn health_services(raw: &Raw) -> Result<Vec<String>, ConfigError> {
    let listed = &raw.health_services;
    if listed.is_empty() {
        return Err(ConfigError {
            line: 0,
            kind: ConfigErrorKind::Missing("health_service"),
        });
    }
    if listed.len() == 1 && listed[0] == "none" {
        return Ok(Vec::new());
    }
    if listed
        .iter()
        .all(|name| ultar_bus::is_valid_segment_name(name) && name != "none")
    {
        Ok(listed.clone())
    } else {
        Err(value_error(0))
    }
}

/// Exactly one backend: `boot_dir`, or `partition_dir` together with `running_slot_file`.
fn boot_store(raw: &Raw) -> Result<BootStore, ConfigError> {
    let files = raw.get("boot_dir");
    let parts = raw.get("partition_dir");
    let slot_file = raw.get("running_slot_file");
    match (files, parts, slot_file) {
        (Some(dir), None, None) => Ok(BootStore::Files(PathBuf::from(dir))),
        (None, Some(dir), Some(file)) => Ok(BootStore::Partitions {
            dir: PathBuf::from(dir),
            running_slot_file: PathBuf::from(file),
        }),
        (None, None, None) => Err(ConfigError {
            line: 0,
            kind: ConfigErrorKind::Missing("boot_dir"),
        }),
        (None, Some(_), None) => Err(ConfigError {
            line: 0,
            kind: ConfigErrorKind::Missing("running_slot_file"),
        }),
        _ => Err(value_error(0)),
    }
}

fn parse_roles(text: &str) -> Result<Vec<Role>, ConfigError> {
    let mut roles = Vec::new();
    for name in text.split(',') {
        roles.push(Role::from_name(name.trim()).ok_or(value_error(0))?);
    }
    roles.sort_unstable();
    roles.dedup();
    Ok(roles)
}

impl DaemonConfig {
    /// Parses a configuration file.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let raw = read(text)?;
        let ident = |key: &'static str| -> Result<String, ConfigError> {
            let value = raw.required(key)?;
            valid_ident(value)
                .then(|| value.to_string())
                .ok_or(value_error(0))
        };
        let device = DeviceIdentity {
            arch: ident("arch")?,
            edition: ident("edition")?,
            device: ident("device")?,
        };
        let version = Version::parse(raw.required("version")?).ok_or(value_error(0))?;
        let trial_tries: u8 = raw.number("trial_tries", DEFAULT_TRIAL_TRIES)?;
        if trial_tries == 0 {
            return Err(value_error(0));
        }
        let engine = EngineConfig {
            state_dir: PathBuf::from(raw.required("state_dir")?),
            keys_dir: PathBuf::from(raw.required("keys_dir")?),
            device,
            factory_version: version,
            factory_rollback_index: raw.number("rollback_index", 0u64)?,
            trial_tries,
            reserve_bytes: raw
                .number("reserve_mib", DEFAULT_RESERVE / (1024 * 1024))?
                .saturating_mul(1024 * 1024),
            expiry_tolerance: raw
                .number("expiry_tolerance_hours", DEFAULT_EXPIRY_TOLERANCE / 3600)?
                .saturating_mul(3600),
            image_time: raw.number("image_time", 0u64)?,
        };
        Ok(Self {
            engine,
            boot: boot_store(&raw)?,
            roles: parse_roles(raw.required("roles")?)?,
            server: raw.get("server").map(str::to_string),
            token_file: raw.get("token_file").map(PathBuf::from),
            access: Access {
                admins: raw.admins.clone(),
                network_identity: raw.get("network_identity").map(str::to_string),
                offline_roots: raw.offline_roots.clone(),
            },
            health_services: health_services(&raw)?,
            health_wait_seconds: raw.number("health_wait_seconds", 120)?,
            check_interval_minutes: raw.number("check_interval_minutes", 360u32)?.max(15),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GOOD: &str = "\
# what this image is
device = pc-uefi
arch=x86_64
edition=desktop
version=0.7.0
rollback_index=4
roles=system, kernel

boot_dir=/boot/ultar
state_dir=/var/lib/ultar/update
keys_dir=/etc/ultar/update-keys
server=https://ultar.code-craft.tech
admin=system.settings
admin=system.admin
network_identity=system.net
offline_root=/media
health_service=settings
health_service=devices
";

    #[test]
    fn parses_a_full_file_with_defaults() {
        let c = DaemonConfig::parse(GOOD).unwrap();
        assert_eq!(c.engine.device.device, "pc-uefi");
        assert_eq!(c.engine.factory_version, Version::new(0, 7, 0));
        assert_eq!(c.engine.factory_rollback_index, 4);
        assert_eq!(c.engine.trial_tries, 3);
        assert_eq!(c.engine.reserve_bytes, 64 * 1024 * 1024);
        assert_eq!(c.engine.expiry_tolerance, 86_400);
        assert_eq!(c.roles, [Role::Kernel, Role::System]);
        assert_eq!(c.boot, BootStore::Files(PathBuf::from("/boot/ultar")));
        assert_eq!(c.access.admins, ["system.settings", "system.admin"]);
        assert_eq!(c.access.network_identity.as_deref(), Some("system.net"));
        assert_eq!(c.health_services.len(), 2);
        assert_eq!(
            (c.health_wait_seconds, c.check_interval_minutes),
            (120, 360)
        );
        assert!(c.token_file.is_none());
    }

    #[test]
    fn minimal_file_denies_everything_by_default() {
        let text = "device=d\narch=a\nedition=e\nversion=1.0.0\nroles=kernel\nboot_dir=/b\nstate_dir=/s\nkeys_dir=/k\nhealth_service=none\n";
        let c = DaemonConfig::parse(text).unwrap();
        assert!(c.access.admins.is_empty());
        assert!(c.access.network_identity.is_none());
        assert!(c.access.offline_roots.is_empty());
        assert!(c.server.is_none());
        assert_eq!(c.engine.factory_rollback_index, 0);
        assert_eq!(c.engine.image_time, 0);
        assert!(c.health_services.is_empty(), "explicit none");
    }

    #[test]
    fn health_services_must_be_named_or_explicitly_none() {
        let base = "device=d\narch=a\nedition=e\nversion=1.0.0\nroles=kernel\nboot_dir=/b\nstate_dir=/s\nkeys_dir=/k\n";
        let missing = DaemonConfig::parse(base).unwrap_err();
        assert_eq!(missing.kind, ConfigErrorKind::Missing("health_service"));
        for bad in ["none\nhealth_service=settings", "two words", "a.b", "../x"] {
            let text = format!("{base}health_service={bad}\n");
            assert_eq!(
                DaemonConfig::parse(&text).unwrap_err().kind,
                ConfigErrorKind::Value,
                "{bad}"
            );
        }
        let text = format!("{base}health_service=settings\nimage_time=1795000000\n");
        let c = DaemonConfig::parse(&text).unwrap();
        assert_eq!(c.health_services, ["settings"]);
        assert_eq!(c.engine.image_time, 1_795_000_000);
    }

    #[test]
    fn errors_name_the_line() {
        let check = |text: String, line: usize, kind: ConfigErrorKind| {
            assert_eq!(
                DaemonConfig::parse(&text).unwrap_err(),
                ConfigError { line, kind }
            );
        };
        check(
            format!("{GOOD}colour=blue\n"),
            19,
            ConfigErrorKind::UnknownKey,
        );
        check(format!("{GOOD}arch=arm\n"), 19, ConfigErrorKind::Repeated);
        check(format!("{GOOD}just words\n"), 19, ConfigErrorKind::Syntax);
        check(format!("{GOOD}server=\n"), 19, ConfigErrorKind::Value);
        check(
            GOOD.replace("version=0.7.0\n", ""),
            0,
            ConfigErrorKind::Missing("version"),
        );
        check(
            GOOD.replace("keys_dir=/etc/ultar/update-keys\n", ""),
            0,
            ConfigErrorKind::Missing("keys_dir"),
        );
        check(
            GOOD.replace("version=0.7.0", "version=seven"),
            0,
            ConfigErrorKind::Value,
        );
        check(
            GOOD.replace("roles=system, kernel", "roles=system,initrd"),
            0,
            ConfigErrorKind::Value,
        );
        check(
            GOOD.replace("device = pc-uefi", "device = ../pc"),
            0,
            ConfigErrorKind::Value,
        );
        check(format!("{GOOD}trial_tries=0\n"), 0, ConfigErrorKind::Value);
        check(
            GOOD.replace("rollback_index=4", "rollback_index=-1"),
            0,
            ConfigErrorKind::Value,
        );
        assert!(ConfigError {
            line: 0,
            kind: ConfigErrorKind::Missing("x")
        }
        .to_string()
        .contains("missing x"));
    }

    #[test]
    fn check_interval_has_a_lower_bound() {
        let c = DaemonConfig::parse(&format!("{GOOD}check_interval_minutes=1\n")).unwrap();
        assert_eq!(c.check_interval_minutes, 15);
    }

    #[test]
    fn exactly_one_boot_backend() {
        let parts = "partition_dir=/parts\nrunning_slot_file=/slot\n";
        let text = GOOD.replace("boot_dir=/boot/ultar\n", parts);
        let c = DaemonConfig::parse(&text).unwrap();
        assert_eq!(
            c.boot,
            BootStore::Partitions {
                dir: PathBuf::from("/parts"),
                running_slot_file: PathBuf::from("/slot"),
            }
        );
        let kind = |t: String| DaemonConfig::parse(&t).unwrap_err().kind;
        assert_eq!(kind(format!("{GOOD}{parts}")), ConfigErrorKind::Value);
        assert_eq!(
            kind(GOOD.replace("boot_dir=/boot/ultar\n", "partition_dir=/p\n")),
            ConfigErrorKind::Missing("running_slot_file")
        );
        assert_eq!(
            kind(GOOD.replace("boot_dir=/boot/ultar\n", "running_slot_file=/s\n")),
            ConfigErrorKind::Value
        );
        assert_eq!(
            kind(GOOD.replace("boot_dir=/boot/ultar\n", "")),
            ConfigErrorKind::Missing("boot_dir")
        );
    }
}
