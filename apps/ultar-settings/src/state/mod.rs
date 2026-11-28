//! The app's state: pure data, changed only by [`crate::update`].

pub mod display;
pub mod foreign;
pub mod mesh;
pub mod more;

use crate::areas::{PageKind, AREAS};
use ultar_bus::messages::account_service::AccountState;
use ultar_bus::messages::device_service::DeviceInfo;
use ultar_bus::messages::settings::PanelInfo;
use ultar_bus::Text;

pub use display::{DisplayEntry, DisplayMode, DisplayState};
pub use more::MoreState;

/// A page below an area page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SubPage {
    /// The area itself.
    #[default]
    None,
    /// Displays > Night light (schedule and warmth).
    NightLight,
    /// Accounts > Ultar account > Create account.
    CreateAccount,
    /// The page of one paired device (Settings › Mesh).
    MeshDevice,
    /// The page of one app from another system.
    ForeignApp,
}

/// State of the Drivers page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DriversState {
    /// Devices as the device daemon reports them.
    pub devices: Vec<DeviceInfo>,
    /// Notices of the daemon.
    pub notices: Vec<Text>,
    /// The daemon answered.
    pub known: bool,
    /// Why the list could not be read.
    pub error: Option<Text>,
    /// Firmware: phase key of the updater, and its last error key.
    pub firmware: Option<(String, Option<String>)>,
    /// A self-test (rescan) is running.
    pub testing: bool,
}

impl DriversState {
    /// Devices without a working driver.
    pub fn without_driver(&self) -> Vec<&DeviceInfo> {
        use ultar_bus::messages::device_service::DevicePhase;
        self.devices
            .iter()
            .filter(|d| matches!(d.phase, DevicePhase::NoDriver | DevicePhase::Failed))
            .collect()
    }
}

/// State of the account page: the service's state plus the form.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AccountView {
    /// Last state of the account service (`None`: not reachable).
    pub state: Option<AccountState>,
    /// Why the service could not be reached or refused.
    pub error: Option<Text>,
    /// Typed user name.
    pub user: String,
    /// Typed server address.
    pub server: String,
    /// "Copied" confirmation is showing.
    pub copied: bool,
}

/// What an open dialog is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DialogPurpose {
    /// Switch the driver of device row `row` to alternative `alt`.
    DriverSwitch {
        /// Device row.
        row: usize,
        /// Index into its alternatives.
        alt: usize,
    },
    /// Update from a drive: path of the bundle.
    FirmwareUsb,
    /// Change password (old, new, new again).
    ChangePassword,
    /// Delete the account (password).
    DeleteAccount,
    /// Remove device `n` of the account.
    RemoveDevice(usize),
    /// The QR code of the Ultar number, large.
    Qr,
    /// Users: add a user (name, password, password again).
    UserAdd,
    /// Users: remove user `n`.
    UserRemove(usize),
    /// Users: new password for user `n` (password, password again).
    UserPassword(usize),
    /// This device shows a pairing code.
    MeshPairShow,
    /// The code of a nearby device is typed.
    MeshPairJoin,
    /// "Unpair this device?".
    MeshUnpair,
    /// The key of the cloud model is typed.
    CloudKey,
    /// "Forget the key?".
    CloudClear,
    /// "Forget what the helper did?".
    HelperForget,
    /// The address of a provider of the person's own choice is typed.
    CloudAddress,
    /// "Remove this app from another system?".
    ForeignRemove,
}

/// An open dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DialogState {
    /// Purpose.
    pub purpose: DialogPurpose,
    /// Error shown in the dialog (fields differ, refused).
    pub error: Option<Text>,
}

/// What a status page (network, Bluetooth, sound) shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StatusView {
    /// `net.state`.
    pub net: Option<ultar_bus::messages::net::NetState>,
    /// `bluetooth.state`.
    pub bluetooth: Option<ultar_bus::messages::bluetooth::BluetoothState>,
    /// `audio.state`.
    pub audio: Option<ultar_bus::messages::audio::AudioState>,
    /// `bluetooth.devices` and the `devices_revision` it was read at (Bluetooth page).
    pub bt_devices: (Vec<ultar_bus::messages::bluetooth::Device>, Option<u32>),
    /// A Bluetooth search ran while the page was open (an empty list after it: "none found").
    pub bt_searched: bool,
    /// The device mesh (Settings › Mesh).
    pub mesh: mesh::MeshView,
    /// Apps from other systems (Settings › Apps from other systems).
    pub foreign: foreign::ForeignView,
    /// The cloud model (Settings › Assistant; `None`: no such service).
    pub cloud: Option<ultar_bus::messages::llm_cloud::CloudState>,
    /// What the helper thinks with right now (`None`: no helper).
    pub brain: Option<ultar_bus::messages::agent::Brain>,
    /// What the helper did lately (`None`: no helper).
    pub activity: Option<ultar_bus::messages::agent::Activity>,
    /// Why the open status page's service could not be read.
    pub error: Option<Text>,
}

/// The whole app.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct State {
    /// Index into [`AREAS`] of the open area.
    pub area: usize,
    /// Page below the area.
    pub sub: SubPage,
    /// Text in the sidebar search field.
    pub query: String,
    /// Displays page.
    pub display: DisplayState,
    /// Drivers page.
    pub drivers: DriversState,
    /// Account page.
    pub account: AccountView,
    /// Network, Bluetooth and sound pages: what the service reported, or why it could not be read.
    pub status: StatusView,
    /// About, Users, Apps and Storage pages: what the system and their services reported.
    pub more: MoreState,
    /// Panels of settingsd (for generic pages and search).
    pub panels: Vec<PanelInfo>,
    /// Why settingsd could not be read.
    pub panels_error: Option<Text>,
    /// The open dialog.
    pub dialog: Option<DialogState>,
    /// A short message for the user (toast).
    pub toast: Option<Text>,
    /// The area open before this one (account page: back).
    pub prev_area: Option<usize>,
    /// The surface is phone-sized (set by the app from the toolkit's size class).
    pub compact: bool,
    /// Compact size class: the list of areas is shown instead of an area page.
    pub compact_list: bool,
    /// A rebuild of the content pane is needed.
    pub dirty: bool,
}

impl State {
    /// The page the open area shows.
    pub fn page(&self) -> PageKind {
        AREAS.get(self.area).map_or(PageKind::NoService, |a| a.page)
    }

    /// The settingsd panel `id`, if listed.
    pub fn panel(&self, id: &str) -> Option<&PanelInfo> {
        self.panels.iter().find(|p| p.id == id)
    }

    /// The kind the Users page proposes for a new user (`users.new_kind`); an invalid
    /// or missing value means the schema default, standard.
    pub fn new_user_kind(&self) -> ultar_bus::messages::account_local::UserKind {
        use ultar_bus::messages::account_local::UserKind;
        use ultar_bus::messages::settings::SettingValue;
        let value = self
            .panel("users")
            .and_then(|p| p.items.iter().find(|i| i.key == "users.new_kind"))
            .map(|i| &i.value);
        match value {
            Some(SettingValue::Text(t)) if t == "administrator" => UserKind::Administrator,
            _ => UserKind::Standard,
        }
    }
}
