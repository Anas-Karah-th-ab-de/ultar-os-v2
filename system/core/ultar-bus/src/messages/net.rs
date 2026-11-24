//! Contract of the network service: Wi-Fi, wired link, airplane mode.
//!
//! What is real: the wired link and its speed, switching the Wi-Fi radio and airplane
//! mode (the kernel's radio switches; a hardware switch is reported, not overridden),
//! all kept across restarts; listing and joining Wi-Fi networks (open and
//! WPA2-Personal) with the service's own supplicant, DHCP and DNS, saved networks and
//! automatic reconnection. Without a Wi-Fi device (the Ultar kernel has no Wi-Fi
//! drivers yet) [`NetState::networks_supported`] is `false` and the calls answer
//! [`NetOutcome::NoDevice`].
//!
//! Version 1 calls kept unchanged: [`Scan`] (networks by name) and [`Connect`] (join
//! and remember). Added in this release (each its own message, version 1): [`Networks`]
//! (every access point with its security), [`Join`] (with a detailed [`JoinOutcome`]),
//! [`Disconnect`], [`Forget`], [`Known`] and the retained topic [`WifiLink`]
//! (`net.wifi`: connection state, address, DNS).

use super::auth::Secret;
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// Name of the service on the bus.
pub const SERVICE: &str = "net";

/// The wired link.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Wired {
    /// Speed in Mbit/s, if the driver says.
    pub speed_mbit: Option<u32>,
}

/// `net.state` – the network of the device (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct NetState {
    /// A Wi-Fi interface exists.
    pub wifi_present: bool,
    /// The Wi-Fi radio is on.
    pub wifi_on: bool,
    /// A hardware switch blocks the Wi-Fi radio.
    pub wifi_switch_off: bool,
    /// The Wi-Fi network in use (`None`: none or unknown).
    pub ssid: Option<String>,
    /// Its signal, 0–100.
    pub signal_percent: Option<u8>,
    /// The wired link, when one is up.
    pub wired: Option<Wired>,
    /// Airplane mode: every radio is off.
    pub airplane: bool,
    /// The device has radios airplane mode can switch.
    pub airplane_possible: bool,
    /// The connection is metered (updates wait).
    pub metered: bool,
    /// Wi-Fi networks can be listed and joined.
    pub networks_supported: bool,
}

/// One Wi-Fi network.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Network {
    /// Name (data).
    pub ssid: String,
    /// Signal, 0–100.
    pub signal_percent: u8,
    /// Needs a password.
    pub secured: bool,
    /// Joined before.
    pub known: bool,
}

/// `net.set_wifi` – Wi-Fi radio on or off.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetWifi {
    /// On.
    pub on: bool,
}

/// `net.set_airplane` – airplane mode on or off.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct SetAirplane {
    /// On.
    pub on: bool,
}

/// `net.scan` – the Wi-Fi networks around.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Scan {}

/// Answer to [`Scan`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct ScanReply {
    /// The result.
    pub outcome: NetOutcome,
    /// The networks (strongest first).
    pub networks: Vec<Network>,
}

/// `net.connect` – join a Wi-Fi network.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Connect {
    /// Name.
    pub ssid: String,
    /// Password of a secured network (never logged).
    pub secret: Option<Secret>,
}

/// How a call went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetOutcome {
    /// Done.
    Done,
    /// No such radio or interface.
    NoDevice,
    /// A hardware switch blocks the radio.
    SwitchOff,
    /// Not in this release (Wi-Fi networks need the supplicant).
    Unsupported,
    /// The kernel refused.
    Failed,
}

/// Answer to the changes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct NetReply {
    /// The result.
    pub outcome: NetOutcome,
}

macro_rules! version_one {
    ($($t:ty),*) => { $( impl Message for $t { const VERSION: u16 = 1; } )* };
}
version_one!(
    Wired,
    NetState,
    Network,
    SetWifi,
    SetAirplane,
    Scan,
    ScanReply,
    Connect,
    NetReply
);

impl Topic for NetState {
    const TOPIC: &'static str = "net.state";
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
call!(SetWifi, "set_wifi", NetReply);
call!(SetAirplane, "set_airplane", NetReply);
call!(Scan, "scan", ScanReply);
call!(Connect, "connect", NetReply);

#[path = "net_wifi.rs"]
mod wifi;
pub use wifi::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::{decode, encode};

    #[test]
    fn messages_round_trip_and_the_secret_stays_hidden() {
        let s = NetState {
            wired: Some(Wired {
                speed_mbit: Some(1000),
            }),
            airplane_possible: true,
            ..NetState::default()
        };
        assert_eq!(decode::<NetState>(&encode(&s).unwrap()).unwrap(), s);
        let c = Connect {
            ssid: "home".into(),
            secret: Some(Secret::new("pass-word".into())),
        };
        assert_eq!(decode::<Connect>(&encode(&c).unwrap()).unwrap(), c);
        assert!(!format!("{c:?}").contains("pass-word"));
    }

    #[test]
    fn wifi_messages_round_trip() {
        let j = Join {
            ssid: "home".into(),
            secret: Some(Secret::new("pass-word".into())),
            remember: true,
        };
        assert_eq!(decode::<Join>(&encode(&j).unwrap()).unwrap(), j);
        assert!(!format!("{j:?}").contains("pass-word"));
        let l = WifiLink {
            phase: WifiPhase::Connected,
            ssid: Some("home".into()),
            signal_percent: Some(70),
            address: Some("10.0.0.2/24".into()),
            router: Some("10.0.0.1".into()),
            dns: vec!["10.0.0.1".into()],
        };
        assert_eq!(decode::<WifiLink>(&encode(&l).unwrap()).unwrap(), l);
        let n = NetworksReply {
            outcome: NetOutcome::Done,
            networks: vec![NetworkInfo {
                ssid: "x".into(),
                bssid: "02:00:00:00:00:01".into(),
                signal_percent: 50,
                frequency_mhz: 2412,
                security: WifiSecurity::Wpa2Personal,
                known: false,
                joinable: true,
                connected: false,
            }],
        };
        assert_eq!(decode::<NetworksReply>(&encode(&n).unwrap()).unwrap(), n);
        assert_eq!(<Join as Call>::METHOD, "join");
        assert_eq!(<WifiLink as Topic>::TOPIC, "net.wifi");
        let k = KnownReply {
            ssids: vec!["a".into()],
        };
        assert_eq!(decode::<KnownReply>(&encode(&k).unwrap()).unwrap(), k);
        assert_eq!(<Forget as Call>::METHOD, "forget");
        assert_eq!(<Disconnect as Call>::METHOD, "disconnect");
        assert_eq!(<Networks as Call>::METHOD, "networks");
        assert_eq!(<Known as Call>::METHOD, "known");
        let r = JoinReply {
            outcome: JoinOutcome::WrongPassword,
        };
        assert_eq!(decode::<JoinReply>(&encode(&r).unwrap()).unwrap(), r);
    }
}
