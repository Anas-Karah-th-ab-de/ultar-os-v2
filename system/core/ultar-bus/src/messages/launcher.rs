//! Contract of the app launcher `ultar-launcher`: installed apps, their
//! running instances and the permissions the user granted them.
//!
//! The caller of `start`, `stop`, `list`, `permissions`, `grant` and
//! `revoke` is the shell (its permission prompts are system dialogs); the
//! administrator's tool `ulaunch` uses the same calls. `install` and
//! `uninstall` are for the installer and administrators: the policy gives
//! them only to those identities, and the launcher checks the caller
//! against the pseudo method [`ADMIN_METHOD`] as well.
//!
//! Every app runs as its own user in its own cell with exactly the
//! permissions its signed manifest declares and the user granted
//! (`docs/integration/ultar-launcher.md`).

use crate::message::{Call, Message};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "launcher";
/// Pseudo method whose call right marks an installer or administrator.
pub const ADMIN_METHOD: &str = "admin";
/// Most apps one [`ListReply`] lists.
pub const MAX_APPS: usize = 256;

/// Which program of an app an instance runs.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord, Hash)]
pub enum Role {
    /// The app's window (started by the user, never restarted).
    Ui,
    /// The app's background service (`[service]` of the manifest; needs the
    /// permission `background`; restarted after its policy).
    Service,
}

impl Role {
    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ui => "ui",
            Self::Service => "service",
        }
    }
}

/// Where an instance is in its life.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstanceState {
    /// The cell is being built.
    Starting,
    /// The app runs.
    Running,
    /// A stop was sent; waiting for the cell to end.
    Stopping,
    /// Ended; a service waits before its next start.
    Backoff,
    /// Ended normally or stopped on request.
    Stopped,
    /// Ended by a crash (signal or failure code) and not restarted.
    Crashed,
    /// A service that crashed too often in a short time: given up.
    Failed,
}

impl InstanceState {
    /// Stable lower-case word.
    pub const fn word(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopping => "stopping",
            Self::Backoff => "backoff",
            Self::Stopped => "stopped",
            Self::Crashed => "crashed",
            Self::Failed => "failed",
        }
    }

    /// Whether a process of the instance exists (or is about to).
    pub const fn is_active(self) -> bool {
        matches!(self, Self::Starting | Self::Running | Self::Stopping)
    }
}

/// How an instance ended.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Exit {
    /// The app returned this exit code.
    Code(i32),
    /// A signal with this number ended it.
    Signal(i32),
    /// The cell could not be built; the code names the step
    /// (`ultar-launcher`'s cell failure codes 110–127).
    Setup(i32),
}

/// One instance in an [`AppStatus`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InstanceStatus {
    /// Which program.
    pub role: Role,
    /// Current state.
    pub state: InstanceState,
    /// Process id of the app (the cell's first process) as the launcher
    /// sees it; 0 when none runs.
    pub pid: u32,
    /// Starts after the first one (crash restarts of a service).
    pub restarts: u32,
    /// How the previous run ended, if there was one.
    pub last_exit: Option<Exit>,
}

/// One installed app in a [`ListReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct AppStatus {
    /// App id.
    pub app: String,
    /// The app's own user id.
    pub uid: u32,
    /// Its profile (`app.standard`, `app.system`).
    pub profile: String,
    /// Version from the manifest.
    pub version: String,
    /// Its instances (only those that ran since the launcher started).
    pub instances: Vec<InstanceStatus>,
}

/// `launcher.list` – the installed apps and their instances.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct List {}

/// Answer to [`List`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct ListReply {
    /// The apps, by id (at most [`MAX_APPS`]).
    pub apps: Vec<AppStatus>,
}

/// `launcher.start` – start an app (single instance per role: a running
/// instance is reported, not started twice).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Start {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
    /// Activation token of the compositor for the user action that asked
    /// for the start (`RequestActivation`); handed to the app so that its
    /// first window may take the focus once. `None`/0: none.
    pub activation_token: Option<u64>,
}

/// Answer to [`Start`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartReply {
    /// State of the instance now.
    pub state: InstanceState,
    /// Process id of the app as the launcher sees it.
    pub pid: u32,
    /// The instance was running already (nothing was started).
    pub already: bool,
}

/// `launcher.stop` – stop an instance (a service stays stopped).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Stop {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
}

/// Answer to [`Stop`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StopReply {
    /// State of the instance now.
    pub state: InstanceState,
}

/// One permission of an app in a [`PermissionsReply`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PermissionState {
    /// Name (`network`, `background`, `ui.capture` …).
    pub name: String,
    /// Whether the user (or, for system apps, the installation) granted it.
    pub granted: bool,
}

/// `launcher.permissions` – what an app declared and what it was granted.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Permissions {
    /// App id.
    pub app: String,
}

/// Answer to [`Permissions`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct PermissionsReply {
    /// The permissions the launcher enforces, as declared in the manifest.
    pub declared: Vec<PermissionState>,
    /// Bus calls the manifest declares (enforced by the bus policy).
    pub bus: Vec<String>,
    /// Named file areas the manifest declares (not granted by the launcher).
    pub files: Vec<String>,
}

/// `launcher.grant` – the user granted a declared permission.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Grant {
    /// App id.
    pub app: String,
    /// Permission name.
    pub permission: String,
}

/// `launcher.revoke` – the user took a permission back. Running instances
/// whose cell depends on it are stopped.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Revoke {
    /// App id.
    pub app: String,
    /// Permission name.
    pub permission: String,
}

/// Answer to [`Grant`] and [`Revoke`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PermissionReply {
    /// Whether the permission is granted now.
    pub granted: bool,
    /// Instances that were stopped because of the change.
    pub stopped: u32,
}

/// `launcher.install` – register a signed package (`.uap`) that the
/// package installer put in place: verify its signature, give the app its
/// user, data directory and memory-group settings.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Install {
    /// Absolute path of the `.uap` file (readable by the launcher).
    pub package: String,
}

/// Answer to [`Install`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct InstallReply {
    /// App id.
    pub app: String,
    /// Its user id (the same as before if it was installed earlier).
    pub uid: u32,
    /// Its profile.
    pub profile: String,
    /// Permissions it declares, with their state.
    pub declared: Vec<PermissionState>,
}

/// `launcher.uninstall` – remove an app: its instances are stopped, its
/// data removed (unless kept); its user id stays reserved for it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Uninstall {
    /// App id.
    pub app: String,
    /// Keep the data directory (for a reinstall).
    pub keep_data: bool,
}

/// Answer to [`Uninstall`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct UninstallReply {
    /// Instances that were stopped.
    pub stopped: u32,
}

/// Error code: no installed app has this id.
pub const ERROR_UNKNOWN_APP: u32 = 1;
/// Error code: the manifest does not declare the permission (or the role).
pub const ERROR_NOT_DECLARED: u32 = 2;
/// Error code: the permission cannot be granted to this app's profile.
pub const ERROR_NOT_GRANTABLE: u32 = 3;
/// Error code: the package or its manifest was refused (signature, rules).
pub const ERROR_PACKAGE_REFUSED: u32 = 4;
/// Error code: the kernel lacks an isolation feature the start needs.
pub const ERROR_PLATFORM: u32 = 5;
/// Error code: a limit (running apps, user ids) is reached.
pub const ERROR_LIMIT: u32 = 6;
/// Error code: the cell could not be started.
pub const ERROR_START_FAILED: u32 = 7;
/// Error code: the launcher is shutting down.
pub const ERROR_SHUTTING_DOWN: u32 = 8;
/// Error code: the launcher's state could not be stored.
pub const ERROR_STORAGE: u32 = 9;
/// Error code: a permission the instance needs is not granted.
pub const ERROR_NOT_GRANTED: u32 = 10;
/// Error code: the request names something invalid (app id, path).
pub const ERROR_INVALID: u32 = 11;

macro_rules! messages {
    ($($t:ty),*) => {$(impl Message for $t { const VERSION: u16 = 1; })*};
}
messages!(
    List,
    ListReply,
    Start,
    StartReply,
    Stop,
    StopReply,
    Permissions,
    PermissionsReply,
    Grant,
    Revoke,
    PermissionReply,
    Install,
    InstallReply,
    Uninstall,
    UninstallReply
);

macro_rules! calls {
    ($($t:ty => $m:literal, $r:ty);*) => {$(
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    )*};
}
calls!(
    List => "list", ListReply;
    Start => "start", StartReply;
    Stop => "stop", StopReply;
    Permissions => "permissions", PermissionsReply;
    Grant => "grant", PermissionReply;
    Revoke => "revoke", PermissionReply;
    Install => "install", InstallReply;
    Uninstall => "uninstall", UninstallReply
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode_versioned, encode};

    fn round<T: Message + Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug>(
        value: &T,
    ) {
        let back: T = decode_versioned(T::VERSION, &encode(value).unwrap()).unwrap();
        assert_eq!(&back, value);
    }

    #[test]
    fn messages_round_trip_and_words_are_distinct() {
        round(&ListReply {
            apps: vec![AppStatus {
                app: "camera".into(),
                uid: 20001,
                profile: "app.system".into(),
                version: "1.0.0".into(),
                instances: vec![InstanceStatus {
                    role: Role::Service,
                    state: InstanceState::Backoff,
                    pid: 0,
                    restarts: 2,
                    last_exit: Some(Exit::Setup(121)),
                }],
            }],
        });
        round(&Start {
            app: "camera".into(),
            role: Role::Ui,
            activation_token: Some(u64::MAX),
        });
        round(&PermissionsReply {
            declared: vec![PermissionState {
                name: "network".into(),
                granted: false,
            }],
            bus: vec!["notify.post".into()],
            files: vec!["media:pictures:rw".into()],
        });
        round(&Uninstall {
            app: "x".into(),
            keep_data: true,
        });
        let states = [
            InstanceState::Starting,
            InstanceState::Running,
            InstanceState::Stopping,
            InstanceState::Backoff,
            InstanceState::Stopped,
            InstanceState::Crashed,
            InstanceState::Failed,
        ];
        let words: std::collections::BTreeSet<&str> = states.iter().map(|s| s.word()).collect();
        assert_eq!(words.len(), states.len());
        assert_eq!(states.iter().filter(|s| s.is_active()).count(), 3);
        assert_ne!(Role::Ui.word(), Role::Service.word());
        assert_eq!(<Install as Call>::METHOD, "install");
        assert_eq!(<Revoke as Call>::SERVICE, SERVICE);
    }
}
