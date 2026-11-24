//! Opening the phone's page in Settings through the app launcher: `launcher.start` version 2
//! (`{app, role, activation_token, uri, new_window}` → `{state, pid, already, opened}`, contract
//! `ultar_bus::messages::launcher` of the launcher delivery). The two messages are mirrored here
//! field for field (postcard is positional, so the wire is the same); when that contract is on
//! `main`, `Start`/`StartReply` replace them one to one.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use serde::{Deserialize, Serialize};
use std::time::Duration;
use ultar_bus::{BusError, Call, Client, Message};

/// App id of Settings (as the shell starts it).
pub const SETTINGS_APP: &str = "org.ultar.settings";
/// The phone's page in Settings (`settings://apps/<id>`, the shell's convention).
pub const PHONE_SETTINGS_URI: &str = "settings://apps/org.ultar.phone";

/// Which program of an app (`launcher::Role`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    /// The window.
    Ui,
    /// The background service.
    Service,
}

/// Where an instance is (`launcher::InstanceState`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum InstanceState {
    /// Being built.
    Starting,
    /// Runs.
    Running,
    /// Stopping.
    Stopping,
    /// Waits before a restart.
    Backoff,
    /// Ended normally.
    Stopped,
    /// Crashed.
    Crashed,
    /// Given up.
    Failed,
}

/// `launcher.start` version 2.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Start {
    /// App id.
    pub app: String,
    /// Which program.
    pub role: Role,
    /// Activation token of the user action, if any.
    pub activation_token: Option<u64>,
    /// What to open.
    pub uri: Option<String>,
    /// Another window of a running app.
    pub new_window: bool,
}

/// Answer to [`Start`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct StartReply {
    /// State of the instance.
    pub state: InstanceState,
    /// Process id.
    pub pid: u32,
    /// It ran already.
    pub already: bool,
    /// The URI was handed over.
    pub opened: bool,
}

impl Message for Start {
    const VERSION: u16 = 2;
}
impl Message for StartReply {
    const VERSION: u16 = 2;
}
impl Call for Start {
    const SERVICE: &'static str = "launcher";
    const METHOD: &'static str = "start";
    type Reply = StartReply;
}

/// The request that opens the phone's settings page.
pub fn phone_settings(activation_token: Option<u64>) -> Start {
    Start {
        app: SETTINGS_APP.into(),
        role: Role::Ui,
        activation_token,
        uri: Some(PHONE_SETTINGS_URI.into()),
        new_window: false,
    }
}

/// Asks the launcher to open the phone's settings page.
pub fn open_phone_settings(
    bus: &Client,
    activation_token: Option<u64>,
    timeout: Duration,
) -> Result<StartReply, BusError> {
    bus.call(&phone_settings(activation_token), timeout)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::{decode, encode};

    #[test]
    fn the_request_is_launcher_start_version_2() {
        let s = phone_settings(Some(7));
        assert_eq!(
            (Start::SERVICE, Start::METHOD, <Start as Message>::VERSION),
            ("launcher", "start", 2)
        );
        assert_eq!(decode::<Start>(&encode(&s).unwrap()).unwrap(), s);
        assert!(s
            .uri
            .as_deref()
            .is_some_and(|u| u.starts_with("settings://") && u.len() <= 2048));
        // Positional wire: app, role (variant 0 = Ui), token, uri, new_window.
        let bytes = encode(&s).unwrap();
        assert_eq!(bytes[0] as usize, SETTINGS_APP.len());
        assert_eq!(bytes[1 + SETTINGS_APP.len()], 0, "role Ui");
    }
}
