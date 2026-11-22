//! Network, Wi-Fi and Bluetooth classes.

use crate::error::{HalError, HalResult};
use std::fmt;
use std::net::IpAddr;

/// Kind of a network interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum InterfaceKind {
    /// Loopback.
    Loopback,
    /// Wired Ethernet.
    Ethernet,
    /// Wi-Fi.
    Wifi,
    /// Cellular data.
    Cellular,
    /// Bluetooth network.
    Bluetooth,
    /// Tunnel, bridge or other software interface.
    Virtual,
}

/// State of the link of an interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LinkState {
    /// Switched off.
    Down,
    /// Switched on, but no cable or no association.
    NoCarrier,
    /// Switched on and connected.
    Up,
}

/// An address assigned to an interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct InterfaceAddress {
    /// The address.
    pub address: IpAddr,
    /// Length of the network prefix in bits.
    pub prefix_len: u8,
}

/// A network interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetInterface {
    /// Kernel name of the interface (technical, not for people).
    pub name: String,
    /// Kind of interface.
    pub kind: InterfaceKind,
    /// Hardware address, if the interface has one.
    pub mac: Option<[u8; 6]>,
    /// State of the link.
    pub link: LinkState,
    /// Assigned addresses.
    pub addresses: Vec<InterfaceAddress>,
}

/// Network interfaces of a backend.
pub trait Network: Send + Sync {
    /// Every interface.
    fn interfaces(&self) -> HalResult<Vec<NetInterface>>;
    /// Link state of one interface.
    fn link_state(&self, name: &str) -> HalResult<LinkState>;
    /// Addresses of one interface.
    fn addresses(&self, name: &str) -> HalResult<Vec<InterfaceAddress>>;
    /// Switches an interface on or off.
    fn set_link_up(&self, name: &str, up: bool) -> HalResult<()>;
}

/// Protection of a Wi-Fi network.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WifiSecurity {
    /// No protection.
    Open,
    /// WEP (obsolete).
    Wep,
    /// WPA or WPA2 with a passphrase.
    Wpa2,
    /// WPA3 with a passphrase.
    Wpa3,
    /// 802.1X with individual accounts.
    Enterprise,
}

impl WifiSecurity {
    /// True if connecting needs a passphrase.
    pub const fn needs_passphrase(self) -> bool {
        matches!(
            self,
            WifiSecurity::Wep | WifiSecurity::Wpa2 | WifiSecurity::Wpa3
        )
    }
}

/// A Wi-Fi network found by a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccessPoint {
    /// Network name; empty for hidden networks.
    pub ssid: String,
    /// Address of the access point.
    pub bssid: [u8; 6],
    /// Signal strength in dBm.
    pub signal_dbm: i16,
    /// Frequency in MHz.
    pub frequency_mhz: u32,
    /// Protection.
    pub security: WifiSecurity,
}

impl AccessPoint {
    /// Signal quality in percent (-100 dBm = 0, -50 dBm and better = 100).
    pub fn signal_percent(&self) -> u8 {
        let clamped = i32::from(self.signal_dbm).clamp(-100, -50);
        ((clamped + 100) * 2) as u8
    }

    /// Reduces scan results to one entry per network name, keeping the strongest
    /// access point, dropping hidden networks, strongest network first.
    pub fn strongest_per_network(mut found: Vec<AccessPoint>) -> Vec<AccessPoint> {
        found.retain(|access_point| !access_point.ssid.is_empty());
        found.sort_by(|a, b| b.signal_dbm.cmp(&a.signal_dbm).then(a.ssid.cmp(&b.ssid)));
        let mut seen: Vec<String> = Vec::new();
        found.retain(|access_point| {
            let is_new = !seen.contains(&access_point.ssid);
            if is_new {
                seen.push(access_point.ssid.clone());
            }
            is_new
        });
        found
    }
}

/// What is needed to join a Wi-Fi network.
#[derive(Clone, PartialEq, Eq)]
pub struct WifiCredentials {
    /// Network name.
    pub ssid: String,
    /// Passphrase; `None` for open networks.
    pub passphrase: Option<String>,
}

impl WifiCredentials {
    /// Credentials for an open network.
    pub fn open(ssid: &str) -> Self {
        WifiCredentials {
            ssid: ssid.to_string(),
            passphrase: None,
        }
    }

    /// Credentials with a passphrase.
    pub fn with_passphrase(ssid: &str, passphrase: &str) -> Self {
        WifiCredentials {
            ssid: ssid.to_string(),
            passphrase: Some(passphrase.to_string()),
        }
    }

    /// Checks the formal limits: name of 1 to 32 bytes; passphrase of 8 to 63 bytes
    /// without control characters, or exactly 64 hexadecimal digits.
    pub fn validate(&self) -> HalResult<()> {
        if self.ssid.is_empty() || self.ssid.len() > 32 {
            return Err(HalError::InvalidArgument);
        }
        let Some(passphrase) = &self.passphrase else {
            return Ok(());
        };
        let is_hex_key =
            passphrase.len() == 64 && passphrase.bytes().all(|byte| byte.is_ascii_hexdigit());
        let is_phrase =
            (8..=63).contains(&passphrase.len()) && !passphrase.chars().any(char::is_control);
        if is_hex_key || is_phrase {
            Ok(())
        } else {
            Err(HalError::InvalidArgument)
        }
    }
}

impl fmt::Debug for WifiCredentials {
    /// Never prints the passphrase.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WifiCredentials")
            .field("ssid", &self.ssid)
            .field("has_passphrase", &self.passphrase.is_some())
            .finish()
    }
}

/// State of the Wi-Fi adapter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WifiState {
    /// Adapter switched off.
    Off,
    /// On, not connected.
    Disconnected,
    /// Joining a network.
    Connecting,
    /// Connected.
    Connected,
}

/// Current Wi-Fi connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WifiStatus {
    /// State of the adapter.
    pub state: WifiState,
    /// Name of the network being joined or connected.
    pub ssid: Option<String>,
    /// Signal strength in dBm while connected.
    pub signal_dbm: Option<i16>,
    /// Frequency in MHz while connected.
    pub frequency_mhz: Option<u32>,
}

impl WifiStatus {
    /// Status of an adapter that is on or off and not connected.
    pub const fn idle(powered: bool) -> Self {
        WifiStatus {
            state: if powered {
                WifiState::Disconnected
            } else {
                WifiState::Off
            },
            ssid: None,
            signal_dbm: None,
            frequency_mhz: None,
        }
    }
}

/// Wi-Fi adapter of a backend.
pub trait Wifi: Send + Sync {
    /// True if the adapter is switched on.
    fn is_powered(&self) -> HalResult<bool>;
    /// Switches the adapter on or off.
    fn set_powered(&self, powered: bool) -> HalResult<()>;
    /// Scans and returns the networks in range.
    fn scan(&self) -> HalResult<Vec<AccessPoint>>;
    /// Joins a network.
    fn connect(&self, credentials: &WifiCredentials) -> HalResult<()>;
    /// Leaves the current network.
    fn disconnect(&self) -> HalResult<()>;
    /// Current connection.
    fn status(&self) -> HalResult<WifiStatus>;
}

/// A Bluetooth device found by a scan.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BluetoothDevice {
    /// Device address.
    pub address: [u8; 6],
    /// Name the device announces, if any.
    pub name: Option<String>,
    /// Signal strength in dBm, if measured.
    pub signal_dbm: Option<i16>,
    /// True if the device is already paired.
    pub paired: bool,
}

/// Bluetooth adapter of a backend.
pub trait Bluetooth: Send + Sync {
    /// True if the adapter is switched on.
    fn is_powered(&self) -> HalResult<bool>;
    /// Switches the adapter on or off.
    fn set_powered(&self, powered: bool) -> HalResult<()>;
    /// Scans for up to `duration_ms` and returns the devices in range.
    fn scan(&self, duration_ms: u32) -> HalResult<Vec<BluetoothDevice>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn access_point(ssid: &str, last_byte: u8, signal_dbm: i16) -> AccessPoint {
        AccessPoint {
            ssid: ssid.into(),
            bssid: [2, 0, 0, 0, 0, last_byte],
            signal_dbm,
            frequency_mhz: 2412,
            security: WifiSecurity::Wpa2,
        }
    }

    #[test]
    fn signal_percent_range() {
        assert_eq!(access_point("a", 1, -100).signal_percent(), 0);
        assert_eq!(access_point("a", 1, -120).signal_percent(), 0);
        assert_eq!(access_point("a", 1, -75).signal_percent(), 50);
        assert_eq!(access_point("a", 1, -50).signal_percent(), 100);
        assert_eq!(access_point("a", 1, -20).signal_percent(), 100);
    }

    #[test]
    fn one_entry_per_network() {
        let found = vec![
            access_point("home", 1, -70),
            access_point("", 2, -30),
            access_point("home", 3, -45),
            access_point("cafe", 4, -60),
            access_point("alpha", 5, -60),
        ];
        let tidy = AccessPoint::strongest_per_network(found);
        let names: Vec<(&str, u8)> = tidy
            .iter()
            .map(|ap| (ap.ssid.as_str(), ap.bssid[5]))
            .collect();
        assert_eq!(names, vec![("home", 3), ("alpha", 5), ("cafe", 4)]);
        assert!(AccessPoint::strongest_per_network(Vec::new()).is_empty());
    }

    #[test]
    fn credential_limits() {
        assert!(WifiCredentials::open("net").validate().is_ok());
        assert!(WifiCredentials::open("").validate().is_err());
        assert!(WifiCredentials::open(&"x".repeat(33)).validate().is_err());
        assert!(WifiCredentials::open(&"x".repeat(32)).validate().is_ok());
        let check =
            |passphrase: &str| WifiCredentials::with_passphrase("net", passphrase).validate();
        assert!(check("12345678").is_ok());
        assert!(check("1234567").is_err());
        assert!(check(&"p".repeat(63)).is_ok());
        assert!(check(&"p".repeat(64)).is_err());
        assert!(check(&"a1".repeat(32)).is_ok());
        assert!(check("كلمة السر").is_ok());
        assert!(check("line\nbreak").is_err());
        assert!(check("tab\tinside").is_err());
    }

    #[test]
    fn debug_hides_passphrase() {
        let credentials = WifiCredentials::with_passphrase("net", "very-secret-1");
        let printed = format!("{credentials:?}");
        assert!(printed.contains("net") && !printed.contains("very-secret-1"));
    }

    #[test]
    fn security_and_idle_status() {
        assert!(!WifiSecurity::Open.needs_passphrase());
        assert!(!WifiSecurity::Enterprise.needs_passphrase());
        assert!(WifiSecurity::Wpa3.needs_passphrase());
        assert_eq!(WifiStatus::idle(false).state, WifiState::Off);
        assert_eq!(WifiStatus::idle(true).state, WifiState::Disconnected);
    }
}
