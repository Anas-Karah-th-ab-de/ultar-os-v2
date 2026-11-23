//! The seat's device nodes (compositor review N14): the event nodes of input
//! devices and the display nodes get owner root, the group `seat` and mode
//! `0660`, so the compositor (a member of that group) opens them and no other
//! user does; `/dev/uinput` stays root-only (`0600`). Devices made by software
//! (`uinput`, below `/devices/virtual/` in sysfs) get the seat group only when
//! their name is on the allow-list of the configuration; otherwise, and
//! whenever a node cannot be traced to its device, the node is made root-only
//! (fail closed).
//!
//! This part runs as its own small program (`ultar-devd-seat`) as root with
//! the single right `file_admin`, apart from the daemon (user 804, no rights):
//! changing the owner of a node of root needs that right, nothing else here
//! does. It has no bus and reads only `/sys` and its configuration.
//!
//! Kernels differ: Linux keeps owner and mode of device nodes; the Ultar kernel
//! (state 23.11.2026) refuses `chown`/`chmod` below `/dev` and gates the nodes
//! by the right `raw_device` instead. Which one runs is detected from the
//! result of the change, never assumed, and written to the state file
//! [`STATE_FILE`] for the compositor.

mod apply;
mod events;
mod run;

pub use apply::{apply_node, scan, write_state, Applied, DeviceModes, NodeReport};
pub use events::{DeviceEvent, EventSocket};
pub use run::run;

use serde::Deserialize;
use std::path::Path;
use ultar_hal_system::seat_nodes;

/// Name of the state file in the state directory: one word, see [`DeviceModes`].
pub const STATE_FILE: &str = "modes";
/// Most names on the allow-list of software input devices.
pub const MAX_VIRTUAL: usize = 16;
/// Longest device name compared (sysfs `name`).
pub const MAX_NAME: usize = 80;

/// `/etc/ultar/devd-seat.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SeatConfig {
    /// Group id of the seat (`seat`, 812); never 0.
    pub group: u32,
    /// Names of software input devices that may join the seat (on-screen
    /// keyboards of the system); empty by default.
    #[serde(default)]
    pub virtual_allowed: Vec<String>,
}

/// Why a configuration was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// Not readable.
    Read(String),
    /// Not TOML, unknown key or wrong type.
    Syntax(String),
    /// Group 0, too many or bad names.
    Bound(&'static str),
}

impl ConfigError {
    /// Key of the message for the operator.
    pub const fn key(&self) -> &'static str {
        match self {
            Self::Read(_) => "seat.config.read",
            Self::Syntax(_) => "seat.config.syntax",
            Self::Bound(_) => "seat.config.bound",
        }
    }

    /// Detail for the message (no secret is ever part of it).
    pub fn detail(&self) -> &str {
        match self {
            Self::Read(text) | Self::Syntax(text) => text,
            Self::Bound(what) => what,
        }
    }
}

impl SeatConfig {
    /// Parses and checks a configuration.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let config: SeatConfig =
            toml::from_str(text).map_err(|e| ConfigError::Syntax(e.message().to_owned()))?;
        if config.group == 0 {
            return Err(ConfigError::Bound("group"));
        }
        if config.virtual_allowed.len() > MAX_VIRTUAL {
            return Err(ConfigError::Bound("virtual_allowed"));
        }
        let bad = |name: &String| {
            name.trim().is_empty() || name.len() > MAX_NAME || name.chars().any(char::is_control)
        };
        if config.virtual_allowed.iter().any(bad) {
            return Err(ConfigError::Bound("virtual_allowed"));
        }
        Ok(config)
    }

    /// Reads and checks the file at `path` (at most 64 KiB).
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = apply::read_small(path, 64 * 1024).map_err(ConfigError::Read)?;
        Self::parse(&text)
    }
}

/// Owner, group and mode a node should have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wanted {
    /// Owner.
    pub uid: u32,
    /// Group.
    pub gid: u32,
    /// Permission bits.
    pub mode: u32,
    /// Why (key of the reason).
    pub reason: Reason,
}

/// Why a node gets its owner and mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// An input device with real hardware behind it.
    SeatInput,
    /// A display node (DRM card, framebuffer).
    SeatDisplay,
    /// A software input device on the allow-list.
    AllowedVirtual,
    /// A software input device not on the allow-list: root only.
    RefusedVirtual,
    /// An event node whose device could not be found in sysfs: root only.
    Untraced,
    /// `/dev/uinput`: creating input devices stays with root.
    Uinput,
}

impl Reason {
    /// Word for journal lines and the `--once` listing.
    pub const fn word(self) -> &'static str {
        match self {
            Self::SeatInput => "seat-input",
            Self::SeatDisplay => "seat-display",
            Self::AllowedVirtual => "allowed-virtual",
            Self::RefusedVirtual => "refused-virtual",
            Self::Untraced => "untraced",
            Self::Uinput => "uinput",
        }
    }
}

/// True for a name below `/dev` this program looks after: `uinput`,
/// `input/event<n>`, `dri/card<n>`, `fb<n>`.
pub fn is_seat_node(devname: &str) -> bool {
    let digits = |rest: &str| {
        !rest.is_empty() && rest.len() <= 4 && rest.bytes().all(|b| b.is_ascii_digit())
    };
    devname == "uinput"
        || devname.strip_prefix("input/event").is_some_and(digits)
        || devname.strip_prefix("dri/card").is_some_and(digits)
        || devname.strip_prefix("fb").is_some_and(digits)
}

/// What node `devname` (relative to `/dev`) should look like; `None` for nodes
/// this program leaves alone. `root` is the root of the tree (`/`; a prepared
/// tree in tests).
pub fn wanted(root: &Path, devname: &str, config: &SeatConfig) -> Option<Wanted> {
    if !is_seat_node(devname) {
        return None;
    }
    let root_only = |reason| Wanted {
        uid: 0,
        gid: 0,
        mode: 0o600,
        reason,
    };
    let seat = |reason| Wanted {
        uid: 0,
        gid: config.group,
        mode: 0o660,
        reason,
    };
    if devname == "uinput" {
        return Some(root_only(Reason::Uinput));
    }
    let Some(event) = devname.strip_prefix("input/") else {
        return Some(seat(Reason::SeatDisplay));
    };
    let class = seat_nodes::input_class(root, event);
    let Ok(device) = std::fs::canonicalize(&class) else {
        return Some(root_only(Reason::Untraced));
    };
    if !seat_nodes::is_virtual_device(&device) {
        return Some(seat(Reason::SeatInput));
    }
    let name = apply::read_small(&seat_nodes::input_name(&class), 256).unwrap_or_default();
    let name = name.trim();
    let allowed = !name.is_empty() && config.virtual_allowed.iter().any(|n| n.trim() == name);
    Some(if allowed {
        seat(Reason::AllowedVirtual)
    } else {
        root_only(Reason::RefusedVirtual)
    })
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
