//! Hardware abstraction layer of Ultar OS (area 3, system core, `system/hal`).
//!
//! This crate is the only door between the system and hardware. It contains **traits
//! only** – one per device class – plus the logic that picks a backend. Services and
//! the platform talk to these traits; backends (generic Linux, Ultar kernel, device
//! specific ones) implement them in their own crates and are the only code allowed to
//! touch device nodes.
//!
//! - Device classes: [`Display`], [`Renderer`], [`Input`], [`Audio`], [`Power`],
//!   [`Network`], [`Wifi`], [`Bluetooth`], [`Radio`], [`Camera`], [`Sensors`],
//!   [`Gnss`], [`Haptics`], [`Leds`], [`Storage`], [`Tpm`]. All are object-safe,
//!   `Send + Sync` and take `&self`, so one backend can be shared between services.
//! - [`Backend`] is the registry of one backend: it hands out `Option<&dyn Trait>`
//!   per class. [`BackendSet`] chooses, per class, the first candidate that probes
//!   successfully and remembers the choice through a [`ChoiceStore`] ("try all,
//!   remember the working one").
//! - [`capabilities_of`] derives the `ultar-profile` capabilities from a backend.
//! - With the feature `mock`, [`mock::MockBackend`] is a complete, deterministic and
//!   scriptable in-memory backend.
//!
//! Errors are typed ([`HalError`]); texts for people come from [`msg`] by key.

#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used)]
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used))]
#![warn(missing_docs)]

mod audio;
mod backend;
mod backend_set;
mod camera;
mod caps;
mod connectivity;
mod display;
mod error;
mod input;
mod misc;
pub mod msg;
mod power;
mod radio;
mod sensors;
mod types;

#[cfg(feature = "mock")]
pub mod mock;

pub use audio::{
    Audio, AudioDirection, AudioEndpoint, AudioEndpointKind, AudioFormat, AudioStream, SampleFormat,
};
pub use backend::{Backend, DeviceClass, SystemSummary};
pub use backend_set::{BackendSet, ChoiceOrigin, ChoiceStore, MemoryChoiceStore, Selection};
pub use camera::{Camera, CameraFacing, CameraFormat, CameraFrame, CameraInfo, CameraStream};
pub use caps::capabilities_of;
pub use connectivity::{
    AccessPoint, Bluetooth, BluetoothDevice, InterfaceAddress, InterfaceKind, LinkState,
    NetInterface, Network, Wifi, WifiCredentials, WifiSecurity, WifiState, WifiStatus,
};
pub use display::{
    Display, DisplayEvent, DisplayInfo, DisplayMode, FrameView, Renderer, RendererKind,
};
pub use error::{HalError, HalResult};
pub use input::{
    Input, InputDeviceInfo, InputDeviceKind, InputEvent, InputEventKind, KeyState, PointerButton,
};
pub use misc::{
    BlockDeviceInfo, Haptics, LedInfo, LedState, Leds, Storage, Tpm, TpmVersion, TPM_DIGEST_LEN,
};
pub use power::{
    BatteryHealth, BatteryState, ChargeStatus, ChargerKind, ChargerState, Power, PowerProfile,
    RebootTarget, ThermalZone,
};
pub use radio::{
    AccessTechnology, ModemState, Radio, Registration, RegistrationState, SignalStrength, SimState,
};
pub use sensors::{
    Gnss, GnssFix, GnssFixKind, SatelliteInfo, SensorInfo, SensorKind, SensorReading, Sensors,
};
pub use types::{
    BlockDeviceId, CameraId, DisplayId, EndpointId, InputDeviceId, LedId, Level, PixelFormat, Rect,
    Rotation, SensorId, Size,
};
