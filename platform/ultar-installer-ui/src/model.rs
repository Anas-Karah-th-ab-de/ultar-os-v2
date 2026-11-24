//! Everything the installer shows: the wizard plus what the services reported.

use crate::wizard::Wizard;
use ultar_bus::messages::device_service::DeviceInfo;
use ultar_bus::Text;

/// The installer's state.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Model {
    /// Entries and steps.
    pub wizard: Wizard,
    /// Devices of the device daemon (step 7).
    pub devices: Vec<DeviceInfo>,
    /// Why the device list could not be read.
    pub devices_error: Option<Text>,
    /// A network connection is known (no network service in this version: `false`).
    pub network: bool,
    /// Bytes written of bytes in all (step 15).
    pub progress: (u64, u64),
    /// Why the installation failed (step 15).
    pub failure: Option<Text>,
    /// The "Cancel the installation?" dialog is open.
    pub cancel_dialog: bool,
}

impl Model {
    /// Devices without a working driver.
    pub fn without_driver(&self) -> Vec<&DeviceInfo> {
        use ultar_bus::messages::device_service::DevicePhase;
        self.devices
            .iter()
            .filter(|d| matches!(d.phase, DevicePhase::NoDriver | DevicePhase::Failed))
            .collect()
    }
}
