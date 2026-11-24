//! The quick settings: which tiles exist, their state and whether they can be used now.
//!
//! A tile appears only with its hardware (DESKTOP-SPEC §5.1); hardware the shell does not
//! know about yet counts as present. A tile whose service is missing is shown disabled with
//! the reason as its state line – never a click without effect.

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
        }
    }

    /// The tile has a "choose" part (a list inside the sheet).
    pub fn has_list(self) -> bool {
        matches!(
            self,
            TileId::Wifi | TileId::Bluetooth | TileId::Vpn | TileId::PowerMode | TileId::Mesh
        )
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

fn service_tile(id: TileId, presence: &Presence, airplane: bool) -> Option<Tile> {
    let hardware = match id {
        TileId::Wifi => presence.wifi,
        TileId::Bluetooth => presence.bluetooth,
        TileId::Mesh => Some(presence.mesh),
        _ => None,
    };
    if hardware == Some(false) {
        return None;
    }
    let avail = if airplane && matches!(id, TileId::Wifi | TileId::Bluetooth) {
        Avail::Disabled("desk.reason.airplane")
    } else if presence.net_service {
        Avail::Ready
    } else {
        Avail::Disabled("desk.reason.no_service")
    };
    let detail = match avail {
        Avail::Disabled(reason) => reason,
        Avail::Ready => "desk.state.off",
    };
    Some(Tile {
        id,
        on: false,
        detail,
        avail,
    })
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

/// The tiles shown, in board order.
pub fn tiles(presence: &Presence, values: &QuickValues) -> Vec<Tile> {
    TileId::ALL
        .into_iter()
        .filter_map(|id| match id.setting() {
            Some(_) => Some(setting_tile(id, presence, values)),
            None => service_tile(id, presence, false),
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
        let all = tiles(&p, &v);
        assert_eq!(all.len(), 9, "no mesh component");
        let dark = all.iter().find(|t| t.id == TileId::Dark).unwrap();
        assert!(dark.on && dark.avail == Avail::Ready);
        let wifi = all.iter().find(|t| t.id == TileId::Wifi).unwrap();
        assert_eq!(wifi.avail, Avail::Disabled("desk.reason.no_service"));
        let no_wifi = Presence {
            wifi: Some(false),
            ..p
        };
        assert!(!tiles(&no_wifi, &v).iter().any(|t| t.id == TileId::Wifi));
        let offline = tiles(&Presence::default(), &v);
        assert!(offline
            .iter()
            .filter(|t| t.id.setting().is_some())
            .all(|t| !matches!(t.avail, Avail::Ready)));
        assert_eq!(PowerMode::Saver.next(), PowerMode::Performance);
        assert_eq!(PowerMode::parse("saver"), Some(PowerMode::Saver));
        assert_eq!(PowerMode::parse("x"), None);
        let ready = Presence {
            net_service: true,
            ..p
        };
        assert_eq!(
            service_tile(TileId::Wifi, &ready, true).unwrap().avail,
            Avail::Disabled("desk.reason.airplane")
        );
        assert!(TileId::ALL.iter().all(|t| !t.name_key().is_empty()));
    }
}
