//! In-memory services for tests and the design-conformance suite (never used by the binary): they
//! keep their data in a shared [`World`] and record every call, so a test can check that a
//! click did what CLICKS.md says.

use super::system::{AppEntry, Drive, LocalUser, SystemPort};
use super::{
    AccountPort, AccountRequest, ClipboardPort, DevicesPort, DisplayPort, FilesPort, PortError,
    PortResult, Ports, SettingsPort, StatusPort, UpdatePort,
};
use std::cell::RefCell;
use std::rc::Rc;
use ultar_bus::messages::account_service::{AccountState, Phase};
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::device_service::{DeviceInfo, ListReply};
use ultar_bus::messages::display_service::{DetectReply, Output};
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::settings::{PanelInfo, SettingValue};
use ultar_bus::Text;

/// The data behind the fake services.
#[derive(Debug, Clone, Default)]
pub struct World {
    /// os-release file of the installed system (About).
    pub os_release: Option<std::path::PathBuf>,
    /// settingsd panels (`None`: service down).
    pub panels: Option<Vec<PanelInfo>>,
    /// Display service report (`None`: service down).
    pub outputs: Option<Vec<Output>>,
    /// Devices (`None`: daemon down).
    pub devices: Option<Vec<DeviceInfo>>,
    /// Account (`None`: service down).
    pub account: Option<AccountState>,
    /// Network state (`None`: service down).
    pub net: Option<NetState>,
    /// Bluetooth state (`None`: service down).
    pub bluetooth: Option<BluetoothState>,
    /// Sound state (`None`: service down).
    pub audio: Option<AudioState>,
    /// Bluetooth devices (`None`: service down).
    pub bt_devices: Option<Vec<ultar_bus::messages::bluetooth::Device>>,
    /// Users, apps and drives (`None` each: service down).
    pub system: SystemWorld,
    /// Refuse the next call with this reason.
    pub refuse: Option<Text>,
    /// Every call in order (`service.method key/arg`).
    pub log: Vec<String>,
    /// Files saved: (name, bytes).
    pub files: Vec<(String, Vec<u8>)>,
    /// Clipboard content.
    pub clipboard: Option<String>,
    /// settings changed elsewhere since the last poll.
    pub changed: bool,
}

/// Shared handle of the world.
pub type Shared = Rc<RefCell<World>>;

#[derive(Debug, Clone)]
struct Fake(Shared);

impl Fake {
    fn call(&self, what: String) -> PortResult<()> {
        let mut w = self.0.borrow_mut();
        w.log.push(what);
        match w.refuse.take() {
            Some(reason) => Err(PortError::Refused(reason)),
            None => Ok(()),
        }
    }
}

fn set_in(panels: &mut [PanelInfo], key: &str, value: SettingValue) -> bool {
    for item in panels.iter_mut().flat_map(|p| p.items.iter_mut()) {
        if item.key == key {
            item.value = value;
            return true;
        }
    }
    false
}

impl SettingsPort for Fake {
    fn list(&mut self, panel: Option<&str>) -> PortResult<Vec<PanelInfo>> {
        let w = self.0.borrow();
        let panels = w.panels.clone().ok_or(PortError::Unavailable)?;
        Ok(panels
            .into_iter()
            .filter(|p| panel.is_none_or(|id| p.id == id))
            .collect())
    }

    fn set(&mut self, key: &str, value: SettingValue) -> PortResult<()> {
        self.call(format!("settings.set {key}={value:?}"))?;
        let mut w = self.0.borrow_mut();
        let panels = w.panels.as_mut().ok_or(PortError::Unavailable)?;
        if set_in(panels, key, value) {
            Ok(())
        } else {
            Err(PortError::Refused(Text::key("app.settings.error.code.1")))
        }
    }

    fn changed(&mut self) -> bool {
        std::mem::take(&mut self.0.borrow_mut().changed)
    }
}

impl DisplayPort for Fake {
    fn outputs(&mut self) -> PortResult<Vec<Output>> {
        self.0
            .borrow()
            .outputs
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn identify(&mut self, order: Vec<String>) -> PortResult<u32> {
        self.call(format!("display.identify {}", order.join(",")))?;
        Ok(order.len() as u32)
    }

    fn detect(&mut self) -> PortResult<DetectReply> {
        self.call("display.detect".into())?;
        let found = self.0.borrow().outputs.as_ref().map_or(0, Vec::len) as u32;
        Ok(DetectReply { found, new: 0 })
    }
}

impl DevicesPort for Fake {
    fn list(&mut self) -> PortResult<ListReply> {
        let devices = self
            .0
            .borrow()
            .devices
            .clone()
            .ok_or(PortError::Unavailable)?;
        Ok(ListReply {
            devices,
            notices: Vec::new(),
            kernel_switch_supported: true,
        })
    }

    fn switch(&mut self, address: &str, driver: &str, version: &str) -> PortResult<DeviceInfo> {
        self.call(format!("devices.switch {address} {driver} {version}"))?;
        let mut w = self.0.borrow_mut();
        let d = w
            .devices
            .as_mut()
            .and_then(|l| l.iter_mut().find(|d| d.address == address))
            .ok_or(PortError::Failed)?;
        let target = d.alternatives.iter().find(|a| a.id == driver).cloned();
        d.previous = d.active.as_ref().map(|a| a.id.clone());
        d.active = target;
        Ok(d.clone())
    }

    fn rollback(&mut self, address: &str) -> PortResult<DeviceInfo> {
        self.call(format!("devices.rollback {address}"))?;
        let w = self.0.borrow();
        w.devices
            .as_ref()
            .and_then(|l| l.iter().find(|d| d.address == address))
            .cloned()
            .ok_or(PortError::Failed)
    }

    fn rescan(&mut self) -> PortResult<Vec<DeviceInfo>> {
        self.call("devices.rescan".into())?;
        self.0
            .borrow()
            .devices
            .clone()
            .ok_or(PortError::Unavailable)
    }
}

impl UpdatePort for Fake {
    fn check(&mut self) -> PortResult<()> {
        self.call("update.check".into())
    }

    fn offline(&mut self, path: &str) -> PortResult<()> {
        self.call(format!("update.offline {path}"))
    }

    fn progress(&mut self) -> Option<(String, Option<String>)> {
        None
    }
}

impl AccountPort for Fake {
    fn state(&mut self) -> PortResult<AccountState> {
        self.0
            .borrow()
            .account
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn request(&mut self, request: AccountRequest) -> PortResult<AccountState> {
        let what = match &request {
            AccountRequest::SignIn { user, .. } => format!("account.sign_in {user}"),
            AccountRequest::Create { user, .. } => format!("account.create {user}"),
            AccountRequest::SignOut => "account.sign_out".into(),
            AccountRequest::SetServer(s) => format!("account.set_server {s}"),
            AccountRequest::SetService(s, on) => format!("account.set_service {s:?} {on}"),
            AccountRequest::ChangePassword { .. } => "account.change_password".into(),
            AccountRequest::Delete(_) => "account.delete".into(),
            AccountRequest::DeviceSignOut(d) => format!("account.device_sign_out {d}"),
            AccountRequest::DeviceRemove(d) => format!("account.device_remove {d}"),
        };
        self.call(what)?;
        let mut w = self.0.borrow_mut();
        let state = w.account.as_mut().ok_or(PortError::Unavailable)?;
        match request {
            AccountRequest::SignIn { user, .. } | AccountRequest::Create { user, .. } => {
                state.phase = Phase::SignedIn;
                state.user = user;
            }
            AccountRequest::SignOut | AccountRequest::Delete(_) => state.phase = Phase::SignedOut,
            AccountRequest::SetServer(s) => state.server = s,
            AccountRequest::SetService(s, on) => {
                state.services.retain(|x| *x != s);
                if on {
                    state.services.push(s);
                }
            }
            AccountRequest::DeviceRemove(id) => state.devices.retain(|d| d.id != id),
            AccountRequest::ChangePassword { .. } | AccountRequest::DeviceSignOut(_) => {}
        }
        Ok(state.clone())
    }
}

impl FilesPort for Fake {
    fn save(&mut self, name: &str, bytes: &[u8]) -> PortResult<String> {
        self.call(format!("files.save {name}"))?;
        self.0
            .borrow_mut()
            .files
            .push((name.to_owned(), bytes.to_vec()));
        Ok(format!("documents/{name}"))
    }
}

impl ClipboardPort for Fake {
    fn copy(&mut self, text: &str) -> PortResult<()> {
        self.call(format!("clipboard.copy {text}"))?;
        self.0.borrow_mut().clipboard = Some(text.to_owned());
        Ok(())
    }
}

impl StatusPort for Fake {
    fn net(&mut self) -> PortResult<NetState> {
        self.0.borrow().net.clone().ok_or(PortError::Unavailable)
    }

    fn bluetooth(&mut self) -> PortResult<BluetoothState> {
        self.0
            .borrow()
            .bluetooth
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn audio(&mut self) -> PortResult<AudioState> {
        self.0.borrow().audio.clone().ok_or(PortError::Unavailable)
    }

    fn bt_devices(&mut self) -> PortResult<Vec<ultar_bus::messages::bluetooth::Device>> {
        self.0
            .borrow()
            .bt_devices
            .clone()
            .ok_or(PortError::Unavailable)
    }

    fn bt_call(&mut self, c: super::BtCall) -> PortResult<()> {
        self.call(match c {
            super::BtCall::Scan(on) => format!("bluetooth.scan {on}"),
            super::BtCall::Pair(id) => format!("bluetooth.pair {id}"),
        })
    }
}

/// What the fake account service, launcher, volume service and mesh hold.
#[derive(Debug, Clone, Default)]
pub struct SystemWorld {
    /// Local users (`None`: account service down).
    pub users: Option<Vec<LocalUser>>,
    /// Installed apps (`None`: launcher down).
    pub apps: Option<Vec<AppEntry>>,
    /// The installed apps that declare each kind (the default-app choices).
    pub candidates: Vec<(ultar_bus::messages::launcher::Kind, Vec<String>)>,
    /// Mounted drives (`None`: volume service down).
    pub drives: Option<Vec<Drive>>,
    /// The device mesh (`None`: service down).
    pub mesh: Option<ultar_bus::messages::mesh::MeshState>,
    /// The cloud model (`None`: no such service).
    pub cloud: Option<ultar_bus::messages::llm_cloud::CloudState>,
    /// What the helper did (`None`: no helper).
    pub activity: Option<ultar_bus::messages::agent::Activity>,
}

fn local_user(name: &str, uid: u32) -> LocalUser {
    LocalUser {
        name: name.to_owned(),
        uid,
        display_name: name.to_owned(),
        kind: ultar_bus::messages::account_local::UserKind::Standard,
        picture: ultar_bus::messages::account_local::PictureInfo::Initials,
        auto_login: false,
        current: false,
    }
}

impl SystemPort for Fake {
    fn users(&mut self) -> PortResult<Vec<LocalUser>> {
        let users = self.0.borrow().system.users.clone();
        users.ok_or(PortError::Unavailable)
    }

    fn user_create(&mut self, name: &str, _password: &str) -> PortResult<Vec<LocalUser>> {
        self.call(format!("account.user_create {name}"))?;
        let mut w = self.0.borrow_mut();
        let users = w.system.users.as_mut().ok_or(PortError::Unavailable)?;
        users.push(local_user(name, 1000 + users.len() as u32));
        Ok(users.clone())
    }

    fn user_remove(&mut self, name: &str) -> PortResult<Vec<LocalUser>> {
        self.call(format!("account.user_remove {name}"))?;
        let mut w = self.0.borrow_mut();
        let users = w.system.users.as_mut().ok_or(PortError::Unavailable)?;
        users.retain(|u| u.name != name);
        Ok(users.clone())
    }

    fn user_password(&mut self, name: &str, _password: &str) -> PortResult<Vec<LocalUser>> {
        self.call(format!("account.user_password {name}"))?;
        self.users()
    }

    fn apps(&mut self) -> PortResult<Vec<AppEntry>> {
        let apps = self.0.borrow().system.apps.clone();
        apps.ok_or(PortError::Unavailable)
    }

    fn candidates(
        &mut self,
        kind: ultar_bus::messages::launcher::Kind,
    ) -> PortResult<Vec<String>> {
        let w = self.0.borrow();
        if w.system.apps.is_none() {
            return Err(PortError::Unavailable);
        }
        Ok(w.system
            .candidates
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, apps)| apps.clone())
            .unwrap_or_default())
    }

    fn app_revoke(&mut self, app: &str, permission: &str) -> PortResult<bool> {
        self.call(format!("launcher.revoke {app} {permission}"))?;
        let mut w = self.0.borrow_mut();
        let apps = w.system.apps.as_mut().ok_or(PortError::Unavailable)?;
        for entry in apps.iter_mut().filter(|a| a.id == app) {
            for p in entry.permissions.iter_mut().filter(|p| p.0 == permission) {
                p.1 = false;
            }
        }
        Ok(false)
    }

    fn drives(&mut self) -> PortResult<Vec<Drive>> {
        let drives = self.0.borrow().system.drives.clone();
        drives.ok_or(PortError::Unavailable)
    }
}

/// Ports backed by `world`.
pub fn ports(world: &Shared) -> Ports {
    let f = || Box::new(Fake(world.clone()));
    Ports {
        os_release: world.borrow().os_release.clone(),
        settings: f(),
        display: f(),
        devices: f(),
        update: f(),
        account: f(),
        status: f(),
        mesh: Box::new(super::mesh::FakeMesh(world.clone())),
        cloud: Box::new(super::cloud::FakeCloud(world.clone())),
        system: f(),
        files: f(),
        clipboard: f(),
    }
}
