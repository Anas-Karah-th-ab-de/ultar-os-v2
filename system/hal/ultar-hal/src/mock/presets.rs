//! Ready-made mock devices: a phone, a laptop and a server.

use super::{MockBackend, MockPower};
use crate::audio::{AudioDirection, AudioEndpointKind};
use crate::backend::{DeviceClass, SystemSummary};
use crate::camera::{CameraFacing, CameraFormat};
use crate::connectivity::InterfaceKind;
use crate::display::DisplayMode;
use crate::input::InputDeviceKind;
use crate::types::{PixelFormat, Size};
use ultar_profile::{Capability, CapabilitySet, CpuTopology, SimdLevel};

const CAMERA_FORMATS: [CameraFormat; 2] = [
    CameraFormat {
        size: Size::new(640, 480),
        format: PixelFormat::Yuyv,
        frames_per_second: 30,
    },
    CameraFormat {
        size: Size::new(1920, 1080),
        format: PixelFormat::Mjpeg,
        frames_per_second: 30,
    },
];

impl MockBackend {
    /// A phone: built-in touch display, battery, modem with SIM, Wi-Fi, Bluetooth,
    /// two cameras, sensors, satellite receiver, vibration, light, fingerprint reader.
    /// Provides every class except the TPM.
    pub fn phone() -> Self {
        let classes: Vec<DeviceClass> = DeviceClass::ALL
            .into_iter()
            .filter(|class| *class != DeviceClass::Tpm)
            .collect();
        let phone = MockBackend::with_classes("mock-phone", &classes);
        phone.set_system(SystemSummary {
            cpu: CpuTopology {
                performance_cores: 2,
                efficiency_cores: 6,
                simd: SimdLevel::Neon,
            },
            memory_mb: 3_712,
            extra: CapabilitySet::of(&[Capability::Fingerprint]),
        });
        phone.ui.display.plug(
            "DSI-1",
            true,
            Size::new(68, 147),
            &[DisplayMode::new(1080, 2340, 60)],
        );
        phone.ui.renderer.set_gpu(true);
        let input = &phone.ui.input;
        input.add_device(InputDeviceKind::Touchscreen, "mock-touch", true);
        input.add_device(InputDeviceKind::Buttons, "mock-keys", true);
        phone.add_phone_media();
        phone
            .base
            .power
            .set_battery(Some(MockPower::discharging_battery(80)));
        phone.base.power.set_zone("battery", 28_500, Some(60_000));
        phone.base.storage.add_device("mmcblk0", 512, 2_048, false);
        let links = &phone.links;
        links.network.add_interface("lo", InterfaceKind::Loopback);
        links.network.add_interface("wlan0", InterfaceKind::Wifi);
        links
            .network
            .add_interface("rmnet0", InterfaceKind::Cellular);
        phone
    }

    fn add_phone_media(&self) {
        use crate::sensors::SensorKind;
        let audio = &self.ui.audio;
        audio.add_endpoint(AudioEndpointKind::Speaker, AudioDirection::Output);
        audio.add_endpoint(AudioEndpointKind::Earpiece, AudioDirection::Output);
        audio.add_endpoint(AudioEndpointKind::Microphone, AudioDirection::Input);
        let camera = &self.ui.camera;
        camera.add_camera(CameraFacing::Back, true, &CAMERA_FORMATS);
        camera.add_camera(CameraFacing::Front, false, &CAMERA_FORMATS[..1]);
        self.ui.leds.add_led("notification", true);
        let sensors = &self.base.sensors;
        sensors.add_sensor(SensorKind::Accelerometer, "mock-accel", [0.0, 9.81, 0.0]);
        sensors.add_sensor(SensorKind::Gyroscope, "mock-gyro", [0.0; 3]);
        sensors.add_sensor(SensorKind::Light, "mock-light", [320.0, 0.0, 0.0]);
        sensors.add_sensor(SensorKind::Proximity, "mock-proximity", [5.0, 0.0, 0.0]);
    }

    /// A laptop: built-in display, keyboard, touchpad, battery, Ethernet, Wi-Fi,
    /// Bluetooth, webcam, storage and TPM. No modem, no sensors, no satellite
    /// receiver, no vibration, no lights.
    pub fn laptop() -> Self {
        use DeviceClass::{
            Audio, Bluetooth, Camera, Display, Input, Network, Power, Renderer, Storage, Tpm, Wifi,
        };
        let laptop = MockBackend::with_classes(
            "mock-laptop",
            &[
                Display, Renderer, Input, Audio, Power, Network, Wifi, Bluetooth, Camera, Storage,
                Tpm,
            ],
        );
        laptop.set_system(SystemSummary {
            cpu: CpuTopology {
                performance_cores: 4,
                efficiency_cores: 8,
                simd: SimdLevel::Avx2,
            },
            memory_mb: 16_384,
            extra: CapabilitySet::EMPTY,
        });
        let modes = [
            DisplayMode::new(1920, 1080, 60),
            DisplayMode::new(1280, 720, 60),
        ];
        let panel = Size::new(309, 174);
        laptop.ui.display.plug("eDP-1", true, panel, &modes);
        laptop.ui.renderer.set_gpu(true);
        let input = &laptop.ui.input;
        input.add_device(InputDeviceKind::Keyboard, "mock-keyboard", true);
        input.add_device(InputDeviceKind::Touchpad, "mock-touchpad", true);
        let audio = &laptop.ui.audio;
        audio.add_endpoint(AudioEndpointKind::Speaker, AudioDirection::Output);
        audio.add_endpoint(AudioEndpointKind::Microphone, AudioDirection::Input);
        let camera = &laptop.ui.camera;
        camera.add_camera(CameraFacing::Front, false, &CAMERA_FORMATS);
        laptop
            .base
            .power
            .set_battery(Some(MockPower::discharging_battery(65)));
        laptop
            .base
            .storage
            .add_device("nvme0n1", 4_096, 1_024, false);
        let network = &laptop.links.network;
        network.add_interface("lo", InterfaceKind::Loopback);
        network.add_interface("eth0", InterfaceKind::Ethernet);
        network.add_interface("wlan0", InterfaceKind::Wifi);
        laptop
    }

    /// A server: power (no battery), Ethernet, two disks and a TPM. Nothing else.
    pub fn server() -> Self {
        use DeviceClass::{Network, Power, Storage, Tpm};
        let server = MockBackend::with_classes("mock-server", &[Power, Network, Storage, Tpm]);
        server.set_system(SystemSummary {
            cpu: CpuTopology {
                performance_cores: 16,
                efficiency_cores: 0,
                simd: SimdLevel::Avx512,
            },
            memory_mb: 65_536,
            extra: CapabilitySet::EMPTY,
        });
        server.base.power.set_zone("cpu", 41_000, Some(100_000));
        server.base.storage.add_device("sda", 512, 4_096, false);
        server.base.storage.add_device("sdb", 512, 4_096, true);
        let network = &server.links.network;
        network.add_interface("lo", InterfaceKind::Loopback);
        network.add_interface("eth0", InterfaceKind::Ethernet);
        server
    }
}
