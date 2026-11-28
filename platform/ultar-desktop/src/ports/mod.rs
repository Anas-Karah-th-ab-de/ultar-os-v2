//! The services behind the shell, as traits (so that the logic is tested without them)
//! with bus implementations in [`bus`]: settings, the app launcher, the lock screen and the
//! authentication service.

pub mod bus;
mod bus_calls;
mod bus_mesh;
mod bus_services;
pub mod services;
pub use bus_services::LockCall;

use crate::shell::Opening;
use std::fmt;
use ultar_bus::messages::auth::{Method, Outcome as AuthOutcome, Secret, UserInfo};
use ultar_bus::messages::settings::{DisplayDescriptor, SettingValue};

/// Why a service call did not work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortError {
    /// The service is not there (not started, not installed, bus down).
    Unavailable,
    /// The service refused (no permission, invalid value).
    Refused,
}

impl fmt::Display for PortError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PortError::Unavailable => f.write_str("service unavailable"),
            PortError::Refused => f.write_str("service refused"),
        }
    }
}

impl std::error::Error for PortError {}

/// The settings service (`settings.*`).
pub trait SettingsPort: Send + Sync {
    /// The value in effect.
    fn get(&self, key: &str) -> Result<SettingValue, PortError>;
    /// Changes a value (risky ones become pending, "keep?").
    fn set(&self, key: &str, value: SettingValue) -> Result<(), PortError>;
    /// Keeps a pending value.
    fn confirm(&self, key: &str) -> Result<(), PortError>;
    /// Takes a pending value back.
    fn revert(&self, key: &str) -> Result<(), PortError>;
    /// Reports the connected displays; returns their settings ids in the same order.
    fn set_displays(&self, displays: &[DisplayDescriptor]) -> Result<Vec<String>, PortError>;
}

/// The app launcher (`launcher.*`, docs/integration/ultar-launcher.md).
pub trait LauncherPort: Send + Sync {
    /// Starts an app's window or opens `open` in it (`launcher.start` version 2); `token`
    /// lets its first window take the focus. `Refused` when the app does not declare the
    /// URI's scheme or cannot take it while it runs (launcher errors 11 and 12).
    fn start(&self, app: &str, open: &Opening, token: Option<u64>) -> Result<(), PortError>;
    /// The installed apps' ids (`launcher.list`).
    fn installed(&self) -> Result<Vec<String>, PortError>;
    /// The app that opens `uri` by default (`launcher.resolve`: `apps.default.*`, only an
    /// installed app that declares it); `None` when there is none.
    fn resolve(&self, uri: &str) -> Result<Option<String>, PortError>;
}

/// The lock screen (`lock.lock`).
pub trait LockPort: Send + Sync {
    /// Locks now.
    fn lock(&self) -> Result<(), PortError>;
}

/// The authentication service (`auth.*`).
pub trait AuthPort: Send + Sync {
    /// The users that can sign in.
    fn users(&self) -> Result<Vec<UserInfo>, PortError>;
    /// Checks a secret.
    fn verify(&self, user: &str, method: Method, secret: Secret) -> Result<AuthOutcome, PortError>;
}
