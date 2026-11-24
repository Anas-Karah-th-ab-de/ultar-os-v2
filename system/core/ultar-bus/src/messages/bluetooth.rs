//! Contract of the Bluetooth service.
//!
//! What this release does for real: whether the device has Bluetooth, switching its
//! radio (the kernel's radio switch), kept across restarts. Pairing and connecting
//! devices needs a Bluetooth host stack, which this release does not have:
//! [`BluetoothState::devices_supported`] is `false` and [`Devices`] / [`Connect`] /
//! [`Disconnect`] answer [`BtOutcome::Unsupported`].

use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "bluetooth";

/// `bluetooth.state` – the Bluetooth of the device (retained).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
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

/// `bluetooth.connect` – connect a device.
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
    /// Not in this release (needs the Bluetooth host stack).
    Unsupported,
    /// The kernel refused.
    Failed,
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
    BluetoothState,
    Device,
    Set,
    Devices,
    DevicesReply,
    Connect,
    Disconnect,
    BtReply
);

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
    }
}
