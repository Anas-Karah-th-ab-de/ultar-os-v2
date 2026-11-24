//! The quick settings: which tiles exist, their state and whether they can be used now.
//!
//! A tile appears only with its hardware (DESKTOP-SPEC §5.1); hardware the shell does not
//! know about yet counts as present. A tile whose service is missing is shown disabled with
//! the reason as its state line – never a click without effect.

use super::services::Services;
use ultar_bus::messages::bluetooth::BluetoothState;
use ultar_bus::messages::net::NetState;
use ultar_ui::icon::IconId;

/// The tiles of the board (order of the board: two columns, row by row).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TileId {
    /// Wi-Fi on/off (+ network list).
    Wifi,
    /// Wired network (state only, opens Settings > Network).
    Wired,
    /// Bluetooth on/off (+ device list).
    Bluetooth,
    /// Airplane mode.
    Airplane,
    /// VPN (+ connection list).
    Vpn,
    /// Night light.
    NightLight,
    /// Dark theme.
    Dark,
    /// Do not disturb.
    Dnd,
    /// Power mode (+ list of the three modes).
    PowerMode,
    /// Mesh (component).
    Mesh,
    /// Mobile data on/off (+ SIM choice); phones only, with a modem.
    Mobile,
}

impl TileId {
    /// All tiles in board order.
    pub const ALL: [TileId; 10] = [
        TileId::Wifi,
        TileId::Wired,
        TileId::Bluetooth,
        TileId::Airplane,
        TileId::Vpn,
        TileId::NightLight,
        TileId::Dark,
        TileId::Dnd,
        TileId::PowerMode,
        TileId::Mesh,
    ];

    /// The tiles of the phone's quick settings in board order (CompactQuick).
    pub const COMPACT: [TileId; 6] = [
        TileId::Wifi,
        TileId::Mobile,
        TileId::Bluetooth,
        TileId::Airplane,
        TileId::NightLight,
        TileId::Dnd,
    ];

    /// Icon (ICONS.md).
    pub fn icon(self) -> IconId {
        match self {
            TileId::Wifi => IconId::Wifi,
            TileId::Wired => IconId::Cable,
            TileId::Bluetooth => IconId::Bluetooth,
            TileId::Airplane => IconId::Plane,
            TileId::Vpn => IconId::Vpn,
            TileId::NightLight => IconId::Moon,
            TileId::Dark => IconId::Contrast,
            TileId::Dnd => IconId::BellOff,
            TileId::PowerMode => IconId::Battery,
            TileId::Mesh => IconId::Mesh,
            TileId::Mobile => IconId::Cell,
        }
    }

    /// Catalog key of the name.
    pub fn name_key(self) -> &'static str {
        match self {
            TileId::Wifi => "desk.qs.wifi",
            TileId::Wired => "desk.qs.wired",
            TileId::Bluetooth => "desk.qs.bluetooth",
            TileId::Airplane => "desk.qs.airplane",
            TileId::Vpn => "desk.qs.vpn",
            TileId::NightLight => "desk.qs.night",
            TileId::Dark => "desk.qs.dark",
            TileId::Dnd => "desk.qs.dnd",
            TileId::PowerMode => "desk.qs.power_mode",
            TileId::Mesh => "desk.qs.mesh",
            TileId::Mobile => "desk.qs.mobile",
        }
    }

    /// The tile has a "choose" part (a list inside the sheet).
    pub fn has_list(self) -> bool {
        matches!(
            self,
            TileId::Wifi
                | TileId::Bluetooth
                | TileId::Vpn
                | TileId::PowerMode
                | TileId::Mesh
                | TileId::Mobile
        )
    }

    /// The area of the Settings app where this tile's part of the system is set up
    /// (`settings://<area>`): where a tile goes whose service cannot switch it now.
    pub fn settings_area(self) -> &'static str {
        match self {
            TileId::Wifi => "wifi",
            TileId::Wired | TileId::Airplane | TileId::Vpn => "network",
            TileId::Bluetooth => "bluetooth",
            TileId::NightLight => "displays",
            TileId::Dark => "appearance",
            TileId::Dnd => "notifications",
            TileId::PowerMode => "power",
            TileId::Mesh => "mesh",
            TileId::Mobile => "cellular",
        }
    }

    /// Settings key the tile switches (the tiles that work through `settingsd`).
    pub fn setting(self) -> Option<&'static str> {
        match self {
            TileId::NightLight => Some("display.night_light.enabled"),
            TileId::Dark => Some("appearance.theme"),
            TileId::Dnd => Some("notifications.do_not_disturb"),
            TileId::PowerMode => Some("power.mode"),
            _ => None,
        }
    }
}

/// The three power modes (`power.mode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PowerMode {
    /// Performance.
    Performance,
    /// Balanced.
    #[default]
    Balanced,
    /// Saver.
    Saver,
}

impl PowerMode {
    /// All, in the order the tile steps through them.
    pub const ALL: [PowerMode; 3] = [
        PowerMode::Performance,
        PowerMode::Balanced,
        PowerMode::Saver,
    ];

    /// Value of `power.mode`.
    pub fn word(self) -> &'static str {
        match self {
            PowerMode::Performance => "performance",
            PowerMode::Balanced => "balanced",
            PowerMode::Saver => "saver",
        }
    }

    /// From the setting's value.
    pub fn parse(word: &str) -> Option<PowerMode> {
        PowerMode::ALL.into_iter().find(|m| m.word() == word)
    }

    /// The next mode of the tile.
    pub fn next(self) -> PowerMode {
        match self {
            PowerMode::Performance => PowerMode::Balanced,
            PowerMode::Balanced => PowerMode::Saver,
            PowerMode::Saver => PowerMode::Performance,
        }
    }
}

/// Whether a tile can be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Avail {
    /// It works.
    Ready,
    /// Shown disabled: the catalog key of the reason is the state line.
    Disabled(&'static str),
}

/// What the shell knows about the device and the services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Presence {
    /// The settings service answers.
    pub settings: bool,
    /// Wi-Fi hardware: `Some(false)` = known to be absent.
    pub wifi: Option<bool>,
    /// Bluetooth hardware.
    pub bluetooth: Option<bool>,
    /// The Mesh component is installed.
    pub mesh: bool,
    /// The network service has a bus contract (not yet in this release).
    pub net_service: bool,
    /// The audio service has a bus contract (not yet in this release).
    pub audio_service: bool,
    /// The app launcher answers.
    pub launcher: bool,
    /// The shell is on the bus (calls without a state topic, such as the screenshot, are
    /// tried; a failure says so).
    pub bus: bool,
    /// Modem present (`None` = not known yet).
    pub modem: Option<bool>,
}

/// Values of the settings-backed tiles and sliders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct QuickValues {
    /// Night light on.
    pub night: bool,
    /// Dark theme.
    pub dark: bool,
    /// Do not disturb.
    pub dnd: bool,
    /// Power mode.
    pub power: PowerMode,
    /// Brightness 0–100 of the main display, if its setting is known.
    pub brightness: Option<u8>,
    /// Battery percent and charging, if the device has one and reports it.
    pub battery: Option<(u8, bool)>,
    /// accessibility.high_contrast: the high-contrast palette on every surface of the shell.
    pub high_contrast: bool,
    /// accessibility.large_text: text 1.25 times larger on every surface of the shell.
    pub large_text: bool,
    /// accessibility.reduce_motion: nothing of the shell moves on its own.
    pub reduce_motion: bool,
}

/// One tile as shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    /// Which.
    pub id: TileId,
    /// On.
    pub on: bool,
    /// Catalog key of the state line.
    pub detail: &'static str,
    /// Usable now.
    pub avail: Avail,
}

fn service_tile(id: TileId, presence: &Presence, svc: &Services) -> Option<Tile> {
    let hardware = match id {
        TileId::Wifi => presence.wifi,
        TileId::Bluetooth => presence.bluetooth,
        TileId::Mesh => Some(presence.mesh),
        TileId::Mobile => presence.modem,
        _ => None,
    };
    if hardware == Some(false) {
        return None;
    }
    match id {
        TileId::Wired => return Some(wired_tile(presence, svc)),
        TileId::Wifi | TileId::Airplane => {
            if let Some(net) = &svc.net {
                return radio_tile(id, net).map(|t| wifi_line(t, svc));
            }
        }
        TileId::Vpn => {
            if let Some(vpn) = &svc.vpn {
                return Some(vpn_tile(vpn));
            }
        }
        TileId::Bluetooth => {
            if let Some(bt) = &svc.bluetooth {
                return bluetooth_tile(bt, svc.net.as_ref().is_some_and(|n| n.airplane));
            }
        }
        TileId::Mesh => {
            if let Some(mesh) = &svc.mesh {
                return Some(switch_tile(id, mesh.enabled, Avail::Ready));
            }
        }
        _ => {}
    }
    // No service on the bus (VPN, mobile data, or one that never reported).
    Some(Tile {
        id,
        on: false,
        detail: "desk.reason.no_service",
        avail: Avail::Disabled("desk.reason.no_service"),
    })
}

/// Wi-Fi radio and airplane mode from `net.state`; an absent radio hides its tile.
fn radio_tile(id: TileId, net: &NetState) -> Option<Tile> {
    let (on, avail) = if id == TileId::Airplane {
        if !net.airplane_possible {
            return None;
        }
        (net.airplane, Avail::Ready)
    } else {
        if !net.wifi_present {
            return None;
        }
        let avail = if net.wifi_switch_off {
            Avail::Disabled("desk.reason.radio_switch")
        } else if net.airplane {
            Avail::Disabled("desk.reason.airplane")
        } else {
            Avail::Ready
        };
        (net.wifi_on, avail)
    };
    Some(switch_tile(id, on, avail))
}

/// The Wi-Fi tile's line while the radio is on: what `net.wifi` says (the connected
/// network's name is data: [`tile_data`]).
fn wifi_line(mut tile: Tile, svc: &Services) -> Tile {
    let phase = svc.wifi.link.as_ref().map(|l| l.phase);
    if tile.id == TileId::Wifi && tile.on && tile.avail == Avail::Ready {
        if let Some(key) = phase.and_then(super::wifi::phase_key) {
            tile.detail = key;
        }
    }
    tile
}

/// The VPN from `vpn.state`: on while up or connecting; disabled without TUN or right.
fn vpn_tile(vpn: &ultar_bus::messages::vpn::VpnState) -> Tile {
    let reason = super::wifi::vpn_reason(vpn);
    Tile {
        id: TileId::Vpn,
        on: super::wifi::vpn_on(vpn),
        detail: reason.unwrap_or_else(|| super::wifi::vpn_key(vpn)),
        avail: reason.map_or(Avail::Ready, Avail::Disabled),
    }
}

/// A tile's line that is data, not text of the catalog: the connected Wi-Fi network
/// (board: "KM-Office"), the VPN connection in use.
pub fn tile_data(tile: &Tile, svc: &Services) -> Option<String> {
    if tile.avail != Avail::Ready || !tile.on {
        return None;
    }
    match tile.id {
        TileId::Wifi => super::wifi::connected_ssid(svc.wifi.link.as_ref()),
        TileId::Vpn => svc.vpn.as_ref().and_then(|v| v.active.clone()),
        _ => None,
    }
}

/// The Bluetooth radio from `bluetooth.state`.
fn bluetooth_tile(bt: &BluetoothState, airplane: bool) -> Option<Tile> {
    if !bt.present {
        return None;
    }
    let avail = if bt.switch_off {
        Avail::Disabled("desk.reason.radio_switch")
    } else if airplane {
        Avail::Disabled("desk.reason.airplane")
    } else {
        Avail::Ready
    };
    Some(switch_tile(TileId::Bluetooth, bt.on, avail))
}

fn switch_tile(id: TileId, on: bool, avail: Avail) -> Tile {
    let detail = match avail {
        Avail::Disabled(reason) => reason,
        Avail::Ready if on => "desk.state.on",
        Avail::Ready => "desk.state.off",
    };
    Tile {
        id,
        on,
        detail,
        avail,
    }
}

/// Whether the list part of a tile works: Wi-Fi networks need a Wi-Fi device the service
/// drives (`networks_supported`), Bluetooth devices the host stack (`devices_supported`);
/// the others follow their tile.
pub fn list_reason(id: TileId, svc: &Services) -> Option<&'static str> {
    let supported = match id {
        TileId::Wifi if svc.net.as_ref().is_some_and(|n| !n.wifi_on) => {
            return Some("desk.reason.wifi_off")
        }
        TileId::Wifi => svc.net.as_ref().is_some_and(|n| n.networks_supported),
        TileId::Bluetooth => svc.bluetooth.as_ref().is_some_and(|b| b.devices_supported),
        _ => return None,
    };
    let reason = match id {
        TileId::Wifi => "desk.reason.no_wifi_device",
        _ => "desk.reason.no_bt_device",
    };
    (!supported).then_some(reason)
}

/// qs.wired is no switch: it opens Settings > Network, which needs only the launcher. Its
/// state line says that until the network service reports the cable (`net.state`).
fn wired_tile(presence: &Presence, svc: &Services) -> Tile {
    let cable = svc.net.as_ref().is_some_and(|n| n.wired.is_some());
    let (detail, avail) = if presence.launcher {
        let detail = if cable {
            "desk.qs.wired.connected"
        } else {
            "desk.qs.wired.open"
        };
        (detail, Avail::Ready)
    } else {
        (
            "desk.reason.no_launcher",
            Avail::Disabled("desk.reason.no_launcher"),
        )
    };
    Tile {
        id: TileId::Wired,
        on: cable,
        detail,
        avail,
    }
}

fn setting_tile(id: TileId, presence: &Presence, v: &QuickValues) -> Tile {
    let on = match id {
        TileId::NightLight => v.night,
        TileId::Dark => v.dark,
        TileId::Dnd => v.dnd,
        _ => true,
    };
    let detail = match (id, on) {
        (TileId::PowerMode, _) => match v.power {
            PowerMode::Performance => "desk.power.performance",
            PowerMode::Balanced => "desk.power.balanced",
            PowerMode::Saver => "desk.power.saver",
        },
        (_, true) => "desk.state.on",
        (_, false) => "desk.state.off",
    };
    if !presence.settings {
        return Tile {
            id,
            on: false,
            detail: "desk.reason.no_settings",
            avail: Avail::Disabled("desk.reason.no_settings"),
        };
    }
    Tile {
        id,
        on,
        detail,
        avail: Avail::Ready,
    }
}

/// One tile as [`tiles`] and [`compact_tiles`] show it (`None`: its hardware is absent).
pub fn tile_of(
    id: TileId,
    presence: &Presence,
    values: &QuickValues,
    svc: &Services,
) -> Option<Tile> {
    match id.setting() {
        Some(_) => Some(setting_tile(id, presence, values)),
        None => service_tile(id, presence, svc),
    }
}

/// The phone's tiles (board CompactQuick), by hardware and services like [`tiles`].
pub fn compact_tiles(presence: &Presence, values: &QuickValues, svc: &Services) -> Vec<Tile> {
    TileId::COMPACT
        .into_iter()
        .filter_map(|id| match id.setting() {
            Some(_) => Some(setting_tile(id, presence, values)),
            None => service_tile(id, presence, svc),
        })
        .collect()
}

/// The tiles shown, in board order.
pub fn tiles(presence: &Presence, values: &QuickValues, svc: &Services) -> Vec<Tile> {
    TileId::ALL
        .into_iter()
        .filter_map(|id| match id.setting() {
            Some(_) => Some(setting_tile(id, presence, values)),
            None => service_tile(id, presence, svc),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiles_follow_hardware_services_and_settings() {
        let p = Presence {
            settings: true,
            ..Presence::default()
        };
        let v = QuickValues {
            dark: true,
            ..QuickValues::default()
        };
        let none = Services::default();
        let all = tiles(&p, &v, &none);
        assert_eq!(all.len(), 9, "no mesh component");
        let dark = all.iter().find(|t| t.id == TileId::Dark).unwrap();
        assert!(dark.on && dark.avail == Avail::Ready);
        let wifi = all.iter().find(|t| t.id == TileId::Wifi).unwrap();
        assert_eq!(wifi.avail, Avail::Disabled("desk.reason.no_service"));
        let no_wifi = Presence {
            wifi: Some(false),
            ..p
        };
        assert!(!tiles(&no_wifi, &v, &none)
            .iter()
            .any(|t| t.id == TileId::Wifi));
        let offline = tiles(&Presence::default(), &v, &none);
        assert!(offline
            .iter()
            .filter(|t| t.id.setting().is_some())
            .all(|t| !matches!(t.avail, Avail::Ready)));
        assert_eq!(PowerMode::Saver.next(), PowerMode::Performance);
        assert_eq!(PowerMode::parse("saver"), Some(PowerMode::Saver));
        assert_eq!(PowerMode::parse("x"), None);
        assert!(TileId::ALL.iter().all(|t| !t.name_key().is_empty()));
        let phone = compact_tiles(&p, &v, &none);
        assert_eq!(phone.len(), 6);
        assert_eq!(phone[1].id, TileId::Mobile);
        assert!(TileId::Mobile.has_list() && TileId::Mobile.setting().is_none());
        let no_modem = Presence {
            modem: Some(false),
            ..p
        };
        assert!(!compact_tiles(&no_modem, &v, &none)
            .iter()
            .any(|t| t.id == TileId::Mobile));
        assert!(!all.iter().any(|t| t.id == TileId::Mobile), "desktop: none");
    }

    #[test]
    fn the_radio_tiles_follow_the_network_service() {
        let p = Presence {
            settings: true,
            ..Presence::default()
        };
        let mut svc = Services {
            net: Some(NetState {
                wifi_present: true,
                wifi_on: true,
                wifi_switch_off: false,
                ssid: None,
                signal_percent: None,
                wired: None,
                airplane: true,
                airplane_possible: true,
                metered: false,
                networks_supported: false,
                ..NetState::default()
            }),
            ..Services::default()
        };
        let wifi = service_tile(TileId::Wifi, &p, &svc).unwrap();
        assert_eq!(wifi.avail, Avail::Disabled("desk.reason.airplane"));
        let plane = service_tile(TileId::Airplane, &p, &svc).unwrap();
        assert!(plane.on && plane.avail == Avail::Ready);
        assert_eq!(
            list_reason(TileId::Wifi, &svc),
            Some("desk.reason.no_wifi_device")
        );
        svc.net.as_mut().unwrap().wifi_present = false;
        assert!(
            service_tile(TileId::Wifi, &p, &svc).is_none(),
            "no radio: no tile"
        );
    }
}
