//! The services the installer uses besides the disks: the device daemon (step 7) and a place
//! to save the hardware plan (the installation medium or another drive).

use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::device_service as dev;
use ultar_bus::{BusError, Client, ErrorCode, Text};

/// Why a call did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// Service not running / not permitted.
    Unavailable,
    /// Refused with a reason.
    Refused(Text),
    /// Failed on the way.
    Failed,
}

impl PortError {
    /// Text for the user.
    pub fn text(&self) -> Text {
        match self {
            PortError::Unavailable => Text::key("installer.error.unavailable"),
            PortError::Refused(t) => t.clone(),
            PortError::Failed => Text::key("installer.error.failed"),
        }
    }
}

/// The device daemon.
pub trait Devices {
    /// Devices.
    fn list(&mut self) -> Result<Vec<dev::DeviceInfo>, PortError>;
    /// Tries another driver live.
    fn switch(
        &mut self,
        address: &str,
        driver: &str,
        version: &str,
    ) -> Result<dev::DeviceInfo, PortError>;
    /// Tests one device again (rescan of its address).
    fn test(&mut self, address: &str) -> Result<dev::DeviceInfo, PortError>;
}

/// Where hardware plans are saved.
pub trait PlanStore {
    /// Saves; the place for the user.
    fn save(&mut self, name: &str, bytes: &[u8]) -> Result<String, PortError>;
}

/// Calls of the device daemon may probe hardware.
const TIMEOUT: Duration = Duration::from_secs(30);

fn port(e: BusError) -> PortError {
    match e {
        BusError::Remote(
            ErrorCode::UnknownService | ErrorCode::PermissionDenied | ErrorCode::ServiceGone,
        )
        | BusError::NotConnected
        | BusError::Closed => PortError::Unavailable,
        _ => PortError::Failed,
    }
}

/// The device daemon over the bus.
pub struct BusDevices(pub Client);

impl std::fmt::Debug for BusDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusDevices")
    }
}

impl Devices for BusDevices {
    fn list(&mut self) -> Result<Vec<dev::DeviceInfo>, PortError> {
        Ok(self.0.call(&dev::List {}, TIMEOUT).map_err(port)?.devices)
    }

    fn switch(
        &mut self,
        address: &str,
        driver: &str,
        version: &str,
    ) -> Result<dev::DeviceInfo, PortError> {
        let call = dev::Switch {
            address: address.into(),
            driver: driver.into(),
            version: version.into(),
        };
        match self.0.call(&call, TIMEOUT).map_err(port)?.outcome {
            dev::Outcome::Done(info) => Ok(*info),
            dev::Outcome::Refused(r) => Err(PortError::Refused(r.reason)),
        }
    }

    fn test(&mut self, address: &str) -> Result<dev::DeviceInfo, PortError> {
        let call = dev::Rescan {
            address: Some(address.into()),
        };
        let reply = self.0.call(&call, TIMEOUT).map_err(port)?;
        let devices = reply.devices.map_err(|r| PortError::Refused(r.reason))?;
        devices
            .into_iter()
            .find(|d| d.address == address)
            .ok_or(PortError::Failed)
    }
}

/// A folder on the medium (atomic writes).
#[derive(Debug, Clone)]
pub struct FolderStore(pub PathBuf);

impl PlanStore for FolderStore {
    fn save(&mut self, name: &str, bytes: &[u8]) -> Result<String, PortError> {
        let ok = !name.is_empty()
            && name.len() <= 128
            && !name.starts_with('.')
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
        if !ok {
            return Err(PortError::Failed);
        }
        let target = self.0.join(name);
        let temp = self.0.join(format!(".{name}.tmp"));
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&self.0)?;
            let mut f = std::fs::File::create(&temp)?;
            f.write_all(bytes)?;
            f.sync_all()?;
            std::fs::rename(&temp, &target)?;
            std::fs::File::open(&self.0)?.sync_all()
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temp);
            PortError::Failed
        })?;
        Ok(target.display().to_string())
    }
}

/// In-memory services for tests (feature `testkit`).
#[cfg(any(test, feature = "testkit"))]
pub mod fake {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Data and call log.
    #[derive(Debug, Default)]
    pub struct World {
        /// Devices (`None`: daemon down).
        pub devices: Option<Vec<dev::DeviceInfo>>,
        /// Calls.
        pub log: Vec<String>,
        /// Saved files.
        pub files: Vec<(String, Vec<u8>)>,
    }

    /// Shared world.
    pub type Shared = Rc<RefCell<World>>;

    /// A fake for both ports.
    #[derive(Debug, Clone)]
    pub struct Fake(pub Shared);

    impl Devices for Fake {
        fn list(&mut self) -> Result<Vec<dev::DeviceInfo>, PortError> {
            self.0
                .borrow()
                .devices
                .clone()
                .ok_or(PortError::Unavailable)
        }

        fn switch(
            &mut self,
            address: &str,
            driver: &str,
            version: &str,
        ) -> Result<dev::DeviceInfo, PortError> {
            let mut w = self.0.borrow_mut();
            w.log
                .push(format!("devices.switch {address} {driver} {version}"));
            let d = w
                .devices
                .as_mut()
                .and_then(|l| l.iter_mut().find(|d| d.address == address))
                .ok_or(PortError::Failed)?;
            d.active = d.alternatives.iter().find(|a| a.id == driver).cloned();
            Ok(d.clone())
        }

        fn test(&mut self, address: &str) -> Result<dev::DeviceInfo, PortError> {
            let mut w = self.0.borrow_mut();
            w.log.push(format!("devices.rescan {address}"));
            w.devices
                .as_ref()
                .and_then(|l| l.iter().find(|d| d.address == address))
                .cloned()
                .ok_or(PortError::Failed)
        }
    }

    impl PlanStore for Fake {
        fn save(&mut self, name: &str, bytes: &[u8]) -> Result<String, PortError> {
            let mut w = self.0.borrow_mut();
            w.log.push(format!("files.save {name}"));
            w.files.push((name.to_owned(), bytes.to_vec()));
            Ok(format!("medium/{name}"))
        }
    }
}
