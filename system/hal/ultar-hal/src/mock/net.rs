//! Mock network interfaces, Wi-Fi and Bluetooth adapters.

use super::cell::MockCell;
use crate::connectivity::{
    AccessPoint, Bluetooth, BluetoothDevice, InterfaceAddress, InterfaceKind, LinkState,
    NetInterface, Network, Wifi, WifiCredentials, WifiSecurity, WifiState, WifiStatus,
};
use crate::error::{HalError, HalResult};
use std::net::IpAddr;

#[derive(Debug, Default)]
struct NetworkState {
    interfaces: Vec<NetInterface>,
}

impl NetworkState {
    fn interface(&mut self, name: &str) -> HalResult<&mut NetInterface> {
        self.interfaces
            .iter_mut()
            .find(|interface| interface.name == name)
            .ok_or(HalError::NotFound)
    }
}

/// Scriptable network interfaces. Starts without interfaces.
#[derive(Debug, Default)]
pub struct MockNetwork {
    cell: MockCell<NetworkState>,
}

impl MockNetwork {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Adds an interface that is switched on; the hardware address is derived from
    /// the position of the interface.
    pub fn add_interface(&self, name: &str, kind: InterfaceKind) {
        let mut state = self.cell.state();
        let number = state.interfaces.len() as u8;
        let mac = (kind != InterfaceKind::Loopback).then_some([2, 0, 0, 0, 0, number]);
        state.interfaces.push(NetInterface {
            name: name.to_string(),
            kind,
            mac,
            link: LinkState::Up,
            addresses: Vec::new(),
        });
    }

    /// Assigns an address to an interface.
    pub fn add_address(&self, name: &str, address: IpAddr, prefix_len: u8) -> HalResult<()> {
        let mut state = self.cell.state();
        state.interface(name)?.addresses.push(InterfaceAddress {
            address,
            prefix_len,
        });
        Ok(())
    }

    /// Plugs the cable in or pulls it out (only visible while the interface is on).
    pub fn set_carrier(&self, name: &str, carrier: bool) -> HalResult<()> {
        let mut state = self.cell.state();
        let interface = state.interface(name)?;
        if interface.link != LinkState::Down {
            interface.link = if carrier {
                LinkState::Up
            } else {
                LinkState::NoCarrier
            };
        }
        Ok(())
    }
}

impl Network for MockNetwork {
    fn interfaces(&self) -> HalResult<Vec<NetInterface>> {
        self.cell.call(|state| Ok(state.interfaces.clone()))
    }

    fn link_state(&self, name: &str) -> HalResult<LinkState> {
        self.cell.call(|state| Ok(state.interface(name)?.link))
    }

    fn addresses(&self, name: &str) -> HalResult<Vec<InterfaceAddress>> {
        self.cell
            .call(|state| Ok(state.interface(name)?.addresses.clone()))
    }

    fn set_link_up(&self, name: &str, up: bool) -> HalResult<()> {
        self.cell.call(|state| {
            state.interface(name)?.link = if up { LinkState::Up } else { LinkState::Down };
            Ok(())
        })
    }
}

#[derive(Debug)]
struct KnownNetwork {
    access_point: AccessPoint,
    passphrase: Option<String>,
}

#[derive(Debug)]
struct WifiAdapter {
    powered: bool,
    networks: Vec<KnownNetwork>,
    connected: Option<usize>,
}

/// Scriptable Wi-Fi adapter. Starts switched on with no network in range.
#[derive(Debug)]
pub struct MockWifi {
    cell: MockCell<WifiAdapter>,
}

impl Default for MockWifi {
    fn default() -> Self {
        MockWifi {
            cell: MockCell::new(WifiAdapter {
                powered: true,
                networks: Vec::new(),
                connected: None,
            }),
        }
    }
}

impl MockWifi {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Puts a network in range. `passphrase` is what [`Wifi::connect`] must present;
    /// `None` makes the network open.
    pub fn add_network(&self, ssid: &str, passphrase: Option<&str>, signal_dbm: i16) {
        let mut state = self.cell.state();
        let number = state.networks.len() as u8;
        state.networks.push(KnownNetwork {
            access_point: AccessPoint {
                ssid: ssid.to_string(),
                bssid: [2, 0, 0, 0, 1, number],
                signal_dbm,
                frequency_mhz: 2412 + 5 * u32::from(number % 13),
                security: if passphrase.is_some() {
                    WifiSecurity::Wpa2
                } else {
                    WifiSecurity::Open
                },
            },
            passphrase: passphrase.map(str::to_string),
        });
    }

    /// Takes every network out of range and drops the connection.
    pub fn clear_networks(&self) {
        let mut state = self.cell.state();
        state.networks.clear();
        state.connected = None;
    }
}

impl Wifi for MockWifi {
    fn is_powered(&self) -> HalResult<bool> {
        self.cell.call(|state| Ok(state.powered))
    }

    fn set_powered(&self, powered: bool) -> HalResult<()> {
        self.cell.call(|state| {
            state.powered = powered;
            if !powered {
                state.connected = None;
            }
            Ok(())
        })
    }

    fn scan(&self) -> HalResult<Vec<AccessPoint>> {
        self.cell.call(|state| {
            if !state.powered {
                return Err(HalError::Rejected);
            }
            Ok(state
                .networks
                .iter()
                .map(|network| network.access_point.clone())
                .collect())
        })
    }

    fn connect(&self, credentials: &WifiCredentials) -> HalResult<()> {
        self.cell.call(|state| {
            credentials.validate()?;
            if !state.powered {
                return Err(HalError::Rejected);
            }
            let index = state
                .networks
                .iter()
                .position(|network| network.access_point.ssid == credentials.ssid)
                .ok_or(HalError::NotFound)?;
            if state.networks[index].passphrase != credentials.passphrase {
                return Err(HalError::Rejected);
            }
            state.connected = Some(index);
            Ok(())
        })
    }

    fn disconnect(&self) -> HalResult<()> {
        self.cell.call(|state| {
            state.connected = None;
            Ok(())
        })
    }

    fn status(&self) -> HalResult<WifiStatus> {
        self.cell.call(|state| {
            let connected = state.connected.and_then(|index| state.networks.get(index));
            Ok(match connected {
                Some(network) => WifiStatus {
                    state: WifiState::Connected,
                    ssid: Some(network.access_point.ssid.clone()),
                    signal_dbm: Some(network.access_point.signal_dbm),
                    frequency_mhz: Some(network.access_point.frequency_mhz),
                },
                None => WifiStatus::idle(state.powered),
            })
        })
    }
}

#[derive(Debug, Default)]
struct BluetoothAdapter {
    powered: bool,
    devices: Vec<BluetoothDevice>,
}

/// Scriptable Bluetooth adapter. Starts switched off with no device in range.
#[derive(Debug, Default)]
pub struct MockBluetooth {
    cell: MockCell<BluetoothAdapter>,
}

impl MockBluetooth {
    /// Makes the next trait call fail with `error`.
    pub fn fail_next(&self, error: HalError) {
        self.cell.fail_next(error);
    }

    /// Puts a device in range.
    pub fn add_device(&self, name: Option<&str>, paired: bool) {
        let mut state = self.cell.state();
        let number = state.devices.len() as u8;
        state.devices.push(BluetoothDevice {
            address: [2, 0, 0, 0, 2, number],
            name: name.map(str::to_string),
            signal_dbm: Some(-60 - i16::from(number)),
            paired,
        });
    }
}

impl Bluetooth for MockBluetooth {
    fn is_powered(&self) -> HalResult<bool> {
        self.cell.call(|state| Ok(state.powered))
    }

    fn set_powered(&self, powered: bool) -> HalResult<()> {
        self.cell.call(|state| {
            state.powered = powered;
            Ok(())
        })
    }

    fn scan(&self, _duration_ms: u32) -> HalResult<Vec<BluetoothDevice>> {
        self.cell.call(|state| {
            if !state.powered {
                return Err(HalError::Rejected);
            }
            Ok(state.devices.clone())
        })
    }
}
