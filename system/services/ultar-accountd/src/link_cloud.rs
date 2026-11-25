//! The real account server for [`crate::link`]: `ultraos-server` over HTTPS through the
//! device client `ultar-cloud` (device key, signed registration and sessions).
//!
//! The device key and registration live in `<dir>/link/device` (`ultar_cloud::DeviceHome`,
//! private files). Errors are reduced to what the person can act on ([`LinkError`]).

use crate::link::{Enrolled, LinkError, RemoteDevice, Server};
use std::path::{Path, PathBuf};
use ultar_cloud::{Client, CloudError, DeviceHome, KeyStore};

/// `ultraos-server` reached with `ultar-cloud`.
pub struct CloudServer {
    home: PathBuf,
}

impl CloudServer {
    /// Device key and registration under `<service dir>/link/device`.
    pub fn new(service_dir: &Path) -> Self {
        Self {
            home: service_dir.join("link").join("device"),
        }
    }

    fn home(&self) -> DeviceHome {
        DeviceHome::new(self.home.clone())
    }
}

fn base(server: &str) -> String {
    format!("https://{server}")
}

fn account(server: &str, token: &str) -> Client {
    Client::new(&base(server), Some(token.to_owned()))
}

fn map(e: CloudError) -> LinkError {
    match e {
        CloudError::InvalidCredentials => LinkError::WrongPassword,
        CloudError::UsernameTaken => LinkError::NameTaken,
        CloudError::InvalidRequest | CloudError::DeviceLimit => LinkError::Rejected,
        CloudError::Transport(_) => LinkError::Unreachable,
        CloudError::RateLimited { .. } | CloudError::CooldownActive { .. } => LinkError::Throttled,
        CloudError::AuthRequired
        | CloudError::NotSignedIn
        | CloudError::DeviceRetired
        | CloudError::DeviceUnknown
        | CloudError::DeviceTaken => LinkError::SessionEnded,
        CloudError::NotApproved { .. } => LinkError::NotManager,
        _ => LinkError::Server,
    }
}

impl CloudServer {
    /// A removed or replaced device key is of no use any more: forget it, so that the next
    /// sign-in registers this device anew.
    fn forget_retired(&self, e: &CloudError) {
        if matches!(e, CloudError::DeviceRetired | CloudError::DeviceTaken) {
            let _ = self.home().reset();
        }
    }
}

impl Server for CloudServer {
    fn health(&self, server: &str) -> Result<(), LinkError> {
        Client::new(&base(server), None)
            .categories()
            .map(|_| ())
            .map_err(map)
    }

    fn login(&self, server: &str, user: &str, password: &str) -> Result<String, LinkError> {
        Client::new(&base(server), None)
            .login(user, password)
            .map_err(map)
    }

    fn register(&self, server: &str, user: &str, password: &str) -> Result<String, LinkError> {
        Client::new(&base(server), None)
            .register(user, password)
            .map_err(map)
    }

    fn enroll(&self, server: &str, token: &str, name: &str) -> Result<Enrolled, LinkError> {
        let home = self.home();
        let client = account(server, token);
        let number = home.register_number(&client, name, None).map_err(|e| {
            self.forget_retired(&e);
            map(e)
        })?;
        let key = home.key_store().map_err(map)?;
        let session = client.device_session(&key).map_err(|e| {
            self.forget_retired(&e);
            map(e)
        })?;
        Ok(Enrolled {
            number,
            device_token: session.token,
        })
    }

    fn devices(&self, server: &str, token: &str) -> Result<Vec<RemoteDevice>, LinkError> {
        let key = self.home().key_store().map_err(map)?;
        let device = account(server, token).as_device(&key).map_err(map)?;
        Ok(device
            .devices()
            .map_err(map)?
            .into_iter()
            .map(|d| RemoteDevice {
                id: d.device_id,
                name: d.name,
                current: d.current,
                last_seen: u64::try_from(d.last_seen).unwrap_or(0),
            })
            .collect())
    }

    fn change_password(
        &self,
        server: &str,
        token: &str,
        old: &str,
        new: &str,
    ) -> Result<String, LinkError> {
        account(server, token)
            .change_password(old, new)
            .map_err(map)
    }

    fn remove_device(&self, server: &str, token: &str, device: &str) -> Result<(), LinkError> {
        let key = self.home().key_store().map_err(map)?;
        account(server, token)
            .remove_device(&key, device)
            .map_err(map)
    }

    fn signal_hello(&self, server: &str) -> Result<String, LinkError> {
        let proof = self.home().signal_proof(&base(server)).map_err(map)?;
        proof.hello().map_err(map)
    }

    fn signal_answer(&self, server: &str, challenge: &str) -> Result<String, LinkError> {
        let proof = self.home().signal_proof(&base(server)).map_err(map)?;
        proof.answer(challenge).map_err(map)
    }

    fn device_key(&self) -> Result<[u8; 32], LinkError> {
        let key = self.home().key_store().map_err(map)?;
        key.public_key().map_err(|_| LinkError::Server)
    }

    fn sign(&self, message: &[u8]) -> Result<Vec<u8>, LinkError> {
        let key = self.home().key_store().map_err(map)?;
        key.sign(message)
            .map(|s| s.to_vec())
            .map_err(|_| LinkError::Server)
    }

    fn turn(&self, server: &str, token: &str) -> Result<(String, String, String), LinkError> {
        let c = account(server, token).turn_credentials().map_err(map)?;
        let host = c.udp_server().ok_or(LinkError::Server)?;
        Ok((host, c.username, c.credential))
    }
}
