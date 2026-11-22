//! Deriving the profile capabilities from what a backend actually offers.

use crate::backend::Backend;
use crate::display::RendererKind;
use crate::input::InputDeviceKind;
use crate::radio::ModemState;
use ultar_profile::{Capabilities, Capability, CapabilitySet};

fn display_capabilities(backend: &dyn Backend, capabilities: &mut Capabilities) {
    let Some(display) = backend.display() else {
        return;
    };
    let displays = display.displays().unwrap_or_default();
    capabilities.display_count = u8::try_from(displays.len()).unwrap_or(u8::MAX);
    let has_external = displays.iter().any(|info| !info.internal);
    capabilities
        .flags
        .set(Capability::ExternalDisplay, has_external);
    if let Some(primary) = displays.first() {
        let mode = display
            .current_mode(primary.id)
            .unwrap_or(primary.preferred_mode);
        capabilities.primary_display = Some(primary.geometry(mode));
    }
}

fn input_capabilities(backend: &dyn Backend, flags: &mut CapabilitySet) {
    let devices = backend
        .input()
        .and_then(|input| input.devices().ok())
        .unwrap_or_default();
    for device in devices {
        match device.kind {
            InputDeviceKind::Touchscreen => flags.insert(Capability::Touch),
            InputDeviceKind::Keyboard => flags.insert(Capability::Keyboard),
            InputDeviceKind::Mouse | InputDeviceKind::Touchpad => flags.insert(Capability::Pointer),
            InputDeviceKind::Buttons | InputDeviceKind::Stylus => {}
        }
    }
}

fn media_capabilities(backend: &dyn Backend, flags: &mut CapabilitySet) {
    if let Some(audio) = backend.audio() {
        let has_output = audio.outputs().is_ok_and(|list| !list.is_empty());
        let has_input = audio.inputs().is_ok_and(|list| !list.is_empty());
        flags.set(Capability::AudioOut, has_output);
        flags.set(Capability::AudioIn, has_input);
    }
    let has_camera = backend
        .camera()
        .is_some_and(|camera| camera.list().is_ok_and(|list| !list.is_empty()));
    flags.set(Capability::Camera, has_camera);
    let has_sensors = backend
        .sensors()
        .is_some_and(|sensors| sensors.list(None).is_ok_and(|list| !list.is_empty()));
    flags.set(Capability::Sensors, has_sensors);
    let has_leds = backend
        .leds()
        .is_some_and(|leds| leds.list().is_ok_and(|list| !list.is_empty()));
    flags.set(Capability::Leds, has_leds);
}

/// Derives the capabilities of the device from a backend (usually a
/// [`crate::BackendSet`]).
///
/// A class counts only if it is really usable: an input class without a touch screen
/// gives no touch, a power class without a battery gives no battery, a radio class
/// whose modem is absent gives no modem. Classes that answer with an error count as
/// not present. Capabilities without a HAL trait (neural processor, fingerprint
/// reader) come from [`Backend::system`].
pub fn capabilities_of(backend: &dyn Backend) -> Capabilities {
    let system = backend.system();
    let mut capabilities = Capabilities {
        flags: system.extra,
        memory_mb: system.memory_mb,
        cpu: system.cpu,
        ..Capabilities::default()
    };
    display_capabilities(backend, &mut capabilities);
    let flags = &mut capabilities.flags;
    input_capabilities(backend, flags);
    media_capabilities(backend, flags);
    let has_battery = backend
        .power()
        .is_some_and(|power| matches!(power.battery(), Ok(Some(_))));
    flags.set(Capability::Battery, has_battery);
    let has_modem = backend.radio().is_some_and(|radio| {
        radio
            .modem_state()
            .is_ok_and(|state| state != ModemState::Absent)
    });
    flags.set(Capability::Modem, has_modem);
    let has_gpu = backend
        .renderer()
        .is_some_and(|renderer| renderer.kind() == RendererKind::Gpu);
    flags.set(Capability::Gpu, has_gpu);
    flags.set(Capability::Wifi, backend.wifi().is_some());
    flags.set(Capability::Bluetooth, backend.bluetooth().is_some());
    flags.set(Capability::Gnss, backend.gnss().is_some());
    flags.set(Capability::Haptics, backend.haptics().is_some());
    flags.set(Capability::Tpm, backend.tpm().is_some());
    capabilities
}
