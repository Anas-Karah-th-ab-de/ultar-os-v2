//! Contract of the Bluetooth service.
//!
//! Whether the device has Bluetooth, switching its radio (the kernel's radio switch),
//! kept across restarts; with a usable controller (the service's own host stack)
//! also looking for devices ([`Scan`]), pairing ([`Pair`], confirmed by the person in the
//! system dialog through [`PairConfirm`]), connecting bonded devices ([`Connect`]),
//! [`Disconnect`] and [`Forget`]. Keyboards and mice go to the input subsystem. Without
//! a usable controller [`BluetoothState::devices_supported`] is `false` and the device
//! calls answer [`BtOutcome::NoDevice`]; audio devices are listed but not connected
//! in this release ([`Device::audio_unsupported`]).
//!
//! Version 2 (`BluetoothState`, `Device`, `DevicesReply`, `BtReply`): new fields and
//! outcomes; new calls `scan`, `pair`, `forget`; the pairing question
//! `sysdialog.bt_pair` ([`PairConfirm`]) is defined here, in one place.

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "bluetooth";

/// `bluetooth.state` – the Bluetooth of the device (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct BluetoothState {
    /// The device has Bluetooth.
    pub present: bool,
    /// The radio is on.
    pub on: bool,
    /// A hardware switch blocks the radio.
    pub switch_off: bool,
    /// Connected devices.
    pub connected: u8,
    /// Devices can be listed, paired and connected.
    pub devices_supported: bool,
    /// Looking for devices (ends by itself after 20 s).
    pub scanning: bool,
    /// The last pairing, while it runs and until the next one.
    pub pairing: Option<Pairing>,
}

/// A pairing and how far it got.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Pairing {
    /// The device id.
    pub id: String,
    /// How far.
    pub phase: PairPhase,
}

/// How far a pairing got.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairPhase {
    /// Connecting, exchanging keys or waiting for the person.
    Running,
    /// Paired (and connected).
    Paired,
    /// Not paired: refused by either side, timed out or the device left.
    Failed,
}

/// What kind of device.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceKind {
    /// Keyboard.
    Keyboard,
    /// Mouse or other pointing device.
    Mouse,
    /// Keyboard and mouse.
    Combo,
    /// Headphones, speakers.
    Audio,
    /// Phone.
    Phone,
    /// Computer.
    Computer,
    /// Anything else.
    Other,
}

/// One device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Device {
    /// Address as text.
    pub id: String,
    /// Name (data).
    pub name: String,
    /// Connected.
    pub connected: bool,
    /// Battery, if the device tells.
    pub battery_percent: Option<u8>,
    /// Paired (bonded).
    pub paired: bool,
    /// Kind (from its class or appearance).
    pub kind: DeviceKind,
    /// Signal strength when last seen (dBm).
    pub rssi: Option<i8>,
    /// Bluetooth Low Energy (else classic).
    pub le: bool,
    /// Its keys / buttons reach the input subsystem now.
    pub input: bool,
    /// An audio device: listed, but not connected in this release.
    pub audio_unsupported: bool,
}

/// `bluetooth.set` – radio on or off.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Set {
    /// On.
    pub on: bool,
}

/// `bluetooth.devices` – known and nearby devices.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Devices {}

/// Answer to [`Devices`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct DevicesReply {
    /// The result.
    pub outcome: BtOutcome,
    /// The devices.
    pub devices: Vec<Device>,
}

/// `bluetooth.scan` – look for devices (on) or stop.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scan {
    /// Look.
    pub on: bool,
}

/// `bluetooth.pair` – pair with a device found by a scan (the answer comes before the
/// pairing ends; follow [`BluetoothState::pairing`]).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Pair {
    /// Its id.
    pub id: String,
}

/// `bluetooth.forget` – remove a pairing (disconnects).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Forget {
    /// Its id.
    pub id: String,
}

/// `bluetooth.connect` – connect a paired device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Connect {
    /// Its id.
    pub id: String,
}

/// `bluetooth.disconnect` – disconnect a device.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Disconnect {
    /// Its id.
    pub id: String,
}

/// How a call went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum BtOutcome {
    /// Done.
    Done,
    /// The device has no Bluetooth.
    NoDevice,
    /// A hardware switch blocks the radio.
    SwitchOff,
    /// Not in this release (audio devices).
    Unsupported,
    /// The kernel refused.
    Failed,
    /// No such device (not found by a scan, not paired, malformed id).
    UnknownDevice,
    /// Another connection or pairing is running.
    Busy,
}

/// What the pairing question shows.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PairQuestion {
    /// "Pair with this device?" (the device has no display: no number to compare).
    Consent,
    /// "Does the device show this number?" (six digits, shown with leading zeros).
    Compare(u32),
    /// "Type this number on the device, then Enter" (keyboards); Cancel stops it.
    Show(u32),
}

/// `sysdialog.bt_pair` – the Bluetooth service asks the person (system dialog, trusted
/// frame) whether to pair. Called by `system.btd` only; unanswered within
/// `sysdialog::ANSWER_WITHIN_MS` the dialog answers `accept: false`.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct PairConfirm {
    /// The device's name (data from the air: show as text, never interpret).
    pub device_name: String,
    /// Its address (`AA:BB:CC:DD:EE:FF`, `/r` for LE random).
    pub address: String,
    /// Its kind.
    pub kind: DeviceKind,
    /// What to show.
    pub question: PairQuestion,
}

/// Answer to [`PairConfirm`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairConfirmReply {
    /// The person agreed (Compare: the numbers match; Show: typed it).
    pub accept: bool,
}

/// Answer to the changes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct BtReply {
    /// The result.
    pub outcome: BtOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Set,
    Devices,
    Connect,
    Disconnect,
    Scan,
    Pair,
    Forget,
    PairConfirm,
    PairConfirmReply
);

macro_rules! version_two {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 2; } )* };
}
version_two!(BluetoothState, Device, DevicesReply, BtReply);

impl Topic for BluetoothState {
    const TOPIC: &'static str = "bluetooth.state";
    const RETAINED: bool = true;
}

macro_rules! call {
    ($t:ty, $m:literal, $r:ty) => {
        impl Call for $t {
            const SERVICE: &'static str = SERVICE;
            const METHOD: &'static str = $m;
            type Reply = $r;
        }
    };
}
call!(Set, "set", BtReply);
call!(Devices, "devices", DevicesReply);
call!(Connect, "connect", BtReply);
call!(Disconnect, "disconnect", BtReply);
call!(Scan, "scan", BtReply);
call!(Pair, "pair", BtReply);
call!(Forget, "forget", BtReply);

impl Call for PairConfirm {
    const SERVICE: &'static str = super::sysdialog::SERVICE;
    const METHOD: &'static str = "bt_pair";
    type Reply = PairConfirmReply;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip() {
        let s = BluetoothState {
            present: true,
            on: true,
            ..BluetoothState::default()
        };
        assert_eq!(decode::<BluetoothState>(&encode(&s).unwrap()).unwrap(), s);
        let r = DevicesReply {
            outcome: BtOutcome::Unsupported,
            devices: vec![],
        };
        assert_eq!(decode::<DevicesReply>(&encode(&r).unwrap()).unwrap(), r);
        let q = PairConfirm {
            device_name: "K380".into(),
            address: "11:22:33:44:55:66".into(),
            kind: DeviceKind::Keyboard,
            question: PairQuestion::Compare(42),
        };
        assert_eq!(decode::<PairConfirm>(&encode(&q).unwrap()).unwrap(), q);
        assert_eq!(<PairConfirm as Call>::SERVICE, "sysdialog");
        assert_eq!(<Pair as Call>::METHOD, "pair");
    }
}
