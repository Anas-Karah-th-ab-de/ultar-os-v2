//! The launcher's configuration file (`/etc/ultar/launcher.toml`).
//!
//! Every key is checked; unknown keys refuse the file. Paths must be
//! absolute and normalised. `accept_missing` is the operator's explicit
//! decision to run apps although the kernel lacks an isolation feature –
//! empty in the shipped file (fail closed), used by development machines
//! and tests that run without root.

use crate::manifest::normalised_absolute;
use crate::platform::Feature;
use crate::store::UidRange;
use serde::Deserialize;
use std::fmt;
use std::path::{Path, PathBuf};

/// Largest configuration file.
const MAX_BYTES: u64 = 64 * 1024;
/// Most entries of `system_view` and `shares`.
const MAX_VIEW: usize = 32;

/// Why the configuration was refused.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Io(std::io::ErrorKind),
    /// Not TOML, an unknown key or a wrong type.
    Syntax,
    /// A path that is not absolute and normalised (key name).
    Path(&'static str),
    /// The user id range is empty, contains root or the unknown id.
    Range,
    /// A bound out of range (key name).
    Bound(&'static str),
    /// An unknown word in `accept_missing`.
    Feature,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(kind) => write!(f, "launcher.config.io ({kind})"),
            Self::Syntax => f.write_str("launcher.config.syntax"),
            Self::Path(key) => write!(f, "launcher.config.path ({key})"),
            Self::Range => f.write_str("launcher.config.range"),
            Self::Bound(key) => write!(f, "launcher.config.bound ({key})"),
            Self::Feature => f.write_str("launcher.config.feature"),
        }
    }
}

impl std::error::Error for ConfigError {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    paths: RawPaths,
    uid_first: u32,
    uid_last: u32,
    #[serde(default)]
    system_view: Vec<String>,
    #[serde(default)]
    shares: Vec<String>,
    #[serde(default)]
    accept_missing: Vec<String>,
    max_running: Option<u32>,
    stop_grace_ms: Option<u64>,
    areas_group: Option<u32>,
    #[serde(default)]
    publishers: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPaths {
    state_dir: String,
    data_root: String,
    cell_root: String,
    group_root: Option<String>,
    system_keys: String,
    store_keys: String,
    compositor_rights: Option<String>,
    output_socket: Option<String>,
    bus_socket: Option<String>,
    policy: Option<String>,
    app_bus: Option<String>,
    bus_apps: Option<String>,
    areas_root: Option<String>,
    publish_root: Option<String>,
    foreign_runtime: Option<String>,
    foreign_bridge: Option<String>,
}

impl RawPaths {
    fn check(self) -> Result<Paths, ConfigError> {
        Ok(Paths {
            state_dir: path("state_dir", &self.state_dir)?,
            data_root: path("data_root", &self.data_root)?,
            cell_root: path("cell_root", &self.cell_root)?,
            group_root: optional("group_root", self.group_root)?,
            system_keys: path("system_keys", &self.system_keys)?,
            store_keys: path("store_keys", &self.store_keys)?,
            compositor_rights: optional("compositor_rights", self.compositor_rights)?,
            output_socket: optional("output_socket", self.output_socket)?,
            bus_socket: optional("bus_socket", self.bus_socket)?,
            policy: optional("policy", self.policy)?,
            app_bus: optional("app_bus", self.app_bus)?,
            bus_apps: optional("bus_apps", self.bus_apps)?,
            areas_root: optional("areas_root", self.areas_root)?,
            publish_root: optional("publish_root", self.publish_root)?,
            foreign_runtime: optional("foreign_runtime", self.foreign_runtime)?,
            foreign_bridge: optional("foreign_bridge", self.foreign_bridge)?,
        })
    }
}

/// Directories and files the launcher works with.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Paths {
    /// The launcher's own state (`0700`).
    pub state_dir: PathBuf,
    /// Root of the apps' data directories (`0700`).
    pub data_root: PathBuf,
    /// Root of the cells' mount points (`0700`).
    pub cell_root: PathBuf,
    /// The memory group tree; `None`: none is used.
    pub group_root: Option<PathBuf>,
    /// Trusted keys of system apps (`app.system`).
    pub system_keys: PathBuf,
    /// Trusted keys of store apps (`app.standard`).
    pub store_keys: PathBuf,
    /// The rights file of the compositor for app ids.
    pub compositor_rights: Option<PathBuf>,
    /// Output channel of the log service.
    pub output_socket: Option<PathBuf>,
    /// The launcher's bus socket.
    pub bus_socket: Option<PathBuf>,
    /// Bus policy (who has `launcher.admin`).
    pub policy: Option<PathBuf>,
    /// Directory of the apps' bus socket (`bus.sock`), bound read-only into
    /// the cell of an app whose manifest declares bus access – and only there.
    pub app_bus: Option<PathBuf>,
    /// The broker's map of app user ids (`uid = "app.<id>"` for apps that
    /// declare bus access), read by `ultar-busd --app-map`.
    pub bus_apps: Option<PathBuf>,
    /// The user's file areas (`<areas_root>/<name>`, see `areas.rs`).
    pub areas_root: Option<PathBuf>,
    /// Where apps named in `publishers` publish for the system (`<publish_root>/<id>`, `0755`,
    /// owned by the app; the root is the launcher's own `0755` directory).
    pub publish_root: Option<PathBuf>,
    /// Root of the Linux area for apps from other systems (`foreign.rs`): what their cells
    /// see as `/`, read-only. `None`: such apps do not start.
    pub foreign_runtime: Option<PathBuf>,
    /// Directory holding the Wayland bridge (`ultar-wayland`, static), bound read-only at
    /// `/.ultar` in their cells. `None`: they start without a window.
    pub foreign_bridge: Option<PathBuf>,
}

/// The configuration.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Config {
    /// Directories and files.
    pub paths: Paths,
    /// App user ids.
    pub uids: UidRange,
    /// Host directories every cell sees read-only.
    pub system_view: Vec<PathBuf>,
    /// Host directories with sockets every cell sees read-only.
    pub shares: Vec<PathBuf>,
    /// Features the operator accepts as missing.
    pub accept_missing: Vec<Feature>,
    /// Most instances running at once.
    pub max_running: u32,
    /// Time between the stop signal and the hard end.
    pub stop_grace: std::time::Duration,
    /// Group of the user's file areas (the only supplementary group of an app with areas).
    pub areas_group: Option<u32>,
    /// System apps that get a publish directory (`paths.publish_root`): what they write there
    /// every other user may read, so only apps whose readers check it (a signed catalogue).
    pub publishers: Vec<String>,
}

fn path(key: &'static str, text: &str) -> Result<PathBuf, ConfigError> {
    if normalised_absolute(text) {
        Ok(PathBuf::from(text))
    } else {
        Err(ConfigError::Path(key))
    }
}

fn optional(key: &'static str, text: Option<String>) -> Result<Option<PathBuf>, ConfigError> {
    text.map(|t| path(key, &t)).transpose()
}

fn list(key: &'static str, items: &[String]) -> Result<Vec<PathBuf>, ConfigError> {
    if items.len() > MAX_VIEW {
        return Err(ConfigError::Bound(key));
    }
    items.iter().map(|t| path(key, t)).collect()
}

fn publishers(ids: Vec<String>) -> Result<Vec<String>, ConfigError> {
    let valid = ids.len() <= MAX_VIEW && ids.iter().all(|id| crate::appid::AppId::new(id).is_ok());
    valid.then_some(ids).ok_or(ConfigError::Bound("publishers"))
}

impl Config {
    /// Reads the configuration text.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let raw: Raw = toml::from_str(text).map_err(|_| ConfigError::Syntax)?;
        if raw.uid_first == 0 || raw.uid_first > raw.uid_last || raw.uid_last >= 65534 {
            return Err(ConfigError::Range);
        }
        let max_running = raw.max_running.unwrap_or(32);
        let grace = raw.stop_grace_ms.unwrap_or(3000);
        if !(1..=256).contains(&max_running) {
            return Err(ConfigError::Bound("max_running"));
        }
        if !(100..=60_000).contains(&grace) {
            return Err(ConfigError::Bound("stop_grace_ms"));
        }
        let accept_missing = raw
            .accept_missing
            .iter()
            .map(|w| Feature::from_word(w).ok_or(ConfigError::Feature))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            paths: raw.paths.check()?,
            uids: UidRange {
                first: raw.uid_first,
                last: raw.uid_last,
            },
            system_view: list("system_view", &raw.system_view)?,
            shares: list("shares", &raw.shares)?,
            accept_missing,
            max_running,
            stop_grace: std::time::Duration::from_millis(grace),
            areas_group: raw.areas_group,
            publishers: publishers(raw.publishers)?,
        })
    }

    /// Reads the configuration file `file`.
    pub fn load(file: &Path) -> Result<Self, ConfigError> {
        let bytes = crate::atomic::read(file, MAX_BYTES).map_err(|e| ConfigError::Io(e.kind()))?;
        let text = std::str::from_utf8(&bytes).map_err(|_| ConfigError::Syntax)?;
        Self::parse(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHIPPED: &str = include_str!("../../../../rootfs/etc/ultar/launcher.toml");

    #[test]
    fn the_shipped_file_is_valid_and_fails_closed() {
        let config = Config::parse(SHIPPED).unwrap();
        assert!(config.accept_missing.is_empty());
        assert!(config.uids.first > 1000 && config.uids.last < 65534);
        assert!(config.paths.group_root.is_some());
        assert!(config.system_view.iter().all(|p| p.is_absolute()));
    }

    #[test]
    fn the_app_range_is_reserved_in_ids_toml_and_holds_no_fixed_id() {
        let ids: toml::Table = include_str!("../../../../rootfs/etc/ultar/ids.toml")
            .parse()
            .unwrap();
        let range = ids["ranges"]["apps"].as_str().unwrap();
        let config = Config::parse(SHIPPED).unwrap();
        assert_eq!(range, format!("{}-{}", config.uids.first, config.uids.last));
        for section in ["users", "groups"] {
            for (name, id) in ids[section].as_table().unwrap() {
                let id = u32::try_from(id.as_integer().unwrap()).unwrap();
                assert!(
                    !config.uids.contains(id),
                    "{name} = {id} lies in the app range"
                );
            }
        }
        assert_eq!(ids["users"]["launcher"].as_integer(), Some(805));
        // the compositor follows the same range and owner
        let compositor = include_str!("../../../../rootfs/etc/ultar/services/compositor.toml");
        assert!(compositor.contains(&format!("\"--app-uids\", \"{range}\"")));
        assert!(compositor.contains("\"--app-rights-owner\", \"805\""));
        let rights = config.paths.compositor_rights.unwrap();
        assert!(compositor.contains(&format!("\"--app-rights\", \"{}\"", rights.display())));
        // the broker reads the launcher's app map of the same range and owner, on the apps' socket
        let bus = include_str!("../../../../rootfs/etc/ultar/services/bus.toml");
        let map = config.paths.bus_apps.unwrap();
        let socket = config.paths.app_bus.unwrap().join("bus.sock");
        assert!(bus.contains(&format!("\"--listen-apps\", \"{}\"", socket.display())));
        assert!(bus.contains(&format!("\"--app-map\", \"{}\"", map.display())));
        assert!(bus.contains("\"--app-map-owner\", \"805\""));
        assert!(bus.contains(&format!("\"--app-uids\", \"{range}\"")));
        // no cell gets the apps' socket for free: only through a declaration
        assert!(!config
            .shares
            .iter()
            .any(|s| s.starts_with("/run/ultar/appbus")));
    }

    #[test]
    fn bad_files_are_refused() {
        let good = SHIPPED.to_owned();
        let cases: [(&str, &str, ConfigError); 7] = [
            ("uid_first = 20000", "uid_first = 0", ConfigError::Range),
            ("uid_last = 49999", "uid_last = 65534", ConfigError::Range),
            ("uid_last = 49999", "uid_last = 100", ConfigError::Range),
            (
                "state_dir = \"/data/ultar/launcher\"",
                "state_dir = \"data\"",
                ConfigError::Path("state_dir"),
            ),
            (
                "state_dir = \"/data/ultar/launcher\"",
                "state_dir = \"/data/../etc\"",
                ConfigError::Path("state_dir"),
            ),
            (
                "accept_missing = []",
                "accept_missing = [\"seccomp\"]",
                ConfigError::Feature,
            ),
            (
                "max_running = 32",
                "max_running = 0",
                ConfigError::Bound("max_running"),
            ),
        ];
        for (from, to, error) in cases {
            assert!(good.contains(from), "{from}");
            assert_eq!(Config::parse(&good.replacen(from, to, 1)), Err(error));
        }
        assert_eq!(Config::parse("x = 1"), Err(ConfigError::Syntax));
        assert_eq!(
            Config::parse(&format!("{good}\nsurprise = 1\n")),
            Err(ConfigError::Syntax)
        );
        assert!(matches!(
            Config::load(Path::new("/nonexistent/launcher.toml")),
            Err(ConfigError::Io(_))
        ));
        assert_eq!(ConfigError::Range.to_string(), "launcher.config.range");
    }

    #[test]
    fn only_the_store_publishes_and_a_bad_name_is_refused() {
        let config = Config::parse(SHIPPED).unwrap();
        assert_eq!(config.publishers, ["store"]);
        let root = config.paths.publish_root.unwrap();
        assert_eq!(root, PathBuf::from("/data/ultar/publish"));
        let bad = SHIPPED.replace("publishers = [\"store\"]", "publishers = [\"../etc\"]");
        assert_eq!(Config::parse(&bad), Err(ConfigError::Bound("publishers")));
        let none = SHIPPED.replace("publishers = [\"store\"]", "");
        assert!(Config::parse(&none).unwrap().publishers.is_empty());
    }
}
