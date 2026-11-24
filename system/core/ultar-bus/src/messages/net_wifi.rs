//! Wi-Fi calls and topic of the network service, added to `net` (re-exported
//! there; every message here is new, version 1).

use super::{NetOutcome, NetReply, Secret, SERVICE};
use crate::message::{Call, Message, Topic};
use serde::{Deserialize, Serialize};

/// What security a Wi-Fi network asks for.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum WifiSecurity {
    /// No password.
    Open,
    /// WPA2-Personal.
    Wpa2Personal,
    /// WPA2/WPA3-Personal transition mode (joined with WPA2).
    Wpa2Wpa3Personal,
    /// WPA3-Personal only: not joinable in this release (no reviewed SAE).
    Wpa3Personal,
    /// Enterprise (802.1X): not joinable in this release.
    Enterprise,
    /// WEP or WPA version 1: insecure, never joined.
    Legacy,
    /// Another combination this release cannot use.
    Other,
}

/// One access point of a scan.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NetworkInfo {
    /// Network name (data, may be empty for hidden networks).
    pub ssid: String,
    /// The access point's address, `aa:bb:cc:dd:ee:ff`.
    pub bssid: String,
    /// Signal, 0–100.
    pub signal_percent: u8,
    /// Frequency in MHz.
    pub frequency_mhz: u32,
    /// Security.
    pub security: WifiSecurity,
    /// Saved before.
    pub known: bool,
    /// This release can join it.
    pub joinable: bool,
    /// Connected to it now.
    pub connected: bool,
}

/// `net.networks` – every access point around (strongest first).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Networks {}

/// Answer to [`Networks`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct NetworksReply {
    /// `Done`, `NoDevice`, `SwitchOff` or `Failed`.
    pub outcome: NetOutcome,
    /// The access points.
    pub networks: Vec<NetworkInfo>,
}

/// `net.join` – join a Wi-Fi network.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Join {
    /// Name.
    pub ssid: String,
    /// Password (8–63 characters or 64 hex digits; never logged). `None`: open, or
    /// a saved network.
    pub secret: Option<Secret>,
    /// Save it (key stored 0600, reconnected automatically).
    pub remember: bool,
}

/// How joining went.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum JoinOutcome {
    /// Connected (the address follows on `net.wifi`).
    Connected,
    /// No Wi-Fi device.
    NoDevice,
    /// The radio is off or blocked by a hardware switch.
    RadioOff,
    /// No network of that name is in range.
    NotFound,
    /// A password is needed (none given, none saved).
    PasswordNeeded,
    /// The password is malformed (length, characters).
    BadPassword,
    /// The handshake failed: most likely a wrong password.
    WrongPassword,
    /// The security of the network is not supported (WPA3-only, enterprise, WEP).
    Unsupported,
    /// The access point refused the association.
    Refused,
    /// No answer in time.
    Timeout,
    /// Another join is running.
    Busy,
    /// The kernel refused.
    Failed,
}

/// Answer to [`Join`].
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct JoinReply {
    /// The result.
    pub outcome: JoinOutcome,
}

/// `net.disconnect` – leave the Wi-Fi network (no automatic reconnection until the
/// next join).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Disconnect {}

/// `net.forget` – remove a saved network (leaves it when connected).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Forget {
    /// Name.
    pub ssid: String,
}

/// `net.known` – the saved networks.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Known {}

/// Answer to [`Known`].
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct KnownReply {
    /// Names, most recently used first.
    pub ssids: Vec<String>,
}

/// State of the Wi-Fi connection.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum WifiPhase {
    /// No Wi-Fi device (or no Wi-Fi support in the kernel).
    #[default]
    NoDevice,
    /// The radio is off.
    Off,
    /// Not connected.
    Disconnected,
    /// Looking for networks.
    Scanning,
    /// Associating with the access point.
    Associating,
    /// WPA2 handshake.
    Authenticating,
    /// Connected, asking DHCP for an address.
    ObtainingAddress,
    /// Connected with an address.
    Connected,
}

/// `net.wifi` – the Wi-Fi connection (retained).
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Default)]
pub struct WifiLink {
    /// State.
    pub phase: WifiPhase,
    /// Network name.
    pub ssid: Option<String>,
    /// Signal, 0–100.
    pub signal_percent: Option<u8>,
    /// IPv4 address with prefix, `a.b.c.d/n`.
    pub address: Option<String>,
    /// Default router.
    pub router: Option<String>,
    /// DNS servers.
    pub dns: Vec<String>,
}

version_one!(
    WifiSecurity,
    NetworkInfo,
    Networks,
    NetworksReply,
    Join,
    JoinOutcome,
    JoinReply,
    Disconnect,
    Forget,
    Known,
    KnownReply,
    WifiPhase,
    WifiLink
);

impl Topic for WifiLink {
    const TOPIC: &'static str = "net.wifi";
    const RETAINED: bool = true;
}

call!(Networks, "networks", NetworksReply);
call!(Join, "join", JoinReply);
call!(Disconnect, "disconnect", NetReply);
call!(Forget, "forget", NetReply);
call!(Known, "known", KnownReply);
