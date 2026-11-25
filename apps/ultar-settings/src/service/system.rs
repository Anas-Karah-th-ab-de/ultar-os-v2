//! The services behind the Users, Apps and Storage pages: the local users of the account
//! service (`account.user_*`; it asks for the administrator in the system dialog), the
//! launcher's installed apps and their permissions, and how full the storage and the mounted
//! drives are (volume service).

use super::bus::{port_error, CALL_TIMEOUT, SLOW_TIMEOUT};
use super::{PortError, PortResult};
use ultar_bus::messages::account_local as users;
use ultar_bus::messages::launcher;
use ultar_bus::messages::volumes;
use ultar_bus::Client;

pub use users::LocalUser;

/// One installed app with the permissions its manifest declares.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AppEntry {
    /// App id.
    pub id: String,
    /// Version from the manifest.
    pub version: String,
    /// Declared permissions: name, granted.
    pub permissions: Vec<(String, bool)>,
}

/// How full the device's own storage or a mounted drive is.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drive {
    /// Label (or the kernel name); empty for the device's own storage.
    pub name: String,
    /// Size in bytes.
    pub total_bytes: u64,
    /// Bytes still free.
    pub free_bytes: u64,
}

/// Users, apps and drives.
pub trait SystemPort {
    /// The local users.
    fn users(&mut self) -> PortResult<Vec<LocalUser>>;
    /// Adds a user of `kind` (the Users page proposes `users.new_kind`); the users afterwards.
    fn user_create(
        &mut self,
        name: &str,
        password: &str,
        kind: users::UserKind,
    ) -> PortResult<Vec<LocalUser>>;
    /// Removes a user (the files stay); the users afterwards.
    fn user_remove(&mut self, name: &str) -> PortResult<Vec<LocalUser>>;
    /// Sets a new password; the users afterwards.
    fn user_password(&mut self, name: &str, password: &str) -> PortResult<Vec<LocalUser>>;
    /// The installed apps with their permissions.
    fn apps(&mut self) -> PortResult<Vec<AppEntry>>;
    /// The installed apps that declare `kind` (the choices of `apps.default.<kind>`).
    fn candidates(&mut self, kind: launcher::Kind) -> PortResult<Vec<String>>;
    /// Takes a granted permission back; whether it is still granted.
    fn app_revoke(&mut self, app: &str, permission: &str) -> PortResult<bool>;
    /// The device's own storage first, then the mounted drives.
    fn drives(&mut self) -> PortResult<Vec<Drive>>;
}

/// The three services over the bus.
pub struct BusSystem {
    client: Client,
}

impl std::fmt::Debug for BusSystem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusSystem")
    }
}

/// Most apps whose permissions are read for the Apps page.
pub const MAX_APPS_SHOWN: usize = 128;

impl BusSystem {
    /// The services reached through `client`.
    pub fn new(client: Client) -> BusSystem {
        BusSystem { client }
    }

    fn users_of(reply: PortResult<users::UsersReply>) -> PortResult<Vec<LocalUser>> {
        reply?
            .outcome
            .map(|u| u.users)
            .map_err(|r| PortError::Refused(r.reason))
    }
}

impl SystemPort for BusSystem {
    fn users(&mut self) -> PortResult<Vec<LocalUser>> {
        let reply = self.client.call(&users::ListUsers {}, CALL_TIMEOUT);
        Self::users_of(reply.map_err(port_error))
    }

    fn user_create(
        &mut self,
        name: &str,
        password: &str,
        kind: users::UserKind,
    ) -> PortResult<Vec<LocalUser>> {
        let call = users::CreateUser {
            name: name.to_owned(),
            display_name: name.to_owned(),
            kind,
            password: users::Secret::new(password.to_owned()),
            confirm: users::Secret::new(password.to_owned()),
        };
        // The account service waits for the administrator in the system dialog.
        Self::users_of(self.client.call(&call, SLOW_TIMEOUT).map_err(port_error))
    }

    fn user_remove(&mut self, name: &str) -> PortResult<Vec<LocalUser>> {
        let call = users::RemoveUser {
            user: name.to_owned(),
            data: users::RemoveData::Keep,
        };
        Self::users_of(self.client.call(&call, SLOW_TIMEOUT).map_err(port_error))
    }

    fn user_password(&mut self, name: &str, password: &str) -> PortResult<Vec<LocalUser>> {
        let call = users::SetPassword {
            user: name.to_owned(),
            current: None,
            new: users::Secret::new(password.to_owned()),
            confirm: users::Secret::new(password.to_owned()),
        };
        Self::users_of(self.client.call(&call, SLOW_TIMEOUT).map_err(port_error))
    }

    fn apps(&mut self) -> PortResult<Vec<AppEntry>> {
        let list = self
            .client
            .call(&launcher::List {}, CALL_TIMEOUT)
            .map_err(port_error)?;
        let mut apps = Vec::new();
        for app in list.apps.into_iter().take(MAX_APPS_SHOWN) {
            let ask = launcher::Permissions {
                app: app.app.clone(),
            };
            let declared = self
                .client
                .call(&ask, CALL_TIMEOUT)
                .map(|r| r.declared)
                .unwrap_or_default();
            apps.push(AppEntry {
                id: app.app,
                version: app.version,
                permissions: declared.into_iter().map(|p| (p.name, p.granted)).collect(),
            });
        }
        Ok(apps)
    }

    fn candidates(&mut self, kind: launcher::Kind) -> PortResult<Vec<String>> {
        let ask = launcher::Candidates { kind };
        let reply = self.client.call(&ask, CALL_TIMEOUT).map_err(port_error)?;
        Ok(reply.apps)
    }

    fn app_revoke(&mut self, app: &str, permission: &str) -> PortResult<bool> {
        let call = launcher::Revoke {
            app: app.to_owned(),
            permission: permission.to_owned(),
        };
        let reply = self.client.call(&call, CALL_TIMEOUT).map_err(port_error)?;
        Ok(reply.granted)
    }

    fn drives(&mut self) -> PortResult<Vec<Drive>> {
        let reply = self
            .client
            .call(&volumes::Usage {}, CALL_TIMEOUT)
            .map_err(port_error)?;
        let drive = |s: volumes::Space| Drive {
            name: s.name,
            total_bytes: s.total_bytes,
            free_bytes: s.free_bytes,
        };
        Ok(reply.places.into_iter().map(drive).collect())
    }
}
