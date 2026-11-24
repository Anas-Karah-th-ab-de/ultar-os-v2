//! The services behind the app, as ports (traits) – the only place with bus calls is
//! [`bus`]. Tests and the conformance suite drive the app through [`fake`].

pub mod bus;
pub mod fake;
pub mod system;
pub mod update_contract;

use ultar_bus::messages::account_service::{AccountService, AccountState};
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::device_service::{DeviceInfo, ListReply};
use ultar_bus::messages::display_service::{DetectReply, Output};
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::settings::{PanelInfo, SettingValue};
use ultar_bus::Text;

/// Why a service call did not do what was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortError {
    /// The service is not running or this app may not call it.
    Unavailable,
    /// The service refused, with the reason for the user.
    Refused(Text),
    /// The call failed on the way (timeout, broken connection, bad answer).
    Failed,
}

impl PortError {
    /// The text for the user.
    pub fn text(&self) -> Text {
        match self {
            PortError::Unavailable => Text::key("app.settings.error.unavailable"),
            PortError::Refused(reason) => reason.clone(),
            PortError::Failed => Text::key("app.settings.error.failed"),
        }
    }
}

impl std::fmt::Display for PortError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.text().key)
    }
}

impl std::error::Error for PortError {}

/// Result of a port call.
pub type PortResult<T> = Result<T, PortError>;

/// `settingsd`.
pub trait SettingsPort {
    /// Panels and items (one panel or all).
    fn list(&mut self, panel: Option<&str>) -> PortResult<Vec<PanelInfo>>;
    /// Changes one setting (risky ones start the 15 s confirm-or-revert in the service).
    fn set(&mut self, key: &str, value: SettingValue) -> PortResult<()>;
    /// `true` when settings changed since the last call (another app, a revert).
    fn changed(&mut self) -> bool {
        false
    }
}

/// The display service.
pub trait DisplayPort {
    /// The connected displays (last `display.outputs`).
    fn outputs(&mut self) -> PortResult<Vec<Output>>;
    /// Shows the numbers on the displays (ids in number order).
    fn identify(&mut self, order: Vec<String>) -> PortResult<u32>;
    /// Looks for displays again.
    fn detect(&mut self) -> PortResult<DetectReply>;
}

/// The device daemon.
pub trait DevicesPort {
    /// Devices with drivers.
    fn list(&mut self) -> PortResult<ListReply>;
    /// Switches the driver of a device.
    fn switch(&mut self, address: &str, driver: &str, version: &str) -> PortResult<DeviceInfo>;
    /// Returns to the previous driver.
    fn rollback(&mut self, address: &str) -> PortResult<DeviceInfo>;
    /// Probes every device again (self-test); the devices afterwards.
    fn rescan(&mut self) -> PortResult<Vec<DeviceInfo>>;
}

/// The updater (firmware and system updates).
pub trait UpdatePort {
    /// Starts a check; the result arrives as progress.
    fn check(&mut self) -> PortResult<()>;
    /// Installs an update bundle from a drive (signature checked by the service).
    fn offline(&mut self, path: &str) -> PortResult<()>;
    /// The last progress: phase key, error key.
    fn progress(&mut self) -> Option<(String, Option<String>)>;
}

/// A change asked of the account service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountRequest {
    /// Sign in.
    SignIn {
        /// User name.
        user: String,
        /// Password.
        password: String,
    },
    /// Create an account.
    Create {
        /// User name.
        user: String,
        /// Password.
        password: String,
        /// Recovery e-mail.
        email: Option<String>,
    },
    /// Sign this device out.
    SignOut,
    /// Change the server (signed out only).
    SetServer(String),
    /// Switch a service.
    SetService(AccountService, bool),
    /// Change the password.
    ChangePassword {
        /// Old password.
        old: String,
        /// New password.
        new: String,
    },
    /// Delete the account.
    Delete(String),
    /// Sign another device out.
    DeviceSignOut(String),
    /// Remove a device.
    DeviceRemove(String),
}

/// The account service.
pub trait AccountPort {
    /// The current state.
    fn state(&mut self) -> PortResult<AccountState>;
    /// Asks for a change; the new state.
    fn request(&mut self, request: AccountRequest) -> PortResult<AccountState>;
}

/// Read-only state of the network, Bluetooth and sound services (their retained topics
/// `net.state`, `bluetooth.state`, `audio.state`): the Network, Bluetooth and Sound pages.
pub trait StatusPort {
    /// The network service's state.
    fn net(&mut self) -> PortResult<NetState>;
    /// The Bluetooth service's state.
    fn bluetooth(&mut self) -> PortResult<BluetoothState>;
    /// The sound service's state.
    fn audio(&mut self) -> PortResult<AudioState>;
    /// `bluetooth.devices`: paired and found devices (Bluetooth page).
    fn bt_devices(&mut self) -> PortResult<Vec<ultar_bus::messages::bluetooth::Device>> {
        Err(PortError::Unavailable)
    }
    /// `bluetooth.scan` / `bluetooth.pair` (Bluetooth page).
    fn bt_call(&mut self, _call: BtCall) -> PortResult<()> {
        Err(PortError::Unavailable)
    }
}

/// A change asked of the Bluetooth service from its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BtCall {
    /// Look for devices (on) or stop.
    Scan(bool),
    /// Pair with a found device (by id).
    Pair(String),
}

/// Saving a file the user asked for (hardware plan) into the documents area.
pub trait FilesPort {
    /// Writes `bytes` atomically as `name`; the place shown to the user.
    fn save(&mut self, name: &str, bytes: &[u8]) -> PortResult<String>;
}

/// The clipboard.
pub trait ClipboardPort {
    /// Offers text to paste elsewhere.
    fn copy(&mut self, text: &str) -> PortResult<()>;
}

/// Every port the app uses.
pub struct Ports {
    /// settingsd.
    pub settings: Box<dyn SettingsPort>,
    /// Display service.
    pub display: Box<dyn DisplayPort>,
    /// Device daemon.
    pub devices: Box<dyn DevicesPort>,
    /// Updater.
    pub update: Box<dyn UpdatePort>,
    /// Account service.
    pub account: Box<dyn AccountPort>,
    /// Network, Bluetooth and sound state (read-only pages).
    pub status: Box<dyn StatusPort>,
    /// Users, apps and drives (Users, Apps and Storage pages).
    pub system: Box<dyn system::SystemPort>,
    /// Documents.
    pub files: Box<dyn FilesPort>,
    /// Clipboard.
    pub clipboard: Box<dyn ClipboardPort>,
    /// The installed release's os-release file (About: version and build), `None` when the
    /// system has none.
    pub os_release: Option<std::path::PathBuf>,
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Ports")
    }
}
