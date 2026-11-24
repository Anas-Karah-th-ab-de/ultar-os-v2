//! Opening the phone's page in Settings through the app launcher: `launcher.start` version 2
//! (`{app, role, activation_token, uri, new_window}` → `{state, pid, already, opened}`), the
//! launcher's own contract `ultar_bus::messages::launcher`.
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

use std::time::Duration;
use ultar_bus::messages::launcher::{Role, Start, StartReply};
use ultar_bus::{BusError, Client};

/// App id of Settings (as the shell starts it).
pub const SETTINGS_APP: &str = "org.ultar.settings";
/// The phone's page in Settings (`settings://apps/<id>`, the shell's convention).
pub const PHONE_SETTINGS_URI: &str = "settings://apps/org.ultar.phone";

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
    use ultar_bus::messages::launcher::MAX_URI;
    use ultar_bus::{decode, encode, Call, Message};

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
            .is_some_and(|u| u.starts_with("settings://") && u.len() <= MAX_URI));
        // Positional wire: app, role (variant 0 = Ui), token, uri, new_window.
        let bytes = encode(&s).unwrap();
        assert_eq!(bytes[0] as usize, SETTINGS_APP.len());
        assert_eq!(bytes[1 + SETTINGS_APP.len()], 0, "role Ui");
    }
}
