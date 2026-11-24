//! The Bluetooth service: radio state and switch, the bus side.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use ultar_bus::messages::bluetooth::{
    BluetoothState, BtOutcome, BtReply, Connect, Devices, DevicesReply, Disconnect, Set, SERVICE,
};
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_hal::HalError;
use ultar_hal_linux::rfkill::{RadioKind, Rfkill};
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// How often the switch is read again.
pub const TICK: Duration = Duration::from_secs(3);

/// What is kept across restarts.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Saved {
    /// Radio wanted on.
    pub on: bool,
}

/// The Bluetooth service.
pub struct BtService {
    rfkill: Rfkill,
    state_file: PathBuf,
    saved: Option<Saved>,
    last: Option<BluetoothState>,
}

impl BtService {
    /// A service reading the kernel below `root`, keeping its wish below `state_dir`.
    pub fn new(root: PathBuf, state_dir: PathBuf) -> Self {
        let state_file = state_dir.join("bluetooth");
        Self {
            rfkill: Rfkill::new(&root),
            saved: store::load(&state_file).ok(),
            state_file,
            last: None,
        }
    }

    /// The state as the kernel shows it now.
    pub fn read(&self) -> BluetoothState {
        let radios: Vec<_> = self
            .rfkill
            .radios()
            .into_iter()
            .filter(|r| r.kind == RadioKind::Bluetooth)
            .collect();
        BluetoothState {
            present: !radios.is_empty(),
            on: !radios.is_empty() && radios.iter().all(|r| !r.soft && !r.hard),
            switch_off: radios.iter().any(|r| r.hard),
            connected: 0,
            devices_supported: false,
        }
    }

    /// Switches the radio.
    pub fn set(&mut self, on: bool) -> BtOutcome {
        let now = self.read();
        if !now.present {
            return BtOutcome::NoDevice;
        }
        if on && now.switch_off {
            return BtOutcome::SwitchOff;
        }
        match self.rfkill.set(RadioKind::Bluetooth, !on) {
            Ok(()) => {
                self.saved = Some(Saved { on });
                BtOutcome::Done
            }
            Err(HalError::NotSupported) => BtOutcome::NoDevice,
            Err(_) => BtOutcome::Failed,
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.read();
        if self.last != Some(now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }
}

impl Service for BtService {
    fn name(&self) -> &'static str {
        SERVICE
    }

    fn tick_every(&self) -> Option<Duration> {
        Some(TICK)
    }

    fn start(&mut self, bus: &Bus<'_, Self>) {
        if let Some(saved) = self.saved {
            let _ = self.set(saved.on);
        }
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let outcome = match request.method() {
            m if m == Set::METHOD => match request.decode::<Set>() {
                Ok(c) => {
                    let outcome = self.set(c.on);
                    if let Some(saved) = self.saved {
                        if store::save(&self.state_file, &saved).is_err() {
                            bus.log(Record::new(Level::Warn, "btd.not_saved"));
                        }
                    }
                    Ok(outcome)
                }
                Err(e) => Err(e),
            },
            m if m == Connect::METHOD => {
                request.decode::<Connect>().map(|_| BtOutcome::Unsupported)
            }
            m if m == Disconnect::METHOD => request
                .decode::<Disconnect>()
                .map(|_| BtOutcome::Unsupported),
            m if m == Devices::METHOD => {
                let _ = request.respond::<Devices>(|_| {
                    Ok(DevicesReply {
                        outcome: BtOutcome::Unsupported,
                        devices: Vec::new(),
                    })
                });
                return;
            }
            _ => {
                let _ = request.fail(ErrorCode::UnknownMethod);
                return;
            }
        };
        match outcome {
            Ok(outcome) => {
                let _ = request.reply(&BtReply { outcome });
                self.publish(bus);
            }
            Err(_) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.publish(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(root: &std::path::Path, path: &str, value: &str) {
        let p = root.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, value).unwrap();
    }

    #[test]
    fn radio_switch_and_absence() {
        let root = std::env::temp_dir().join(format!("btd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut none = BtService::new(root.clone(), root.clone());
        assert_eq!(none.read(), BluetoothState::default());
        assert_eq!(none.set(true), BtOutcome::NoDevice);
        put(&root, "sys/class/rfkill/rfkill3/type", "bluetooth");
        put(&root, "sys/class/rfkill/rfkill3/soft", "0");
        put(&root, "sys/class/rfkill/rfkill3/hard", "0");
        let mut bt = BtService::new(root.clone(), root.clone());
        assert!(bt.read().present && bt.read().on);
        assert_eq!(bt.set(false), BtOutcome::NoDevice, "no control device");
        put(&root, "dev/rfkill", "");
        assert_eq!(bt.set(false), BtOutcome::Done);
        assert_eq!(
            std::fs::read(root.join("dev/rfkill")).unwrap(),
            [0, 0, 0, 0, 2, 3, 1, 0]
        );
        put(&root, "sys/class/rfkill/rfkill3/hard", "1");
        assert_eq!(bt.set(true), BtOutcome::SwitchOff);
        let _ = std::fs::remove_dir_all(&root);
    }
}
