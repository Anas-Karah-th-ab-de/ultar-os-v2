//! The quick settings of the phone: the two large tiles, the round switches and the sliders.
//! Which tiles exist follows the hardware (no mobile data without a modem, no Bluetooth
//! without a radio).

use ultar_ui::icon::IconId;

/// A switch of the quick settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Toggle {
    /// Wi-Fi (large tile).
    Wifi,
    /// Mobile data (large tile; only with a modem).
    MobileData,
    /// Bluetooth.
    Bluetooth,
    /// Airplane mode.
    Airplane,
    /// Do not disturb.
    Dnd,
    /// Night light.
    NightLight,
    /// Battery saver (only with a battery).
    Saver,
    /// Location.
    Location,
    /// VPN.
    Vpn,
    /// Torch (only with a camera light).
    Torch,
    /// Device mesh (large tile; only with the mesh service). Its state lives in the mesh
    /// model, not in [`Quick`].
    Mesh,
}

impl Toggle {
    /// Every switch in the order of the quick settings.
    pub const ALL: [Toggle; 11] = [
        Toggle::Wifi,
        Toggle::MobileData,
        Toggle::Mesh,
        Toggle::Bluetooth,
        Toggle::Airplane,
        Toggle::Dnd,
        Toggle::NightLight,
        Toggle::Saver,
        Toggle::Location,
        Toggle::Vpn,
        Toggle::Torch,
    ];

    /// Its icon.
    pub fn icon(self) -> IconId {
        match self {
            Toggle::Wifi => IconId::Wifi,
            Toggle::MobileData => IconId::Cell,
            Toggle::Bluetooth => IconId::Bluetooth,
            Toggle::Airplane => IconId::Plane,
            Toggle::Dnd => IconId::BellOff,
            Toggle::NightLight => IconId::Moon,
            Toggle::Saver => IconId::Battery,
            Toggle::Location => IconId::Compass,
            Toggle::Vpn => IconId::Vpn,
            Toggle::Torch => IconId::Sun,
            Toggle::Mesh => IconId::Mesh,
        }
    }

    /// The catalog key of its name (`mobile.qs.<id>`).
    pub fn key(self) -> &'static str {
        match self {
            Toggle::Wifi => "mobile.qs.wifi",
            Toggle::MobileData => "mobile.qs.data",
            Toggle::Bluetooth => "mobile.qs.bluetooth",
            Toggle::Airplane => "mobile.qs.airplane",
            Toggle::Dnd => "mobile.qs.dnd",
            Toggle::NightLight => "mobile.qs.night",
            Toggle::Saver => "mobile.qs.saver",
            Toggle::Location => "mobile.qs.location",
            Toggle::Vpn => "mobile.qs.vpn",
            Toggle::Torch => "mobile.qs.torch",
            Toggle::Mesh => "mobile.qs.mesh",
        }
    }

    /// One of the two large tiles at the top.
    pub fn large(self) -> bool {
        matches!(self, Toggle::Wifi | Toggle::MobileData | Toggle::Mesh)
    }
}

/// What the phone has (decides which tiles are shown).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardware {
    /// A modem with a SIM slot.
    pub modem: bool,
    /// A Bluetooth radio.
    pub bluetooth: bool,
    /// A battery.
    pub battery: bool,
    /// A camera light.
    pub torch: bool,
    /// The mesh service runs.
    pub mesh: bool,
}

impl Hardware {
    /// A phone with everything.
    pub const PHONE: Hardware = Hardware {
        modem: true,
        bluetooth: true,
        battery: true,
        torch: true,
        mesh: false,
    };

    /// Whether the phone can offer `t`.
    pub fn offers(self, t: Toggle) -> bool {
        match t {
            Toggle::MobileData => self.modem,
            Toggle::Bluetooth => self.bluetooth,
            Toggle::Saver => self.battery,
            Toggle::Torch => self.torch,
            Toggle::Mesh => self.mesh,
            _ => true,
        }
    }
}

/// State of the quick settings.
#[derive(Debug, Clone, PartialEq)]
pub struct Quick {
    on: Vec<Toggle>,
    /// Screen brightness 0–100.
    pub brightness: u8,
    /// Media volume 0–100.
    pub volume: u8,
    /// Name of the Wi-Fi network in use.
    pub network: Option<String>,
}

impl Default for Quick {
    fn default() -> Quick {
        Quick {
            on: vec![Toggle::Wifi],
            brightness: 70,
            volume: 50,
            network: None,
        }
    }
}

impl Quick {
    /// Whether `t` is on.
    pub fn is_on(&self, t: Toggle) -> bool {
        self.on.contains(&t)
    }

    /// Sets `t`; airplane mode turns the radios off (they come back by hand).
    pub fn set(&mut self, t: Toggle, on: bool) {
        self.on.retain(|x| *x != t);
        if on {
            self.on.push(t);
            if t == Toggle::Airplane {
                self.on.retain(|x| {
                    !matches!(x, Toggle::Wifi | Toggle::MobileData | Toggle::Bluetooth)
                });
            }
        }
        self.on.sort_unstable();
    }

    /// The switches shown on `hw`, large tiles first.
    pub fn shown(hw: Hardware) -> (Vec<Toggle>, Vec<Toggle>) {
        let all = Toggle::ALL.into_iter().filter(|t| hw.offers(*t));
        all.partition(|t| t.large())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn airplane_mode_turns_the_radios_off() {
        let mut q = Quick::default();
        q.set(Toggle::Bluetooth, true);
        q.set(Toggle::Airplane, true);
        assert!(q.is_on(Toggle::Airplane));
        assert!(!q.is_on(Toggle::Wifi) && !q.is_on(Toggle::Bluetooth));
        q.set(Toggle::Wifi, true);
        assert!(q.is_on(Toggle::Wifi) && q.is_on(Toggle::Airplane));
        q.set(Toggle::Airplane, false);
        assert!(!q.is_on(Toggle::Airplane));
    }

    #[test]
    fn without_modem_there_is_no_data_tile() {
        let hw = Hardware {
            modem: false,
            ..Hardware::PHONE
        };
        let (large, small) = Quick::shown(hw);
        assert_eq!(large, vec![Toggle::Wifi]);
        assert!(!small.contains(&Toggle::MobileData));
        let (large, small) = Quick::shown(Hardware::PHONE);
        assert_eq!(large.len(), 2);
        assert_eq!(small.len(), 8);
        let (large, _) = Quick::shown(Hardware {
            mesh: true,
            ..Hardware::PHONE
        });
        assert_eq!(large, vec![Toggle::Wifi, Toggle::MobileData, Toggle::Mesh]);
    }
}
