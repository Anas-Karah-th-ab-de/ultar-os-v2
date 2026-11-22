//! Capability requirements: which apps fit the device and which settings panels show.

use crate::capabilities::Capabilities;
use crate::capability::{Capability, CapabilitySet};

/// True if the device has every capability a manifest (app, panel, service) requires.
pub fn requirements_met(required: &CapabilitySet, have: &Capabilities) -> bool {
    have.flags.contains_all(*required)
}

/// Panel of the settings app.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PanelId {
    /// Wired network, addresses, firewall.
    Network,
    /// Wi-Fi networks.
    Wifi,
    /// Bluetooth devices.
    Bluetooth,
    /// Mobile network, SIM cards, mobile data.
    MobileNetwork,
    /// Brightness, resolution, scaling, arrangement of displays.
    Display,
    /// Wallpaper, colours, text size.
    Appearance,
    /// Volume, outputs, inputs.
    Sound,
    /// Vibration strength and patterns.
    Vibration,
    /// Battery state and charging.
    Battery,
    /// Power profile, suspend behaviour.
    Power,
    /// Location services.
    Location,
    /// Camera permissions and defaults.
    Camera,
    /// Fingerprints.
    Fingerprint,
    /// Lock, encryption, permissions.
    Security,
    /// Keyboard layouts (hardware and on-screen).
    Keyboard,
    /// Mouse and touchpad.
    Pointer,
    /// Touch gestures.
    Gestures,
    /// Language and region.
    Language,
    /// Date, time, time zone.
    DateTime,
    /// Storage devices and usage.
    Storage,
    /// Installed apps and their permissions.
    Apps,
    /// User accounts.
    Accounts,
    /// System updates.
    Updates,
    /// Drivers and hardware.
    Drivers,
    /// About this device.
    About,
}

impl PanelId {
    /// Every panel in the order the settings app lists them.
    pub const ALL: [PanelId; 25] = [
        PanelId::Network,
        PanelId::Wifi,
        PanelId::Bluetooth,
        PanelId::MobileNetwork,
        PanelId::Display,
        PanelId::Appearance,
        PanelId::Sound,
        PanelId::Vibration,
        PanelId::Battery,
        PanelId::Power,
        PanelId::Location,
        PanelId::Camera,
        PanelId::Fingerprint,
        PanelId::Security,
        PanelId::Keyboard,
        PanelId::Pointer,
        PanelId::Gestures,
        PanelId::Language,
        PanelId::DateTime,
        PanelId::Storage,
        PanelId::Apps,
        PanelId::Accounts,
        PanelId::Updates,
        PanelId::Drivers,
        PanelId::About,
    ];

    /// Stable message key of the panel title (texts live with the settings app).
    pub const fn key(self) -> &'static str {
        match self {
            PanelId::Network => "settings.panel.network",
            PanelId::Wifi => "settings.panel.wifi",
            PanelId::Bluetooth => "settings.panel.bluetooth",
            PanelId::MobileNetwork => "settings.panel.mobile_network",
            PanelId::Display => "settings.panel.display",
            PanelId::Appearance => "settings.panel.appearance",
            PanelId::Sound => "settings.panel.sound",
            PanelId::Vibration => "settings.panel.vibration",
            PanelId::Battery => "settings.panel.battery",
            PanelId::Power => "settings.panel.power",
            PanelId::Location => "settings.panel.location",
            PanelId::Camera => "settings.panel.camera",
            PanelId::Fingerprint => "settings.panel.fingerprint",
            PanelId::Security => "settings.panel.security",
            PanelId::Keyboard => "settings.panel.keyboard",
            PanelId::Pointer => "settings.panel.pointer",
            PanelId::Gestures => "settings.panel.gestures",
            PanelId::Language => "settings.panel.language",
            PanelId::DateTime => "settings.panel.date_time",
            PanelId::Storage => "settings.panel.storage",
            PanelId::Apps => "settings.panel.apps",
            PanelId::Accounts => "settings.panel.accounts",
            PanelId::Updates => "settings.panel.updates",
            PanelId::Drivers => "settings.panel.drivers",
            PanelId::About => "settings.panel.about",
        }
    }
}

/// What a device needs so that a panel is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PanelRule {
    /// Capabilities that must all be present.
    pub all_of: CapabilitySet,
    /// Capabilities of which at least one must be present (empty = no such condition).
    pub any_of: CapabilitySet,
    /// The panel only makes sense with at least one display connected.
    pub needs_display: bool,
}

impl PanelRule {
    const ALWAYS: PanelRule = PanelRule {
        all_of: CapabilitySet::EMPTY,
        any_of: CapabilitySet::EMPTY,
        needs_display: false,
    };

    const fn all(capability: Capability) -> PanelRule {
        PanelRule {
            all_of: CapabilitySet::of(&[capability]),
            ..PanelRule::ALWAYS
        }
    }

    const fn any(list: &[Capability]) -> PanelRule {
        PanelRule {
            any_of: CapabilitySet::of(list),
            ..PanelRule::ALWAYS
        }
    }

    const DISPLAY: PanelRule = PanelRule {
        needs_display: true,
        ..PanelRule::ALWAYS
    };

    /// True if the capabilities satisfy this rule.
    pub fn is_met(&self, capabilities: &Capabilities) -> bool {
        capabilities.flags.contains_all(self.all_of)
            && (self.any_of.is_empty() || capabilities.flags.intersects(self.any_of))
            && (!self.needs_display || capabilities.has_display())
    }
}

/// Rule that decides whether a panel is shown.
pub const fn panel_rule(panel: PanelId) -> PanelRule {
    match panel {
        PanelId::Wifi => PanelRule::all(Capability::Wifi),
        PanelId::Bluetooth => PanelRule::all(Capability::Bluetooth),
        PanelId::MobileNetwork => PanelRule::all(Capability::Modem),
        PanelId::Display | PanelId::Appearance => PanelRule::DISPLAY,
        PanelId::Sound => PanelRule::any(&[Capability::AudioOut, Capability::AudioIn]),
        PanelId::Vibration => PanelRule::all(Capability::Haptics),
        PanelId::Battery => PanelRule::all(Capability::Battery),
        PanelId::Location => PanelRule::all(Capability::Gnss),
        PanelId::Camera => PanelRule::all(Capability::Camera),
        PanelId::Fingerprint => PanelRule::all(Capability::Fingerprint),
        PanelId::Keyboard => PanelRule::any(&[Capability::Keyboard, Capability::Touch]),
        PanelId::Pointer => PanelRule::all(Capability::Pointer),
        PanelId::Gestures => PanelRule::all(Capability::Touch),
        PanelId::Network
        | PanelId::Power
        | PanelId::Security
        | PanelId::Language
        | PanelId::DateTime
        | PanelId::Storage
        | PanelId::Apps
        | PanelId::Accounts
        | PanelId::Updates
        | PanelId::Drivers
        | PanelId::About => PanelRule::ALWAYS,
    }
}

/// Panels the settings app shows on a device with these capabilities, in list order.
pub fn visible_panels(capabilities: &Capabilities) -> Vec<PanelId> {
    PanelId::ALL
        .iter()
        .copied()
        .filter(|panel| panel_rule(*panel).is_met(capabilities))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(list: &[Capability], displays: u8) -> Capabilities {
        Capabilities {
            flags: CapabilitySet::of(list),
            display_count: displays,
            ..Capabilities::default()
        }
    }

    #[test]
    fn requirements() {
        let have = caps(&[Capability::Camera, Capability::Touch], 1);
        assert!(requirements_met(&CapabilitySet::EMPTY, &have));
        assert!(requirements_met(
            &CapabilitySet::of(&[Capability::Camera]),
            &have
        ));
        assert!(!requirements_met(
            &CapabilitySet::of(&[Capability::Camera, Capability::Modem]),
            &have
        ));
        assert!(!requirements_met(
            &CapabilitySet::of(&[Capability::Tpm]),
            &Capabilities::default()
        ));
    }

    #[test]
    fn bare_server_shows_only_general_panels() {
        let panels = visible_panels(&Capabilities::default());
        assert_eq!(
            panels,
            vec![
                PanelId::Network,
                PanelId::Power,
                PanelId::Security,
                PanelId::Language,
                PanelId::DateTime,
                PanelId::Storage,
                PanelId::Apps,
                PanelId::Accounts,
                PanelId::Updates,
                PanelId::Drivers,
                PanelId::About,
            ]
        );
    }

    #[test]
    fn hardware_panels_follow_capabilities() {
        let cases = [
            (PanelId::Wifi, Capability::Wifi),
            (PanelId::Bluetooth, Capability::Bluetooth),
            (PanelId::MobileNetwork, Capability::Modem),
            (PanelId::Vibration, Capability::Haptics),
            (PanelId::Battery, Capability::Battery),
            (PanelId::Location, Capability::Gnss),
            (PanelId::Camera, Capability::Camera),
            (PanelId::Fingerprint, Capability::Fingerprint),
            (PanelId::Pointer, Capability::Pointer),
            (PanelId::Gestures, Capability::Touch),
        ];
        for (panel, capability) in cases {
            assert!(
                !visible_panels(&Capabilities::default()).contains(&panel),
                "{panel:?}"
            );
            assert!(
                visible_panels(&caps(&[capability], 0)).contains(&panel),
                "{panel:?}"
            );
        }
    }

    #[test]
    fn any_of_and_display_rules() {
        for capability in [Capability::AudioOut, Capability::AudioIn] {
            assert!(visible_panels(&caps(&[capability], 0)).contains(&PanelId::Sound));
        }
        for capability in [Capability::Keyboard, Capability::Touch] {
            assert!(visible_panels(&caps(&[capability], 0)).contains(&PanelId::Keyboard));
        }
        let with_display = visible_panels(&caps(&[], 1));
        assert!(with_display.contains(&PanelId::Display));
        assert!(with_display.contains(&PanelId::Appearance));
        let without = visible_panels(&caps(&[Capability::Gpu], 0));
        assert!(!without.contains(&PanelId::Display));
        assert!(!without.contains(&PanelId::Appearance));
    }

    #[test]
    fn full_device_shows_everything_in_order() {
        let everything = Capabilities {
            flags: CapabilitySet::all(),
            display_count: 1,
            ..Capabilities::default()
        };
        assert_eq!(visible_panels(&everything), PanelId::ALL.to_vec());
    }

    #[test]
    fn panel_keys_are_unique() {
        let mut keys: Vec<&str> = PanelId::ALL.iter().map(|panel| panel.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), PanelId::ALL.len());
        assert!(keys.iter().all(|key| key.starts_with("settings.panel.")));
    }
}
