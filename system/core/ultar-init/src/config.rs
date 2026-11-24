//! `/etc/ultar/init.toml`: what PID 1 sets up at boot (host name, mounts,
//! modules) and where the service manager finds and puts things. Every key
//! has a default; a faulty file is reported and the defaults are used.

use serde::Deserialize;
use std::path::PathBuf;

/// The configuration file.
pub const INIT_CONF: &str = "/etc/ultar/init.toml";
/// Service files of the system.
pub const SERVICE_DIR: &str = "/etc/ultar/services";
/// Service files of installed apps (on the data partition).
pub const USER_SERVICE_DIR: &str = "/data/ultar/services";
/// Additional module list (one `.ko` per line), written by the image build.
pub const MODULE_LIST: &str = "/etc/ultar/modules.list";

/// One mount after the base file systems.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MountEntry {
    /// Device or partition (`PARTLABEL=…` and the like, see `hw.rs`).
    pub source: String,
    /// Mount point.
    pub target: String,
    /// File system type.
    pub fstype: String,
    /// Mount options.
    #[serde(default)]
    pub options: Option<String>,
    /// Read only.
    #[serde(default)]
    pub readonly: bool,
    /// A failure is ignored (an SD card that may be missing).
    #[serde(default)]
    pub optional: bool,
    /// How long to wait for the source (USB stick, NVMe module loading).
    #[serde(default)]
    pub wait_ms: u64,
    /// Editions this mount belongs to (empty: all). The desktop's system and
    /// data partitions are mounted by the kernel; the phone's partitions
    /// (/compat, /data on eMMC, /vendor, /efs) are tried on phones only.
    #[serde(default)]
    pub editions: Vec<String>,
}

/// The configuration.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InitConfig {
    /// Host name.
    #[serde(default)]
    pub hostname: Option<String>,
    /// Mounts after the base file systems.
    #[serde(default)]
    pub mounts: Vec<MountEntry>,
    /// Uncompressed kernel modules loaded at boot.
    #[serde(default)]
    pub modules: Vec<String>,
    /// Default deadline between stop signal and hard end of a service.
    #[serde(default = "default_grace")]
    pub shutdown_grace_ms: u64,
    /// Directories of service files.
    #[serde(default = "default_service_dirs")]
    pub service_dirs: Vec<PathBuf>,
    /// Root of the memory group tree (default: `/sys/fs/cgroup` as PID 1,
    /// none otherwise).
    #[serde(default)]
    pub group_root: Option<PathBuf>,
    /// Output socket of the log service.
    #[serde(default = "default_output_socket")]
    pub output_socket: Option<PathBuf>,
    /// Bus socket of init.
    #[serde(default = "default_bus_socket")]
    pub bus_socket: Option<PathBuf>,
    /// Bus policy (administrators of `init`).
    #[serde(default = "default_bus_policy")]
    pub bus_policy: PathBuf,
    /// Text control socket (`ultar-ctl`).
    #[serde(default = "default_control_socket")]
    pub control_socket: Option<PathBuf>,
    /// File naming services that may use the network group.
    #[serde(default = "default_net_allowed")]
    pub net_allowed: Option<PathBuf>,
    /// Program executed as launcher (default: init itself).
    #[serde(default)]
    pub launcher: Option<PathBuf>,
    /// The administrator's override: services allowed to keep every
    /// kernel right (`rights = ["all"]`); all others asking for it are
    /// refused at start.
    #[serde(default)]
    pub all_rights_allowed: Vec<String>,
}

fn default_grace() -> u64 {
    3000
}

fn default_service_dirs() -> Vec<PathBuf> {
    vec![PathBuf::from(SERVICE_DIR), PathBuf::from(USER_SERVICE_DIR)]
}

fn default_output_socket() -> Option<PathBuf> {
    Some(PathBuf::from("/run/ultar/log/init.sock"))
}

fn default_bus_socket() -> Option<PathBuf> {
    Some(PathBuf::from("/run/ultar/bus/init.sock"))
}

fn default_bus_policy() -> PathBuf {
    PathBuf::from("/etc/ultar/bus-policy.toml")
}

fn default_control_socket() -> Option<PathBuf> {
    Some(PathBuf::from(
        std::env::var("ULTAR_CTL_SOCK").unwrap_or_else(|_| "/run/ultar/ctl.sock".into()),
    ))
}

fn default_net_allowed() -> Option<PathBuf> {
    Some(PathBuf::from("/data/ultar/netz-erlaubt"))
}

impl Default for InitConfig {
    fn default() -> Self {
        Self {
            hostname: None,
            mounts: Vec::new(),
            modules: Vec::new(),
            shutdown_grace_ms: default_grace(),
            service_dirs: default_service_dirs(),
            group_root: None,
            output_socket: default_output_socket(),
            bus_socket: default_bus_socket(),
            bus_policy: default_bus_policy(),
            control_socket: default_control_socket(),
            net_allowed: default_net_allowed(),
            launcher: None,
            all_rights_allowed: Vec::new(),
        }
    }
}

/// Reads the configuration at `path`; a missing file gives the defaults,
/// a faulty one is reported and gives the defaults.
pub fn load(path: &std::path::Path) -> InitConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|error: toml::de::Error| {
            crate::log(&format!(
                "config refused file={} detail={}",
                path.display(),
                error.message()
            ));
            InitConfig::default()
        }),
        Err(_) => InitConfig::default(),
    }
}

/// Kernel parameter `ultar.profile=<name>` (the last valid one; the Ultar kernel names
/// `installer` when it starts the installer session from a medium): the boot profile whose
/// services start. Default [`ultar_init::spec::SYSTEM_PROFILE`].
pub fn cmdline_profile() -> String {
    profile_of(&ultar_sys::machine::kernel_arguments())
}

/// The boot profile named by kernel arguments `words`.
pub fn profile_of(words: &[String]) -> String {
    words
        .iter()
        .filter_map(|word| word.strip_prefix("ultar.profile="))
        .rfind(|name| ultar_init::spec::valid_name(name))
        .unwrap_or(ultar_init::spec::SYSTEM_PROFILE)
        .to_owned()
}

/// Kernel parameter `ultar.start=a,b`: start these (manual) services at boot.
pub fn cmdline_start() -> Vec<String> {
    ultar_sys::machine::kernel_arguments()
        .iter()
        .filter_map(|word| word.strip_prefix("ultar.start="))
        .flat_map(|value| value.split(',').map(String::from).collect::<Vec<_>>())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_file_and_an_empty_one_parse() {
        let shipped = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../rootfs/etc/ultar/init.toml");
        let text = std::fs::read_to_string(shipped).unwrap();
        let config: InitConfig = toml::from_str(&text).unwrap();
        assert!(config.shutdown_grace_ms > 0);
        let empty: InitConfig = toml::from_str("").unwrap();
        assert_eq!(empty.service_dirs.len(), 2);
        assert!(toml::from_str::<InitConfig>("frob = 1").is_err());
    }

    #[test]
    fn the_boot_profile_comes_from_the_kernel_arguments() {
        let words = |s: &str| s.split_whitespace().map(String::from).collect::<Vec<_>>();
        assert_eq!(profile_of(&words("quiet")), "system");
        assert_eq!(profile_of(&words("ultar.profile=installer")), "installer");
        assert_eq!(
            profile_of(&words("ultar.profile=a ultar.profile=installer")),
            "installer"
        );
        assert_eq!(profile_of(&words("ultar.profile=../x")), "system");
        assert_eq!(
            profile_of(&words("ultar.profile=installer ultar.profile=")),
            "installer"
        );
    }
}
