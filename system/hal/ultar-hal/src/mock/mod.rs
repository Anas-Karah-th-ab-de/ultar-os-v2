//! Complete in-memory backend for developing and testing without hardware.
//!
//! [`MockBackend`] implements every device class. It is deterministic (fixed clocks,
//! fixed "random" numbers) and scriptable: tests inject input events, plug and unplug
//! displays, change battery or modem state and arm an error for the next call of any
//! class (`fail_next`). The presets [`MockBackend::phone`], [`MockBackend::laptop`] and
//! [`MockBackend::server`] describe typical devices.
//!
//! Wrap the backend in an [`std::sync::Arc`] to keep a scripting handle while a
//! [`crate::BackendSet`] or a service owns the other reference.

mod audio;
mod cell;
mod display;
mod input;
mod media;
mod misc;
mod net;
mod power;
mod presets;
mod radio;

pub use audio::MockAudio;
pub use display::{MockDisplay, MockRenderer};
pub use input::MockInput;
pub use media::{MockCamera, MockSensors};
pub use misc::{MockHaptics, MockLeds, MockStorage, MockTpm};
pub use net::{MockBluetooth, MockNetwork, MockWifi};
pub use power::{MockPower, PowerAction};
pub use radio::{MockGnss, MockRadio};

use crate::backend::{Backend, DeviceClass, SystemSummary};
use crate::{
    Audio, Bluetooth, Camera, Display, Gnss, Haptics, Input, Leds, Network, Power, Radio, Renderer,
    Sensors, Storage, Tpm, Wifi,
};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

/// Mock devices a person sees, hears or touches.
#[derive(Debug, Default)]
pub struct MockUi {
    /// Display outputs.
    pub display: MockDisplay,
    /// Renderer description.
    pub renderer: MockRenderer,
    /// Input devices.
    pub input: MockInput,
    /// Sound hardware.
    pub audio: MockAudio,
    /// Cameras.
    pub camera: MockCamera,
    /// Vibration motor.
    pub haptics: MockHaptics,
    /// Indicator lights.
    pub leds: MockLeds,
}

/// Mock devices that connect the device to the outside.
#[derive(Debug, Default)]
pub struct MockLinks {
    /// Network interfaces.
    pub network: MockNetwork,
    /// Wi-Fi adapter.
    pub wifi: MockWifi,
    /// Bluetooth adapter.
    pub bluetooth: MockBluetooth,
    /// Cellular modem.
    pub radio: MockRadio,
    /// Satellite receiver.
    pub gnss: MockGnss,
}

/// Mock devices of the machine itself.
#[derive(Debug, Default)]
pub struct MockBase {
    /// Power hardware.
    pub power: MockPower,
    /// Sensors.
    pub sensors: MockSensors,
    /// Block storage.
    pub storage: MockStorage,
    /// Trusted platform module.
    pub tpm: MockTpm,
}

#[derive(Debug)]
struct Control {
    probe_result: AtomicBool,
    probe_count: AtomicUsize,
    classes: AtomicU32,
    system: Mutex<SystemSummary>,
}

/// Scriptable backend that implements every device class in memory.
#[derive(Debug)]
pub struct MockBackend {
    name: String,
    control: Control,
    /// Display, renderer, input, audio, camera, haptics and lights.
    pub ui: MockUi,
    /// Network, Wi-Fi, Bluetooth, modem and satellite receiver.
    pub links: MockLinks,
    /// Power, sensors, storage and TPM.
    pub base: MockBase,
}

const fn class_bit(class: DeviceClass) -> u32 {
    1 << class.index()
}

impl MockBackend {
    /// Backend that probes successfully and provides the given classes. The devices
    /// behind the classes start empty (no display, no input device …).
    pub fn with_classes(name: &str, classes: &[DeviceClass]) -> Self {
        let mask = classes
            .iter()
            .fold(0, |mask, class| mask | class_bit(*class));
        MockBackend {
            name: name.to_string(),
            control: Control {
                probe_result: AtomicBool::new(true),
                probe_count: AtomicUsize::new(0),
                classes: AtomicU32::new(mask),
                system: Mutex::new(SystemSummary::default()),
            },
            ui: MockUi::default(),
            links: MockLinks::default(),
            base: MockBase::default(),
        }
    }

    /// Backend that provides no class at all.
    pub fn empty(name: &str) -> Self {
        MockBackend::with_classes(name, &[])
    }

    /// Backend that provides every class, with empty devices.
    pub fn full(name: &str) -> Self {
        MockBackend::with_classes(name, &DeviceClass::ALL)
    }

    /// Returns the backend under another name (builder style).
    #[must_use]
    pub fn named(mut self, name: &str) -> Self {
        self.name = name.to_string();
        self
    }

    /// Sets what [`Backend::probe`] answers from now on.
    pub fn set_probe_result(&self, works: bool) {
        self.control.probe_result.store(works, Ordering::SeqCst);
    }

    /// Number of times [`Backend::probe`] was called.
    pub fn probe_count(&self) -> usize {
        self.control.probe_count.load(Ordering::SeqCst)
    }

    /// Starts or stops providing a class.
    pub fn set_class(&self, class: DeviceClass, provided: bool) {
        if provided {
            self.control
                .classes
                .fetch_or(class_bit(class), Ordering::SeqCst);
        } else {
            self.control
                .classes
                .fetch_and(!class_bit(class), Ordering::SeqCst);
        }
    }

    /// Sets what [`Backend::system`] reports.
    pub fn set_system(&self, system: SystemSummary) {
        *self
            .control
            .system
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = system;
    }

    fn has(&self, class: DeviceClass) -> bool {
        self.control.classes.load(Ordering::SeqCst) & class_bit(class) != 0
    }
}

impl Backend for MockBackend {
    fn name(&self) -> &str {
        &self.name
    }

    fn probe(&self) -> bool {
        self.control.probe_count.fetch_add(1, Ordering::SeqCst);
        self.control.probe_result.load(Ordering::SeqCst)
    }

    fn system(&self) -> SystemSummary {
        *self
            .control
            .system
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn display(&self) -> Option<&dyn Display> {
        self.has(DeviceClass::Display)
            .then_some(&self.ui.display as &dyn Display)
    }
    fn renderer(&self) -> Option<&dyn Renderer> {
        self.has(DeviceClass::Renderer)
            .then_some(&self.ui.renderer as &dyn Renderer)
    }
    fn input(&self) -> Option<&dyn Input> {
        self.has(DeviceClass::Input)
            .then_some(&self.ui.input as &dyn Input)
    }
    fn audio(&self) -> Option<&dyn Audio> {
        self.has(DeviceClass::Audio)
            .then_some(&self.ui.audio as &dyn Audio)
    }
    fn power(&self) -> Option<&dyn Power> {
        self.has(DeviceClass::Power)
            .then_some(&self.base.power as &dyn Power)
    }
    fn network(&self) -> Option<&dyn Network> {
        self.has(DeviceClass::Network)
            .then_some(&self.links.network as &dyn Network)
    }
    fn wifi(&self) -> Option<&dyn Wifi> {
        self.has(DeviceClass::Wifi)
            .then_some(&self.links.wifi as &dyn Wifi)
    }
    fn bluetooth(&self) -> Option<&dyn Bluetooth> {
        self.has(DeviceClass::Bluetooth)
            .then_some(&self.links.bluetooth as &dyn Bluetooth)
    }
    fn radio(&self) -> Option<&dyn Radio> {
        self.has(DeviceClass::Radio)
            .then_some(&self.links.radio as &dyn Radio)
    }
    fn camera(&self) -> Option<&dyn Camera> {
        self.has(DeviceClass::Camera)
            .then_some(&self.ui.camera as &dyn Camera)
    }
    fn sensors(&self) -> Option<&dyn Sensors> {
        self.has(DeviceClass::Sensors)
            .then_some(&self.base.sensors as &dyn Sensors)
    }
    fn gnss(&self) -> Option<&dyn Gnss> {
        self.has(DeviceClass::Gnss)
            .then_some(&self.links.gnss as &dyn Gnss)
    }
    fn haptics(&self) -> Option<&dyn Haptics> {
        self.has(DeviceClass::Haptics)
            .then_some(&self.ui.haptics as &dyn Haptics)
    }
    fn leds(&self) -> Option<&dyn Leds> {
        self.has(DeviceClass::Leds)
            .then_some(&self.ui.leds as &dyn Leds)
    }
    fn storage(&self) -> Option<&dyn Storage> {
        self.has(DeviceClass::Storage)
            .then_some(&self.base.storage as &dyn Storage)
    }
    fn tpm(&self) -> Option<&dyn Tpm> {
        self.has(DeviceClass::Tpm)
            .then_some(&self.base.tpm as &dyn Tpm)
    }
}
