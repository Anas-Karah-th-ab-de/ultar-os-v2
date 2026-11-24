//! The ports over `ultar-bus`.

use super::{AuthPort, LauncherPort, LockPort, PortError, SettingsPort};
use crate::shell::Opening;
use std::time::Duration;
use ultar_bus::messages::auth::{self, Method, Outcome as AuthOutcome, Secret, UserInfo};
use ultar_bus::messages::launcher::{List, Role, Start};
use ultar_bus::messages::lock::Lock;
use ultar_bus::messages::settings::{
    Confirm, DisplayDescriptor, Get, Revert, Set, SetDisplays, SettingValue,
};
use ultar_bus::{BusError, Client, ErrorCode};

/// How long a call may take (settings, launcher, lock).
pub const CALL_TIMEOUT: Duration = Duration::from_secs(3);
/// How long a password check may take (Argon2id plus the bus).
pub const VERIFY_TIMEOUT: Duration = Duration::from_secs(10);

fn port_error(e: BusError) -> PortError {
    match e {
        BusError::Remote(
            ErrorCode::PermissionDenied | ErrorCode::BadPayload | ErrorCode::Service(_),
        ) => PortError::Refused,
        _ => PortError::Unavailable,
    }
}

/// All ports over one bus connection.
#[derive(Clone)]
pub struct BusPorts {
    client: Client,
}

impl std::fmt::Debug for BusPorts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BusPorts").finish_non_exhaustive()
    }
}

impl BusPorts {
    /// Ports over `client`.
    pub fn new(client: Client) -> BusPorts {
        BusPorts { client }
    }

    /// The connection (subscriptions).
    pub fn client(&self) -> &Client {
        &self.client
    }
}

impl SettingsPort for BusPorts {
    fn get(&self, key: &str) -> Result<SettingValue, PortError> {
        let reply = self
            .client
            .call(
                &Get {
                    key: key.to_owned(),
                },
                CALL_TIMEOUT,
            )
            .map_err(port_error)?;
        reply.value.ok_or(PortError::Refused)
    }

    fn set(&self, key: &str, value: SettingValue) -> Result<(), PortError> {
        let request = Set {
            key: key.to_owned(),
            value,
        };
        self.client
            .call(&request, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(())
    }

    fn confirm(&self, key: &str) -> Result<(), PortError> {
        let request = Confirm {
            key: Some(key.to_owned()),
        };
        self.client
            .call(&request, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(())
    }

    fn revert(&self, key: &str) -> Result<(), PortError> {
        let request = Revert {
            key: Some(key.to_owned()),
        };
        self.client
            .call(&request, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(())
    }

    fn set_displays(&self, displays: &[DisplayDescriptor]) -> Result<Vec<String>, PortError> {
        let request = SetDisplays {
            displays: displays.to_vec(),
        };
        let reply = self
            .client
            .call(&request, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(reply.ids)
    }
}

/// Prefix of the shell's names of base-system apps (`org.ultar.files`).
const BASE_PREFIX: &str = "org.ultar.";

/// The launcher's id of an app the shell names `app`: base-system apps are signed with the
/// system key and have one-part ids in the launcher (`files`, `settings`; a dotted id belongs
/// to a store publisher, launcher `install.rs`), the shell names them `org.ultar.<id>`.
/// Found in VMware: every click asked the launcher for `org.ultar.settings`, which it can never
/// have (three parts are no app id), so nothing opened.
pub fn launcher_id(app: &str) -> &str {
    app.strip_prefix(BASE_PREFIX)
        .filter(|rest| !rest.is_empty() && !rest.contains('.'))
        .unwrap_or(app)
}

/// The shell's name of an app the launcher lists as `id` (the inverse of [`launcher_id`]).
pub fn shell_id(id: &str) -> String {
    if id.contains('.') {
        id.to_owned()
    } else {
        format!("{BASE_PREFIX}{id}")
    }
}

impl LauncherPort for BusPorts {
    fn start(&self, app: &str, open: &Opening, token: Option<u64>) -> Result<(), PortError> {
        let request = Start {
            app: launcher_id(app).to_owned(),
            role: Role::Ui,
            activation_token: token.filter(|t| *t != 0),
            uri: open.uri.clone(),
            new_window: open.new_window,
        };
        self.client
            .call(&request, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(())
    }

    fn installed(&self) -> Result<Vec<String>, PortError> {
        let reply = self
            .client
            .call(&List {}, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(reply.apps.into_iter().map(|a| shell_id(&a.app)).collect())
    }
}

impl LockPort for BusPorts {
    fn lock(&self) -> Result<(), PortError> {
        let reply = self
            .client
            .call(&Lock {}, CALL_TIMEOUT)
            .map_err(port_error)?;
        if reply.locked {
            Ok(())
        } else {
            Err(PortError::Refused)
        }
    }
}

impl AuthPort for BusPorts {
    fn users(&self) -> Result<Vec<UserInfo>, PortError> {
        let reply = self
            .client
            .call(&auth::Users {}, CALL_TIMEOUT)
            .map_err(port_error)?;
        Ok(reply.users)
    }

    fn verify(&self, user: &str, method: Method, secret: Secret) -> Result<AuthOutcome, PortError> {
        let request = auth::Verify {
            user: user.to_owned(),
            method,
            secret,
        };
        let reply = self
            .client
            .call(&request, VERIFY_TIMEOUT)
            .map_err(port_error)?;
        Ok(reply.outcome)
    }
}

#[cfg(test)]
mod launcher_ids {
    use super::{launcher_id, shell_id};

    #[test]
    fn base_apps_map_to_the_launchers_one_part_ids_and_back() {
        for (shell, launcher) in [
            ("org.ultar.settings", "settings"),
            ("org.ultar.files", "files"),
            ("org.ultar.browser", "browser"),
            ("org.ultar.store", "store"),
            ("org.ultar.phone", "phone"),
        ] {
            assert_eq!(launcher_id(shell), launcher);
            assert_eq!(shell_id(launcher), shell);
        }
        // store apps keep their publisher id both ways
        assert_eq!(launcher_id("acme.notes"), "acme.notes");
        assert_eq!(shell_id("acme.notes"), "acme.notes");
        // nothing else is rewritten
        assert_eq!(launcher_id("org.ultar."), "org.ultar.");
        assert_eq!(launcher_id("org.ultar.a.b"), "org.ultar.a.b");
    }
}
