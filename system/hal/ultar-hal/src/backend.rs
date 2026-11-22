//! The backend registry trait and the list of device classes.

use crate::audio::Audio;
use crate::camera::Camera;
use crate::connectivity::{Bluetooth, Network, Wifi};
use crate::display::{Display, Renderer};
use crate::input::Input;
use crate::misc::{Haptics, Leds, Storage, Tpm};
use crate::power::Power;
use crate::radio::Radio;
use crate::sensors::{Gnss, Sensors};
use std::fmt;
use ultar_profile::{CapabilitySet, CpuTopology};

/// A class of hardware, one per HAL trait.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DeviceClass {
    /// [`Display`].
    Display,
    /// [`Renderer`].
    Renderer,
    /// [`Input`].
    Input,
    /// [`Audio`].
    Audio,
    /// [`Power`].
    Power,
    /// [`Network`].
    Network,
    /// [`Wifi`].
    Wifi,
    /// [`Bluetooth`].
    Bluetooth,
    /// [`Radio`].
    Radio,
    /// [`Camera`].
    Camera,
    /// [`Sensors`].
    Sensors,
    /// [`Gnss`].
    Gnss,
    /// [`Haptics`].
    Haptics,
    /// [`Leds`].
    Leds,
    /// [`Storage`].
    Storage,
    /// [`Tpm`].
    Tpm,
}

impl DeviceClass {
    /// Number of classes.
    pub const COUNT: usize = 16;

    /// Every class, in a fixed order.
    pub const ALL: [DeviceClass; DeviceClass::COUNT] = [
        DeviceClass::Display,
        DeviceClass::Renderer,
        DeviceClass::Input,
        DeviceClass::Audio,
        DeviceClass::Power,
        DeviceClass::Network,
        DeviceClass::Wifi,
        DeviceClass::Bluetooth,
        DeviceClass::Radio,
        DeviceClass::Camera,
        DeviceClass::Sensors,
        DeviceClass::Gnss,
        DeviceClass::Haptics,
        DeviceClass::Leds,
        DeviceClass::Storage,
        DeviceClass::Tpm,
    ];

    /// Position of the class in [`DeviceClass::ALL`].
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Stable name used in stored choices and logs.
    pub const fn as_str(self) -> &'static str {
        match self {
            DeviceClass::Display => "display",
            DeviceClass::Renderer => "renderer",
            DeviceClass::Input => "input",
            DeviceClass::Audio => "audio",
            DeviceClass::Power => "power",
            DeviceClass::Network => "network",
            DeviceClass::Wifi => "wifi",
            DeviceClass::Bluetooth => "bluetooth",
            DeviceClass::Radio => "radio",
            DeviceClass::Camera => "camera",
            DeviceClass::Sensors => "sensors",
            DeviceClass::Gnss => "gnss",
            DeviceClass::Haptics => "haptics",
            DeviceClass::Leds => "leds",
            DeviceClass::Storage => "storage",
            DeviceClass::Tpm => "tpm",
        }
    }

    /// Parses a stable name.
    pub fn parse(name: &str) -> Option<DeviceClass> {
        let wanted = name.trim().to_ascii_lowercase();
        DeviceClass::ALL
            .iter()
            .copied()
            .find(|class| class.as_str() == wanted)
    }

    /// Stable message key of the class name for people (see [`crate::msg`]).
    pub const fn key(self) -> &'static str {
        match self {
            DeviceClass::Display => "hal.class.display",
            DeviceClass::Renderer => "hal.class.renderer",
            DeviceClass::Input => "hal.class.input",
            DeviceClass::Audio => "hal.class.audio",
            DeviceClass::Power => "hal.class.power",
            DeviceClass::Network => "hal.class.network",
            DeviceClass::Wifi => "hal.class.wifi",
            DeviceClass::Bluetooth => "hal.class.bluetooth",
            DeviceClass::Radio => "hal.class.radio",
            DeviceClass::Camera => "hal.class.camera",
            DeviceClass::Sensors => "hal.class.sensors",
            DeviceClass::Gnss => "hal.class.gnss",
            DeviceClass::Haptics => "hal.class.haptics",
            DeviceClass::Leds => "hal.class.leds",
            DeviceClass::Storage => "hal.class.storage",
            DeviceClass::Tpm => "hal.class.tpm",
        }
    }
}

impl fmt::Display for DeviceClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Facts about the machine that belong to no device class.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SystemSummary {
    /// CPU topology.
    pub cpu: CpuTopology,
    /// Installed memory in mebibytes.
    pub memory_mb: u32,
    /// Capabilities that have no HAL trait of their own (neural processor,
    /// fingerprint reader).
    pub extra: CapabilitySet,
}

impl SystemSummary {
    /// True if the summary carries no information.
    pub fn is_empty(&self) -> bool {
        *self == SystemSummary::default()
    }
}

/// One backend: a registry that hands out the device classes it implements.
///
/// Every class method defaults to `None`, so a backend only overrides what it has.
pub trait Backend: Send + Sync {
    /// Short technical name, for example `generic-linux` or `exynos7885`.
    fn name(&self) -> &str;
    /// Checks whether this backend works on the running device. May touch hardware.
    fn probe(&self) -> bool;

    /// Facts about the machine outside the device classes.
    fn system(&self) -> SystemSummary {
        SystemSummary::default()
    }
    /// Display outputs.
    fn display(&self) -> Option<&dyn Display> {
        None
    }
    /// Renderer capability query.
    fn renderer(&self) -> Option<&dyn Renderer> {
        None
    }
    /// Input devices.
    fn input(&self) -> Option<&dyn Input> {
        None
    }
    /// Sound hardware.
    fn audio(&self) -> Option<&dyn Audio> {
        None
    }
    /// Power hardware.
    fn power(&self) -> Option<&dyn Power> {
        None
    }
    /// Network interfaces.
    fn network(&self) -> Option<&dyn Network> {
        None
    }
    /// Wi-Fi adapter.
    fn wifi(&self) -> Option<&dyn Wifi> {
        None
    }
    /// Bluetooth adapter.
    fn bluetooth(&self) -> Option<&dyn Bluetooth> {
        None
    }
    /// Cellular modem.
    fn radio(&self) -> Option<&dyn Radio> {
        None
    }
    /// Cameras.
    fn camera(&self) -> Option<&dyn Camera> {
        None
    }
    /// Sensors.
    fn sensors(&self) -> Option<&dyn Sensors> {
        None
    }
    /// Satellite receiver.
    fn gnss(&self) -> Option<&dyn Gnss> {
        None
    }
    /// Vibration motor.
    fn haptics(&self) -> Option<&dyn Haptics> {
        None
    }
    /// Indicator lights.
    fn leds(&self) -> Option<&dyn Leds> {
        None
    }
    /// Block storage.
    fn storage(&self) -> Option<&dyn Storage> {
        None
    }
    /// Trusted platform module.
    fn tpm(&self) -> Option<&dyn Tpm> {
        None
    }

    /// True if the backend implements the class.
    fn provides(&self, class: DeviceClass) -> bool {
        match class {
            DeviceClass::Display => self.display().is_some(),
            DeviceClass::Renderer => self.renderer().is_some(),
            DeviceClass::Input => self.input().is_some(),
            DeviceClass::Audio => self.audio().is_some(),
            DeviceClass::Power => self.power().is_some(),
            DeviceClass::Network => self.network().is_some(),
            DeviceClass::Wifi => self.wifi().is_some(),
            DeviceClass::Bluetooth => self.bluetooth().is_some(),
            DeviceClass::Radio => self.radio().is_some(),
            DeviceClass::Camera => self.camera().is_some(),
            DeviceClass::Sensors => self.sensors().is_some(),
            DeviceClass::Gnss => self.gnss().is_some(),
            DeviceClass::Haptics => self.haptics().is_some(),
            DeviceClass::Leds => self.leds().is_some(),
            DeviceClass::Storage => self.storage().is_some(),
            DeviceClass::Tpm => self.tpm().is_some(),
        }
    }
}

/// A shared backend is a backend: lets a service and a [`crate::BackendSet`] (or a
/// test and the mock) hold the same instance.
impl<T: Backend + ?Sized> Backend for std::sync::Arc<T> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn probe(&self) -> bool {
        (**self).probe()
    }
    fn system(&self) -> SystemSummary {
        (**self).system()
    }
    fn display(&self) -> Option<&dyn Display> {
        (**self).display()
    }
    fn renderer(&self) -> Option<&dyn Renderer> {
        (**self).renderer()
    }
    fn input(&self) -> Option<&dyn Input> {
        (**self).input()
    }
    fn audio(&self) -> Option<&dyn Audio> {
        (**self).audio()
    }
    fn power(&self) -> Option<&dyn Power> {
        (**self).power()
    }
    fn network(&self) -> Option<&dyn Network> {
        (**self).network()
    }
    fn wifi(&self) -> Option<&dyn Wifi> {
        (**self).wifi()
    }
    fn bluetooth(&self) -> Option<&dyn Bluetooth> {
        (**self).bluetooth()
    }
    fn radio(&self) -> Option<&dyn Radio> {
        (**self).radio()
    }
    fn camera(&self) -> Option<&dyn Camera> {
        (**self).camera()
    }
    fn sensors(&self) -> Option<&dyn Sensors> {
        (**self).sensors()
    }
    fn gnss(&self) -> Option<&dyn Gnss> {
        (**self).gnss()
    }
    fn haptics(&self) -> Option<&dyn Haptics> {
        (**self).haptics()
    }
    fn leds(&self) -> Option<&dyn Leds> {
        (**self).leds()
    }
    fn storage(&self) -> Option<&dyn Storage> {
        (**self).storage()
    }
    fn tpm(&self) -> Option<&dyn Tpm> {
        (**self).tpm()
    }
    fn provides(&self, class: DeviceClass) -> bool {
        (**self).provides(class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::msg::{self, Lang};

    struct Bare;

    impl Backend for Bare {
        fn name(&self) -> &str {
            "bare"
        }
        fn probe(&self) -> bool {
            true
        }
    }

    #[test]
    fn class_names_round_trip() {
        for (index, class) in DeviceClass::ALL.iter().enumerate() {
            assert_eq!(class.index(), index);
            assert_eq!(DeviceClass::parse(class.as_str()), Some(*class));
            assert_eq!(class.to_string(), class.as_str());
            for lang in Lang::ALL {
                assert!(msg::text(class.key(), lang).is_some(), "{class}");
            }
        }
        assert_eq!(DeviceClass::parse(" WiFi "), Some(DeviceClass::Wifi));
        assert_eq!(DeviceClass::parse("printer"), None);
    }

    #[test]
    fn default_backend_provides_nothing() {
        let bare = Bare;
        assert!(bare.probe());
        assert!(bare.system().is_empty());
        for class in DeviceClass::ALL {
            assert!(!bare.provides(class), "{class}");
        }
        let object: &dyn Backend = &bare;
        assert_eq!(object.name(), "bare");
        assert!(object.display().is_none() && object.tpm().is_none());
        let shared = std::sync::Arc::new(Bare);
        assert_eq!(shared.name(), "bare");
        assert!(shared.probe() && !shared.provides(DeviceClass::Input));
    }
}
