//! In-memory services for tests and the design-conformance suite (never used by the binary): they
//! keep their data in a shared [`World`] and record every call, so a test can check that a
//! click did what CLICKS.md says.

use super::{
    AccountPort, AccountRequest, ClipboardPort, DevicesPort, DisplayPort, FilesPort, PortError,
    PortResult, Ports, SettingsPort, UpdatePort,
};
use std::cell::RefCell;
use std::rc::Rc;
use ultar_bus::messages::account_service::{AccountState, Phase};
use ultar_bus::messages::device_service::{DeviceInfo, ListReply};
use ultar_bus::messages::display_service::{DetectReply, Output};
use ultar_bus::messages::settings::{PanelInfo, SettingValue};
use ultar_bus::Text;

/// The data behind the fake services.
#[derive(Debug, Clone, Default)]
pub struct World {
    /// settingsd panels (`None`: service down).
    pub panels: Option<Vec<PanelInfo>>,
    /// Display service report (`None`: service down).
    pub outputs: Option<Vec<Output>>,
    /// Devices (`None`: daemon down).
    pub devices: Option<Vec<DeviceInfo>>,
    /// Account (`None`: service down).
    pub account: Option<AccountState>,
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

/// Ports backed by `world`.
pub fn ports(world: &Shared) -> Ports {
    let f = || Box::new(Fake(world.clone()));
    Ports {
        settings: f(),
        display: f(),
        devices: f(),
        update: f(),
        account: f(),
        files: f(),
        clipboard: f(),
    }
}
