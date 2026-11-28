//! What the kernel offers for a cell – detected at start, never assumed –
//! and the rule that a missing feature stops a start unless the operator
//! accepted it (`accept_missing` in the configuration).
//!
//! | Feature | Needed for | Detected by |
//! |---|---|---|
//! | `ids` | an own user per app | root on Linux; the right `set_id` on the Ultar kernel |
//! | `cell` | mount + pid namespaces, the root switch | a helper that tries `unshare` (`--probe`) |
//! | `groups` | memory and process limits | a group tree with `cgroup.procs` at the configured root |
//! | `net` | no network without the permission | the helper tries a network namespace |
//! | `net-guard` | network apps reach only the internet (no host loopback, device address, LAN) | the helper sets the Ultar kernel's "internet only" barrier |
//! | `device-binds` | apps from other systems: only the harmless device nodes, never the device tree | the helper binds one node (the same probe that gives every cell its single-bind `/dev`) |
//!
//! Also detected: whether single device nodes can be bound into a cell
//! (else the Ultar kernel's device tree, gated by modes and the right
//! `raw_device`, is mounted) and whether the kernel has Ultar rights.

use std::path::Path;
use std::process::Command;

/// An isolation feature.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Feature {
    /// Own user ids per app.
    Ids,
    /// Mount and pid namespaces.
    Cell,
    /// Memory groups with limits.
    Groups,
    /// Network namespaces.
    Net,
    /// The "internet only" barrier of the Ultar kernel.
    NetGuard,
    /// Single device nodes bound into a cell (required for apps from other systems).
    DeviceBinds,
}

impl Feature {
    /// Every feature.
    pub const ALL: [Feature; 6] = [
        Self::Ids,
        Self::Cell,
        Self::Groups,
        Self::Net,
        Self::NetGuard,
        Self::DeviceBinds,
    ];

    /// Word in the configuration and in records.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ids => "ids",
            Self::Cell => "cell",
            Self::Groups => "groups",
            Self::Net => "net",
            Self::NetGuard => "net-guard",
            Self::DeviceBinds => "device-binds",
        }
    }

    /// The feature called `word`.
    pub fn from_word(word: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.word() == word)
    }
}

/// Answer of the probe helper: exit code [`PROBE_BASE`] plus these bits.
pub const PROBE_BASE: i32 = 64;
/// Probe bit: mount + pid namespaces.
pub const PROBE_CELL: i32 = 1;
/// Probe bit: network namespace.
pub const PROBE_NET: i32 = 2;
/// Probe bit: a single device node can be bound.
pub const PROBE_FILE_BIND: i32 = 4;
/// Probe bit: IPC namespace.
pub const PROBE_IPC: i32 = 8;
/// Probe bit: the "internet only" barrier (in a network namespace of its own).
pub const PROBE_NET_GUARD: i32 = 16;

/// What the kernel offers.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Platform {
    /// The Ultar kernel (it answers the rights `prctl`).
    pub ultar: bool,
    /// Ids can be switched.
    pub ids: bool,
    /// Mount + pid namespaces.
    pub cell: bool,
    /// Network namespaces.
    pub net: bool,
    /// The "internet only" barrier.
    pub net_guard: bool,
    /// IPC namespaces.
    pub ipc: bool,
    /// Single device nodes can be bound.
    pub file_binds: bool,
    /// A group tree exists at the configured root.
    pub groups: bool,
}

/// The `set_id` right of the Ultar kernel.
const SET_ID: u64 = 1 << 8;

impl Platform {
    /// Combines the probe's exit code with what this process can see.
    pub fn from_probe(code: Option<i32>, rights: Option<u64>, euid: u32, groups: bool) -> Self {
        let bits = code
            .filter(|c| (PROBE_BASE..PROBE_BASE + 32).contains(c))
            .map_or(0, |c| c - PROBE_BASE);
        Self {
            ultar: rights.is_some(),
            ids: rights.map_or(euid == 0, |r| r & SET_ID != 0),
            cell: bits & PROBE_CELL != 0,
            net: bits & PROBE_NET != 0,
            net_guard: bits & PROBE_NET_GUARD != 0,
            ipc: bits & PROBE_IPC != 0,
            file_binds: bits & PROBE_FILE_BIND != 0,
            groups,
        }
    }

    /// Detects the platform: runs `helper --probe <scratch>` (a fresh
    /// process, so this one keeps its namespaces) and looks for the group
    /// tree at `group_root`.
    pub fn detect(helper: &Path, scratch: &Path, group_root: Option<&Path>) -> Self {
        let code = Command::new(helper)
            .arg(crate::cell::PROBE_ARG)
            .arg(scratch)
            .env_clear()
            .status()
            .ok()
            .and_then(|s| s.code());
        let groups = group_root.is_some_and(|root| root.join("cgroup.procs").is_file());
        Self::from_probe(
            code,
            ultar_sys::ultar_rights().ok(),
            ultar_sys::euid(),
            groups,
        )
    }

    /// Whether `feature` is there.
    pub const fn has(&self, feature: Feature) -> bool {
        match feature {
            Feature::Ids => self.ids,
            Feature::Cell => self.cell,
            Feature::Groups => self.groups,
            Feature::Net => self.net,
            Feature::NetGuard => self.net_guard,
            Feature::DeviceBinds => self.file_binds,
        }
    }

    /// Whether `feature` is there – for apps from other systems, where nothing missing is
    /// ever accepted (`accept_missing` does not count). `Err` names the feature.
    pub fn require_strictly(&self, feature: Feature) -> Result<(), Feature> {
        if self.has(feature) {
            Ok(())
        } else {
            Err(feature)
        }
    }

    /// Whether a start may go ahead without `feature`: it is there, or the
    /// operator accepted that it is missing. `Err` names the feature.
    pub fn require(&self, feature: Feature, accepted: &[Feature]) -> Result<bool, Feature> {
        if self.has(feature) {
            Ok(true)
        } else if accepted.contains(&feature) {
            Ok(false)
        } else {
            Err(feature)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_answers_are_decoded_and_others_mean_nothing() {
        let all = Platform::from_probe(Some(PROBE_BASE + 15), None, 0, true);
        assert!(all.cell && all.net && all.ipc && all.file_binds && all.ids && all.groups);
        assert!(!all.ultar);
        for code in [
            None,
            Some(0),
            Some(1),
            Some(127),
            Some(PROBE_BASE + 32),
            Some(-1),
        ] {
            let p = Platform::from_probe(code, None, 1000, false);
            assert_eq!(p, Platform::default(), "{code:?}");
        }
        // the Ultar kernel: ids come from the right, not from uid 0
        let ultar = Platform::from_probe(Some(PROBE_BASE + 1), Some(0), 0, true);
        assert!(ultar.ultar && !ultar.ids && ultar.cell && !ultar.net);
        assert!(Platform::from_probe(None, Some(SET_ID), 805, false).ids);
        let guarded = Platform::from_probe(
            Some(PROBE_BASE + PROBE_NET + PROBE_NET_GUARD),
            Some(0),
            0,
            true,
        );
        assert!(guarded.net && guarded.net_guard);
    }

    #[test]
    fn a_missing_feature_stops_unless_accepted() {
        let p = Platform {
            cell: true,
            ..Platform::default()
        };
        assert_eq!(p.require(Feature::Cell, &[]), Ok(true));
        assert_eq!(p.require(Feature::Net, &[]), Err(Feature::Net));
        assert_eq!(p.require(Feature::Net, &[Feature::Net]), Ok(false));
        for f in Feature::ALL {
            assert_eq!(Feature::from_word(f.word()), Some(f));
        }
        assert_eq!(Feature::from_word("seccomp"), None);
    }

    #[test]
    fn a_helper_that_cannot_run_offers_nothing() {
        let p = Platform::detect(Path::new("/nonexistent/helper"), Path::new("/tmp"), None);
        assert!(!p.cell && !p.net && !p.groups);
    }
}
