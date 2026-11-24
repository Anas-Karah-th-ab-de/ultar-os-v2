//! The Bluetooth service: radio state and switch, and the host stack behind the bus.

use crate::runner::{self, Question, Stack, Uinput};
use crate::stack::bonds::Kind;
use crate::stack::hci::Addr;
use crate::stack::host::{Cmd, Config, Refusal};
use crate::stack::smp::Ask;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ultar_bus::messages::bluetooth::{
    BluetoothState, BtOutcome, BtReply, Connect, Device, DeviceKind, Devices, DevicesReply,
    Disconnect, Forget, Pair, PairConfirm, PairPhase, PairQuestion, Pairing, Scan, Set, SERVICE,
};
use ultar_bus::messages::sysdialog::CALL_TIMEOUT_MS;
use ultar_bus::{Call, ErrorCode, IncomingRequest};
use ultar_hal::HalError;
use ultar_hal_linux::rfkill::{RadioKind, Rfkill};
use ultar_service::log::{Level, Record};
use ultar_svckit::{store, Bus, Service};

/// How often the switch and the stack are looked at.
pub const TICK: Duration = Duration::from_millis(500);
/// A controller that could not be opened is tried again after this.
pub const RETRY: Duration = Duration::from_secs(10);

/// What is kept across restarts.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Saved {
    /// Radio wanted on.
    pub on: bool,
}

/// The Bluetooth service.
pub struct BtService {
    rfkill: Rfkill,
    root: PathBuf,
    state_file: PathBuf,
    bonds_file: PathBuf,
    saved: Option<Saved>,
    last: Option<BluetoothState>,
    stack: Option<Stack>,
    next_try: Option<Instant>,
    pairing: Option<Pairing>,
}

fn kind(k: Kind) -> DeviceKind {
    match k {
        Kind::Keyboard => DeviceKind::Keyboard,
        Kind::Mouse => DeviceKind::Mouse,
        Kind::Combo => DeviceKind::Combo,
        Kind::Audio => DeviceKind::Audio,
        Kind::Phone => DeviceKind::Phone,
        Kind::Computer => DeviceKind::Computer,
        Kind::Other => DeviceKind::Other,
    }
}

fn refusal(r: Refusal) -> BtOutcome {
    match r {
        Refusal::NotReady => BtOutcome::NoDevice,
        Refusal::Unknown => BtOutcome::UnknownDevice,
        Refusal::Busy => BtOutcome::Busy,
    }
}

impl BtService {
    /// A service reading the kernel below `root`, keeping its wish and bonds below `state_dir`.
    pub fn new(root: PathBuf, state_dir: PathBuf) -> Self {
        let state_file = state_dir.join("bluetooth");
        Self {
            rfkill: Rfkill::new(&root),
            root,
            saved: store::load(&state_file).ok(),
            state_file,
            bonds_file: state_dir.join("bonds"),
            last: None,
            stack: None,
            next_try: None,
            pairing: None,
        }
    }

    fn ready(&self) -> bool {
        self.stack.as_ref().is_some_and(|s| s.snapshot().ready)
    }

    /// The state as the kernel and the stack show it now.
    pub fn read(&self) -> BluetoothState {
        let radios: Vec<_> = self
            .rfkill
            .radios()
            .into_iter()
            .filter(|r| r.kind == RadioKind::Bluetooth)
            .collect();
        let snap = self
            .stack
            .as_ref()
            .map(|s| s.snapshot())
            .unwrap_or_default();
        let present = !radios.is_empty() || snap.ready;
        BluetoothState {
            present,
            on: present && radios.iter().all(|r| !r.soft && !r.hard),
            switch_off: radios.iter().any(|r| r.hard),
            connected: snap.devices.iter().filter(|d| d.connected).count().min(255) as u8,
            devices_supported: snap.ready,
            scanning: snap.scanning,
            pairing: self.pairing.clone(),
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
                self.next_try = None;
                BtOutcome::Done
            }
            Err(HalError::NotSupported) => BtOutcome::NoDevice,
            Err(_) => BtOutcome::Failed,
        }
    }

    fn publish(&mut self, bus: &Bus<'_, Self>) {
        let now = self.read();
        if self.last.as_ref() != Some(&now) {
            bus.retain(&now);
            self.last = Some(now);
        }
    }

    /// Opens the controller when the radio may be on and none is open.
    fn keep_stack(&mut self, bus: &Bus<'_, Self>) {
        if self.stack.as_ref().is_some_and(|s| s.snapshot().ended) {
            self.stack = None;
            bus.log(Record::new(Level::Warn, "btd.controller_gone"));
        }
        let wanted = self.saved.is_none_or(|s| s.on);
        if self.stack.is_some() || !wanted || self.next_try.is_some_and(|t| Instant::now() < t) {
            return;
        }
        self.next_try = Some(Instant::now() + RETRY);
        let Some(t) = runner::open(&self.root) else {
            return;
        };
        let sink = Box::new(Uinput::new(self.root.clone()));
        self.stack = Stack::spawn(t, self.bonds_file.clone(), sink, Config::default());
        if self.stack.is_some() {
            bus.log(Record::new(Level::Info, "btd.controller_open"));
        }
    }

    fn news(&mut self, bus: &Bus<'_, Self>) {
        let Some(stack) = self.stack.as_ref() else {
            return;
        };
        let (questions, results) = stack.take_news();
        for (addr, reason) in results {
            let id = addr.text();
            if self
                .pairing
                .as_ref()
                .is_some_and(|p| p.id == id || p.phase == PairPhase::Running)
            {
                let phase = if reason == 0 {
                    PairPhase::Paired
                } else {
                    PairPhase::Failed
                };
                self.pairing = Some(Pairing { id, phase });
                bus.log(Record::new(
                    Level::Info,
                    if reason == 0 {
                        "btd.paired"
                    } else {
                        "btd.pair_failed"
                    },
                ));
            }
        }
        for q in questions {
            self.ask(q, bus);
        }
    }

    /// Hands a question to the system dialog on a helper; no answer = no.
    fn ask(&mut self, q: Question, bus: &Bus<'_, Self>) {
        let question = match q.ask {
            Ask::Consent => PairQuestion::Consent,
            Ask::Compare(n) => PairQuestion::Compare(n),
            Ask::Show(n) => PairQuestion::Show(n),
            Ask::Type => {
                self.answer(q.token, false);
                return;
            }
        };
        let call = PairConfirm {
            device_name: q.name,
            address: q.addr.text(),
            kind: kind(q.kind),
            question,
        };
        let token = q.token;
        let started = bus.later(move |client| {
            let accept = client
                .call(&call, Duration::from_millis(CALL_TIMEOUT_MS))
                .map(|r| r.accept)
                .unwrap_or(false);
            Box::new(move |svc: &mut BtService, _bus: &Bus<'_, BtService>| {
                svc.answer(token, accept)
            })
        });
        if !started {
            self.answer(token, false);
        }
    }

    fn answer(&mut self, token: u32, accept: bool) {
        if let Some(s) = self.stack.as_ref() {
            let _ = s.command(Cmd::Answer { token, accept });
        }
    }

    fn device_call(&mut self, id: &str, make: impl FnOnce(Addr) -> Cmd) -> BtOutcome {
        let Some(stack) = self.stack.as_ref() else {
            return BtOutcome::NoDevice;
        };
        let Some(addr) = Addr::parse(id) else {
            return BtOutcome::UnknownDevice;
        };
        let cmd = make(addr);
        let audio = stack
            .snapshot()
            .devices
            .iter()
            .any(|d| d.addr == addr && d.kind == Kind::Audio);
        if audio && matches!(cmd, Cmd::Pair(_) | Cmd::Connect(_)) {
            return BtOutcome::Unsupported;
        }
        match stack.command(cmd) {
            Ok(()) => {
                if matches!(cmd, Cmd::Pair(_)) {
                    self.pairing = Some(Pairing {
                        id: addr.text(),
                        phase: PairPhase::Running,
                    });
                }
                BtOutcome::Done
            }
            Err(r) => refusal(r),
        }
    }

    fn devices(&self) -> DevicesReply {
        let Some(stack) = self.stack.as_ref().filter(|_| self.ready()) else {
            return DevicesReply {
                outcome: BtOutcome::NoDevice,
                devices: Vec::new(),
            };
        };
        let devices = stack
            .snapshot()
            .devices
            .into_iter()
            .map(|d| Device {
                id: d.addr.text(),
                name: d.name,
                connected: d.connected,
                battery_percent: None,
                paired: d.paired,
                kind: kind(d.kind),
                rssi: d.rssi,
                le: d.le,
                input: d.input,
                audio_unsupported: d.kind == Kind::Audio,
            })
            .collect();
        DevicesReply {
            outcome: BtOutcome::Done,
            devices,
        }
    }

    fn change(&mut self, request: &IncomingRequest, bus: &Bus<'_, Self>) -> Result<BtOutcome, ()> {
        let m = request.method();
        Ok(if m == Set::METHOD {
            let c = request.decode::<Set>().map_err(|_| ())?;
            let outcome = self.set(c.on);
            if let Some(saved) = self.saved {
                if store::save(&self.state_file, &saved).is_err() {
                    bus.log(Record::new(Level::Warn, "btd.not_saved"));
                }
            }
            outcome
        } else if m == Scan::METHOD {
            let c = request.decode::<Scan>().map_err(|_| ())?;
            match self.stack.as_ref() {
                Some(s) => s
                    .command(Cmd::Scan(c.on))
                    .map(|_| BtOutcome::Done)
                    .unwrap_or_else(refusal),
                None => BtOutcome::NoDevice,
            }
        } else if m == Pair::METHOD {
            let c = request.decode::<Pair>().map_err(|_| ())?;
            self.device_call(&c.id, Cmd::Pair)
        } else if m == Connect::METHOD {
            let c = request.decode::<Connect>().map_err(|_| ())?;
            self.device_call(&c.id, Cmd::Connect)
        } else if m == Disconnect::METHOD {
            let c = request.decode::<Disconnect>().map_err(|_| ())?;
            self.device_call(&c.id, Cmd::Disconnect)
        } else {
            let c = request.decode::<Forget>().map_err(|_| ())?;
            self.device_call(&c.id, Cmd::Forget)
        })
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
        self.keep_stack(bus);
        self.publish(bus);
    }

    fn request(&mut self, request: IncomingRequest, bus: &Bus<'_, Self>) {
        let m = request.method();
        if m == Devices::METHOD {
            let reply = self.devices();
            let _ = request.respond::<Devices>(|_| Ok(reply));
            return;
        }
        let known = [
            Set::METHOD,
            Scan::METHOD,
            Pair::METHOD,
            Connect::METHOD,
            Disconnect::METHOD,
            Forget::METHOD,
        ];
        if !known.contains(&m) {
            let _ = request.fail(ErrorCode::UnknownMethod);
            return;
        }
        match self.change(&request, bus) {
            Ok(outcome) => {
                let _ = request.reply(&BtReply { outcome });
                self.publish(bus);
            }
            Err(()) => {
                let _ = request.fail(ErrorCode::BadPayload);
            }
        }
    }

    fn tick(&mut self, bus: &Bus<'_, Self>) {
        self.keep_stack(bus);
        self.news(bus);
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
        assert_eq!(none.devices().outcome, BtOutcome::NoDevice);
        assert_eq!(
            none.device_call("11:22:33:44:55:66", Cmd::Pair),
            BtOutcome::NoDevice
        );
        put(&root, "sys/class/rfkill/rfkill3/type", "bluetooth");
        put(&root, "sys/class/rfkill/rfkill3/soft", "0");
        put(&root, "sys/class/rfkill/rfkill3/hard", "0");
        let mut bt = BtService::new(root.clone(), root.clone());
        assert!(bt.read().present && bt.read().on && !bt.read().devices_supported);
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
