//! Bus implementations of the ports – the only place of the app with bus calls.

use super::update_contract::{Check, Offline, Progress};
use super::BtCall;
use super::{
    AccountPort, AccountRequest, ClipboardPort, DevicesPort, DisplayPort, FilesPort, PortError,
    PortResult, SettingsPort, StatusPort, UpdatePort,
};
use std::cell::RefCell;
use std::io::Write;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use ultar_bus::messages::account_service as acc;
use ultar_bus::messages::audio::AudioState;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::device_service as dev;
use ultar_bus::messages::display_service as disp;
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::settings::{
    Changed, ListRequest, PanelInfo, PendingResolved, Set, SettingValue,
};
use ultar_bus::{BusError, Call, Client, ErrorCode, Text, TypedSubscription};

/// How long a call may take before the app reports it failed.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(5);
/// Calls that probe hardware (rescan, detect, driver switch) may take longer.
pub const SLOW_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest wait for a retained topic right after subscribing.
const RETAINED_WAIT: Duration = Duration::from_millis(300);

/// Maps a transport error to what the user is told.
pub fn port_error(error: BusError) -> PortError {
    match error {
        BusError::Remote(
            ErrorCode::UnknownService
            | ErrorCode::UnknownMethod
            | ErrorCode::PermissionDenied
            | ErrorCode::ServiceGone
            | ErrorCode::VersionMismatch,
        )
        | BusError::NotConnected
        | BusError::Closed => PortError::Unavailable,
        BusError::Remote(ErrorCode::Service(code)) => {
            PortError::Refused(Text::key(&format!("app.settings.error.code.{code}")))
        }
        _ => PortError::Failed,
    }
}

pub(crate) fn call<C: Call>(
    client: &Client,
    message: &C,
    timeout: Duration,
) -> PortResult<C::Reply> {
    client.call(message, timeout).map_err(port_error)
}

/// `settingsd` over the bus.
pub struct BusSettings {
    client: Client,
    changed: Option<TypedSubscription<Changed>>,
    resolved: Option<TypedSubscription<PendingResolved>>,
}

impl std::fmt::Debug for BusSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusSettings")
    }
}

impl BusSettings {
    /// Follows `settings.changed` and `settings.pending_resolved`.
    pub fn new(client: Client) -> BusSettings {
        BusSettings {
            changed: client.subscribe::<Changed>().ok(),
            resolved: client.subscribe::<PendingResolved>().ok(),
            client,
        }
    }
}

impl SettingsPort for BusSettings {
    fn changed(&mut self) -> bool {
        let mut any = false;
        if let Some(sub) = &self.changed {
            while let Ok(Some(_)) = sub.try_recv() {
                any = true;
            }
        }
        if let Some(sub) = &self.resolved {
            while let Ok(Some(_)) = sub.try_recv() {
                any = true;
            }
        }
        any
    }

    fn list(&mut self, panel: Option<&str>) -> PortResult<Vec<PanelInfo>> {
        let request = ListRequest {
            panel: panel.map(str::to_owned),
        };
        Ok(call(&self.client, &request, CALL_TIMEOUT)?.panels)
    }

    fn set(&mut self, key: &str, value: SettingValue) -> PortResult<()> {
        let set = Set {
            key: key.to_owned(),
            value,
        };
        let result = call(&self.client, &set, CALL_TIMEOUT).map(|_| ());
        // one line per change for the log (`app-settings`): which key and whether settingsd took
        // it (or the key of its refusal) - never the value
        let verdict = match &result {
            Ok(()) => "ok".to_owned(),
            Err(e) => e.text().key,
        };
        eprintln!("ultar-settings: set {key} {verdict}");
        result
    }
}

/// The display service over the bus.
pub struct BusDisplay {
    client: Client,
    outputs: Option<TypedSubscription<disp::Outputs>>,
    last: Option<Vec<disp::Output>>,
}

impl std::fmt::Debug for BusDisplay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusDisplay")
    }
}

impl BusDisplay {
    /// Subscribes to `display.outputs`.
    pub fn new(client: Client) -> BusDisplay {
        let outputs = client.subscribe::<disp::Outputs>().ok();
        BusDisplay {
            client,
            outputs,
            last: None,
        }
    }
}

impl DisplayPort for BusDisplay {
    fn outputs(&mut self) -> PortResult<Vec<disp::Output>> {
        let sub = self.outputs.as_ref().ok_or(PortError::Unavailable)?;
        if self.last.is_none() {
            if let Ok(first) = sub.recv_timeout(RETAINED_WAIT) {
                self.last = Some(first.outputs);
            }
        }
        while let Ok(Some(next)) = sub.try_recv() {
            self.last = Some(next.outputs);
        }
        let mut list = self.last.clone().ok_or(PortError::Unavailable)?;
        list.truncate(disp::MAX_OUTPUTS);
        Ok(list)
    }

    fn identify(&mut self, order: Vec<String>) -> PortResult<u32> {
        Ok(call(&self.client, &disp::Identify { order }, CALL_TIMEOUT)?.shown)
    }

    fn detect(&mut self) -> PortResult<disp::DetectReply> {
        call(&self.client, &disp::Detect {}, SLOW_TIMEOUT)
    }
}

/// The device daemon over the bus.
pub struct BusDevices(pub Client);

impl std::fmt::Debug for BusDevices {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusDevices")
    }
}

fn outcome(reply: dev::OperationReply) -> PortResult<dev::DeviceInfo> {
    match reply.outcome {
        dev::Outcome::Done(info) => Ok(*info),
        dev::Outcome::Refused(refusal) => Err(PortError::Refused(refusal.reason)),
    }
}

impl DevicesPort for BusDevices {
    fn list(&mut self) -> PortResult<dev::ListReply> {
        call(&self.0, &dev::List {}, CALL_TIMEOUT)
    }

    fn switch(
        &mut self,
        address: &str,
        driver: &str,
        version: &str,
    ) -> PortResult<dev::DeviceInfo> {
        let switch = dev::Switch {
            address: address.to_owned(),
            driver: driver.to_owned(),
            version: version.to_owned(),
        };
        outcome(call(&self.0, &switch, SLOW_TIMEOUT)?)
    }

    fn rollback(&mut self, address: &str) -> PortResult<dev::DeviceInfo> {
        let rollback = dev::Rollback {
            address: address.to_owned(),
        };
        outcome(call(&self.0, &rollback, SLOW_TIMEOUT)?)
    }

    fn rescan(&mut self) -> PortResult<Vec<dev::DeviceInfo>> {
        let reply = call(&self.0, &dev::Rescan { address: None }, SLOW_TIMEOUT)?;
        reply.devices.map_err(|r| PortError::Refused(r.reason))
    }
}

/// The updater over the bus.
pub struct BusUpdate {
    client: Client,
    progress: Option<TypedSubscription<Progress>>,
    last: Option<Progress>,
}

impl std::fmt::Debug for BusUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusUpdate")
    }
}

impl BusUpdate {
    /// Subscribes to `update.progress`.
    pub fn new(client: Client) -> BusUpdate {
        let progress = client.subscribe::<Progress>().ok();
        BusUpdate {
            client,
            progress,
            last: None,
        }
    }
}

impl UpdatePort for BusUpdate {
    fn check(&mut self) -> PortResult<()> {
        call(&self.client, &Check {}, CALL_TIMEOUT).map(|_| ())
    }

    fn offline(&mut self, path: &str) -> PortResult<()> {
        let offline = Offline {
            path: path.to_owned(),
            allow_downgrade: false,
        };
        call(&self.client, &offline, CALL_TIMEOUT).map(|_| ())
    }

    fn progress(&mut self) -> Option<(String, Option<String>)> {
        if let Some(sub) = &self.progress {
            while let Ok(Some(next)) = sub.try_recv() {
                self.last = Some(next);
            }
        }
        self.last
            .as_ref()
            .map(|p| (p.phase.key().to_owned(), p.error_key.clone()))
    }
}

/// The account service over the bus.
pub struct BusAccount(pub Client);

impl std::fmt::Debug for BusAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusAccount")
    }
}

fn account_reply(reply: acc::Reply) -> PortResult<acc::AccountState> {
    reply.outcome.map_err(|r| PortError::Refused(r.reason))
}

impl AccountPort for BusAccount {
    fn state(&mut self) -> PortResult<acc::AccountState> {
        account_reply(call(&self.0, &acc::Status {}, CALL_TIMEOUT)?)
    }

    fn request(&mut self, request: AccountRequest) -> PortResult<acc::AccountState> {
        let c = &self.0;
        let t = SLOW_TIMEOUT;
        let reply = match request {
            AccountRequest::SignIn { user, password } => {
                call(c, &acc::SignIn { user, password }, t)
            }
            AccountRequest::Create {
                user,
                password,
                email,
            } => call(
                c,
                &acc::Create {
                    user,
                    password,
                    email,
                },
                t,
            ),
            AccountRequest::SignOut => call(c, &acc::SignOut {}, t),
            AccountRequest::SetServer(server) => call(c, &acc::SetServer { server }, t),
            AccountRequest::SetService(service, on) => call(c, &acc::SetService { service, on }, t),
            AccountRequest::ChangePassword { old, new } => {
                call(c, &acc::ChangePassword { old, new }, t)
            }
            AccountRequest::Delete(password) => call(c, &acc::Delete { password }, t),
            AccountRequest::DeviceSignOut(device) => call(c, &acc::DeviceSignOut { device }, t),
            AccountRequest::DeviceRemove(device) => call(c, &acc::DeviceRemove { device }, t),
        }?;
        account_reply(reply)
    }
}

/// The user's documents folder (saving is atomic: temp file, fsync, rename).
#[derive(Debug, Clone)]
pub struct DocumentsFiles {
    /// The folder.
    pub dir: PathBuf,
}

impl FilesPort for DocumentsFiles {
    fn save(&mut self, name: &str, bytes: &[u8]) -> PortResult<String> {
        let safe = !name.is_empty()
            && name.len() <= 128
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
            && !name.starts_with('.');
        if !safe {
            return Err(PortError::Failed);
        }
        let target = self.dir.join(name);
        let temp = self.dir.join(format!(".{name}.tmp"));
        let write = || -> std::io::Result<()> {
            std::fs::create_dir_all(&self.dir)?;
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            std::fs::rename(&temp, &target)?;
            std::fs::File::open(&self.dir)?.sync_all()
        };
        write().map_err(|_| {
            let _ = std::fs::remove_file(&temp);
            PortError::Failed
        })?;
        Ok(target.display().to_string())
    }
}

/// Text waiting to be offered on the clipboard by the window loop.
#[derive(Debug, Clone, Default)]
pub struct ClipboardQueue(pub Rc<RefCell<Option<String>>>);

impl ClipboardPort for ClipboardQueue {
    fn copy(&mut self, text: &str) -> PortResult<()> {
        *self.0.borrow_mut() = Some(text.to_owned());
        Ok(())
    }
}

/// Network, Bluetooth and sound state over the bus: the services' retained topics.
pub struct BusStatus {
    client: Client,
    net: Retained<NetState>,
    bluetooth: Retained<BluetoothState>,
    audio: Retained<AudioState>,
}

impl std::fmt::Debug for BusStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusStatus")
    }
}

/// The last value of a retained topic (the service sends it on subscribe).
pub(crate) struct Retained<T: ultar_bus::Topic> {
    sub: Option<TypedSubscription<T>>,
    last: Option<T>,
}

impl<T: ultar_bus::Topic + Clone> Retained<T> {
    pub(crate) fn new(client: &Client) -> Self {
        Retained {
            sub: client.subscribe::<T>().ok(),
            last: None,
        }
    }

    pub(crate) fn get(&mut self) -> PortResult<T> {
        let sub = self.sub.as_ref().ok_or(PortError::Unavailable)?;
        if self.last.is_none() {
            if let Ok(first) = sub.recv_timeout(RETAINED_WAIT) {
                self.last = Some(first);
            }
        }
        while let Ok(Some(next)) = sub.try_recv() {
            self.last = Some(next);
        }
        self.last.clone().ok_or(PortError::Unavailable)
    }
}

impl BusStatus {
    /// Subscribes to `net.state`, `bluetooth.state` and `audio.state`.
    pub fn new(client: &Client) -> BusStatus {
        BusStatus {
            client: client.clone(),
            net: Retained::new(client),
            bluetooth: Retained::new(client),
            audio: Retained::new(client),
        }
    }
}

impl StatusPort for BusStatus {
    fn net(&mut self) -> PortResult<NetState> {
        self.net.get()
    }

    fn bluetooth(&mut self) -> PortResult<BluetoothState> {
        self.bluetooth.get()
    }

    fn audio(&mut self) -> PortResult<AudioState> {
        self.audio.get()
    }

    fn bt_devices(&mut self) -> PortResult<Vec<ultar_bus::messages::bluetooth::Device>> {
        use ultar_bus::messages::bluetooth::{BtOutcome, Devices};
        let r = call(&self.client, &Devices {}, CALL_TIMEOUT)?;
        match r.outcome {
            BtOutcome::Done => Ok(r.devices),
            _ => Err(PortError::Unavailable),
        }
    }

    fn bt_call(&mut self, c: BtCall) -> PortResult<()> {
        use ultar_bus::messages::bluetooth::{BtOutcome, Pair, Scan};
        let r = match c {
            BtCall::Scan(on) => call(&self.client, &Scan { on }, CALL_TIMEOUT)?,
            BtCall::Pair(id) => call(&self.client, &Pair { id }, CALL_TIMEOUT)?,
        };
        match r.outcome {
            BtOutcome::Done => Ok(()),
            BtOutcome::NoDevice => Err(PortError::Unavailable),
            _ => Err(PortError::Failed),
        }
    }
}
