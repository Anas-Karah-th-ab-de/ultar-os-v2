//! The permissions the launcher grants and enforces, and the kernel rights
//! a manifest may name.
//!
//! | Permission | Enforced by |
//! |---|---|
//! | `network` | the cell: without it the app gets a network namespace of its own (only a loopback device) |
//! | `background` | the launcher: the app's `[service]` starts only with it |
//! | `ui.capture`, `ui.overlay`, `ui.client-header` | the compositor, through the app rights file the launcher writes (`crate::rightsfile`) |
//!
//! `ui.shell`, `ui.input-method`, `ui.lock-screen` and `ui.system-dialog`
//! are known names but never an app's permission (review S-7): the shell,
//! the keyboard, the lock screen and the system dialogs are system services
//! whose rights come only from the static `compositor-rights.toml`. No
//! profile can grant them, so one "Allow" can never hand an app `Unlock`,
//! the shell protocol or the trusted mark of a system dialog.
//!
//! Bus calls and named file areas of a manifest are recorded and shown,
//! not granted here (bus: the broker's policy; files: no file service yet).

use std::fmt;

/// One permission an app can declare and be granted.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Permission {
    /// Reach the network.
    Network,
    /// Run a background service.
    Background,
    /// Panels and wallpaper of the shell.
    UiShell,
    /// On-screen keyboard.
    UiInputMethod,
    /// Lock screen.
    UiLockScreen,
    /// Read the composed screen.
    UiCapture,
    /// Overlays (notifications).
    UiOverlay,
    /// System dialogs (password and permission prompts).
    UiSystemDialog,
    /// The window draws its own header (the phone's call screens; the compositor refuses it for
    /// system dialogs). Without it `ClientHeader` ends the app's connection: found in VMware, the
    /// phone closed at once.
    UiClientHeader,
}

impl Permission {
    /// Every permission, in bit order.
    pub const ALL: [Permission; 9] = [
        Self::Network,
        Self::Background,
        Self::UiShell,
        Self::UiInputMethod,
        Self::UiLockScreen,
        Self::UiCapture,
        Self::UiOverlay,
        Self::UiSystemDialog,
        Self::UiClientHeader,
    ];

    /// The permission's name in manifests, on the bus and on the command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::Background => "background",
            Self::UiShell => "ui.shell",
            Self::UiInputMethod => "ui.input-method",
            Self::UiLockScreen => "ui.lock-screen",
            Self::UiCapture => "ui.capture",
            Self::UiOverlay => "ui.overlay",
            Self::UiSystemDialog => "ui.system-dialog",
            Self::UiClientHeader => "ui.client-header",
        }
    }

    /// The permission called `name`.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.name() == name)
    }

    /// Whether the compositor enforces it (it belongs in the rights file).
    pub const fn is_ui(self) -> bool {
        !matches!(self, Self::Network | Self::Background)
    }

    /// Whether it belongs to the system's own user interface (shell, keyboard,
    /// lock screen, system dialogs): only the static rights file gives it,
    /// never the launcher (review S-7).
    pub const fn is_system_ui(self) -> bool {
        matches!(
            self,
            Self::UiShell | Self::UiInputMethod | Self::UiLockScreen | Self::UiSystemDialog
        )
    }

    const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A set of permissions.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct PermissionSet(u16);

impl PermissionSet {
    /// No permission.
    pub const EMPTY: Self = Self(0);

    /// Every permission.
    pub const fn all() -> Self {
        Self((1 << Permission::ALL.len()) - 1)
    }

    /// Every permission an app can hold at all: all but the system UI ones
    /// ([`Permission::is_system_ui`]).
    pub const fn for_apps() -> Self {
        Self::all()
            .without(Permission::UiShell)
            .without(Permission::UiInputMethod)
            .without(Permission::UiLockScreen)
            .without(Permission::UiSystemDialog)
    }

    /// The set with exactly `permissions`.
    pub fn of(permissions: &[Permission]) -> Self {
        Self(permissions.iter().fold(0, |bits, p| bits | p.bit()))
    }

    /// Whether `permission` is in the set.
    pub const fn has(self, permission: Permission) -> bool {
        self.0 & permission.bit() != 0
    }

    /// The set with `permission` added.
    pub const fn with(self, permission: Permission) -> Self {
        Self(self.0 | permission.bit())
    }

    /// The set without `permission`.
    pub const fn without(self, permission: Permission) -> Self {
        Self(self.0 & !permission.bit())
    }

    /// Permissions in both sets.
    pub const fn and(self, other: Self) -> Self {
        Self(self.0 & other.0)
    }

    /// Whether every permission of `self` is in `other`.
    pub const fn within(self, other: Self) -> bool {
        self.0 & !other.0 == 0
    }

    /// Whether the set is empty.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The permissions of the set, in bit order.
    pub fn iter(self) -> impl Iterator<Item = Permission> {
        Permission::ALL.into_iter().filter(move |p| self.has(*p))
    }

    /// The names joined by commas (state file, rights file).
    pub fn names(self) -> String {
        self.iter()
            .map(Permission::name)
            .collect::<Vec<_>>()
            .join(",")
    }

    /// Parses [`PermissionSet::names`]; `None` for an unknown name.
    pub fn parse_names(text: &str) -> Option<Self> {
        if text.is_empty() {
            return Some(Self::EMPTY);
        }
        text.split(',')
            .map(Permission::from_name)
            .try_fold(Self::EMPTY, |set, p| p.map(|p| set.with(p)))
    }
}

/// Names of the kernel rights in bit order (the Ultar kernel's
/// `PR_ULTAR_RIGHTS_*` bits, the same as `ultar_service::Right`).
pub const KERNEL_RIGHT_NAMES: [&str; 14] = [
    "mount",
    "net_admin",
    "load_driver",
    "kill_any",
    "set_time",
    "raw_device",
    "reboot",
    "manage_groups",
    "set_id",
    "file_admin",
    "bluetooth",
    "input_inject",
    "sound",
    "camera",
];

/// The kernel right `sound`: the sound card's running streams `/dev/ultar/pcm`.
pub const KERNEL_SOUND: u64 = 1 << 12;

/// The bit mask of the kernel rights named in `names`; `None` for an
/// unknown name.
pub fn kernel_rights(names: &[String]) -> Option<u64> {
    names.iter().try_fold(0u64, |mask, name| {
        KERNEL_RIGHT_NAMES
            .iter()
            .position(|known| known == name)
            .map(|bit| mask | 1 << bit)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_sets_behave() {
        for p in Permission::ALL {
            assert_eq!(Permission::from_name(p.name()), Some(p));
            assert_eq!(p.to_string(), p.name());
        }
        assert_eq!(Permission::from_name("root"), None);
        let set = PermissionSet::of(&[Permission::Network, Permission::UiCapture]);
        assert_eq!(set.names(), "network,ui.capture");
        assert_eq!(PermissionSet::parse_names(&set.names()), Some(set));
        assert_eq!(PermissionSet::parse_names(""), Some(PermissionSet::EMPTY));
        assert_eq!(PermissionSet::parse_names("network,nope"), None);
        assert!(set.has(Permission::UiCapture) && !set.has(Permission::Background));
        assert!(set.without(Permission::Network).within(set));
        assert!(!PermissionSet::all().within(set));
        assert_eq!(PermissionSet::all().iter().count(), 9);
        assert_eq!(
            set.and(PermissionSet::of(&[Permission::Network])).names(),
            "network"
        );
        assert!(PermissionSet::EMPTY.is_empty() && !set.is_empty());
        assert_eq!(PermissionSet::all().iter().filter(|p| p.is_ui()).count(), 7);
    }

    #[test]
    fn the_system_ui_is_no_app_permission() {
        let apps = PermissionSet::for_apps();
        for p in Permission::ALL {
            assert_eq!(apps.has(p), !p.is_system_ui(), "{p}");
        }
        assert_eq!(
            apps.names(),
            "network,background,ui.capture,ui.overlay,ui.client-header"
        );
    }

    #[test]
    fn kernel_right_names_follow_the_kernel_bits() {
        assert_eq!(kernel_rights(&[]), Some(0));
        assert_eq!(kernel_rights(&["mount".into()]), Some(1));
        assert_eq!(
            kernel_rights(&["file_admin".into(), "set_id".into()]),
            Some(0x300)
        );
        assert_eq!(kernel_rights(&["root".into()]), None);
        assert_eq!(kernel_rights(&["sound".into()]), Some(KERNEL_SOUND));
        assert_eq!(KERNEL_RIGHT_NAMES.len(), ultar_service::Right::ALL.len());
        for (bit, right) in ultar_service::Right::ALL.iter().enumerate() {
            assert_eq!(right.bit(), 1 << bit);
        }
    }
}
