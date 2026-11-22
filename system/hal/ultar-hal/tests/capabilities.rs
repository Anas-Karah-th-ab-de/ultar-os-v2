//! Deriving capabilities from backends and using them with `ultar-profile`:
//! interface mode, settings panels and app requirements follow the hardware.

use std::sync::Arc;
use ultar_hal::mock::MockBackend;
use ultar_hal::{
    capabilities_of, Backend, BackendSet, DeviceClass, DisplayMode, HalError, InputDeviceKind,
    MemoryChoiceStore, ModemState, Size,
};
use ultar_profile::{
    choose_mode, detect, requirements_met, visible_panels, Capability, CapabilitySet, FormFactor,
    MemorySource, Mode, ModePreference, PanelId, SimdLevel,
};

fn flags(backend: &dyn Backend) -> CapabilitySet {
    capabilities_of(backend).flags
}

#[test]
fn phone_preset_capabilities() {
    let phone = MockBackend::phone();
    let capabilities = capabilities_of(&phone);
    let expected = CapabilitySet::all().missing_in(CapabilitySet::of(&[
        Capability::Keyboard,
        Capability::Pointer,
        Capability::ExternalDisplay,
        Capability::Tpm,
        Capability::Npu,
    ]));
    assert_eq!(capabilities.flags, expected);
    assert_eq!(capabilities.display_count, 1);
    assert_eq!(capabilities.memory_mb, 3_712);
    assert_eq!(capabilities.cpu.total_cores(), 8);
    assert_eq!(capabilities.cpu.simd, SimdLevel::Neon);
    let display = capabilities.primary_display.unwrap();
    assert_eq!((display.width_px, display.height_px), (1080, 2340));
    assert_eq!(display.dpi, 403);
}

#[test]
fn laptop_and_server_preset_capabilities() {
    let laptop = flags(&MockBackend::laptop());
    let laptop_expected = [
        Capability::Keyboard,
        Capability::Pointer,
        Capability::Battery,
        Capability::Camera,
        Capability::Tpm,
        Capability::Gpu,
        Capability::Wifi,
        Capability::Bluetooth,
        Capability::AudioOut,
        Capability::AudioIn,
    ];
    assert_eq!(laptop, CapabilitySet::of(&laptop_expected));

    let server = capabilities_of(&MockBackend::server());
    assert_eq!(server.flags, CapabilitySet::of(&[Capability::Tpm]));
    assert_eq!(server.display_count, 0);
    assert_eq!(server.primary_display, None);
    assert_eq!(server.memory_mb, 65_536);

    let nothing = capabilities_of(&MockBackend::empty("nothing"));
    assert_eq!(nothing, Default::default());
}

#[test]
fn a_class_counts_only_if_it_is_usable() {
    // Every class is provided, but the devices behind them are empty.
    let hollow = MockBackend::full("hollow");
    hollow.links.radio.set_modem_state(ModemState::Absent);
    let capabilities = capabilities_of(&hollow);
    let expected = [
        Capability::Wifi,
        Capability::Bluetooth,
        Capability::Gnss,
        Capability::Haptics,
        Capability::Tpm,
    ];
    assert_eq!(capabilities.flags, CapabilitySet::of(&expected));
    assert_eq!(capabilities.display_count, 0);

    let phone = MockBackend::phone();
    phone.base.power.set_battery(None);
    phone.links.radio.set_modem_state(ModemState::Absent);
    phone.ui.renderer.set_gpu(false);
    let reduced = flags(&phone);
    for gone in [Capability::Battery, Capability::Modem, Capability::Gpu] {
        assert!(!reduced.contains(gone), "{gone}");
    }
    // A modem that is merely switched off is still a modem.
    phone.links.radio.set_modem_state(ModemState::Off);
    assert!(flags(&phone).contains(Capability::Modem));
}

#[test]
fn failing_classes_count_as_absent() {
    let phone = MockBackend::phone();
    phone.ui.display.fail_next(HalError::Io(5));
    phone.ui.input.fail_next(HalError::Disconnected);
    phone.base.power.fail_next(HalError::Timeout);
    phone.links.radio.fail_next(HalError::Timeout);
    phone.ui.camera.fail_next(HalError::Busy);
    phone.base.sensors.fail_next(HalError::Busy);
    phone.ui.leds.fail_next(HalError::Busy);
    let capabilities = capabilities_of(&phone);
    assert_eq!(capabilities.display_count, 0);
    assert_eq!(capabilities.primary_display, None);
    for gone in [
        Capability::Touch,
        Capability::Battery,
        Capability::Modem,
        Capability::Camera,
        Capability::Sensors,
        Capability::Leds,
    ] {
        assert!(!capabilities.has(gone), "{gone}");
    }
    // The failures were one-shot: the next look sees the full phone again.
    assert!(capabilities_of(&phone).has(Capability::Touch));
    assert_eq!(capabilities_of(&phone).display_count, 1);
}

#[test]
fn input_devices_map_to_capabilities() {
    let backend = MockBackend::full("inputs");
    let input = &backend.ui.input;
    let cases = [
        (InputDeviceKind::Buttons, None),
        (InputDeviceKind::Stylus, None),
        (InputDeviceKind::Touchscreen, Some(Capability::Touch)),
        (InputDeviceKind::Keyboard, Some(Capability::Keyboard)),
        (InputDeviceKind::Touchpad, Some(Capability::Pointer)),
    ];
    let mut expected = flags(&backend);
    for (kind, capability) in cases {
        input.add_device(kind, "device", false);
        if let Some(capability) = capability {
            expected.insert(capability);
        }
        assert_eq!(flags(&backend), expected, "{kind:?}");
    }
    let mouse_only = MockBackend::with_classes("mouse", &[DeviceClass::Input]);
    mouse_only
        .ui
        .input
        .add_device(InputDeviceKind::Mouse, "mouse", false);
    assert_eq!(
        flags(&mouse_only),
        CapabilitySet::of(&[Capability::Pointer])
    );
}

#[test]
fn docking_a_phone_changes_mode_through_real_hotplug() {
    let phone = Arc::new(MockBackend::phone());
    let mut store = MemoryChoiceStore::new();
    let set = BackendSet::select(vec![Box::new(phone.clone())], &mut store);
    let profile_now = || {
        let source = MemorySource::new()
            .with_device_tree()
            .with_identity("Mock Phone", "6.1")
            .with_capabilities(capabilities_of(&set));
        detect(&source)
    };
    let preference = ModePreference::default();
    let profile = profile_now();
    assert_eq!(profile.form_factor, FormFactor::Phone);
    assert_eq!(choose_mode(&profile, &preference), Mode::Mobile);

    let monitor = phone.ui.display.plug(
        "DP-1",
        false,
        Size::new(597, 336),
        &[DisplayMode::new(2560, 1440, 60)],
    );
    let docked = profile_now();
    assert!(docked.capabilities.has(Capability::ExternalDisplay));
    assert_eq!(docked.capabilities.display_count, 2);
    // The built-in panel stays the primary display.
    assert_eq!(docked.capabilities.primary_display.unwrap().width_px, 1080);
    assert_eq!(choose_mode(&docked, &preference), Mode::Desktop);

    phone.ui.display.unplug(monitor).unwrap();
    assert_eq!(choose_mode(&profile_now(), &preference), Mode::Mobile);

    // Keyboard and mouse alone also make a desk.
    let input = &phone.ui.input;
    let keyboard = input.add_device(InputDeviceKind::Keyboard, "bt-keyboard", false);
    assert_eq!(choose_mode(&profile_now(), &preference), Mode::Mobile);
    input.add_device(InputDeviceKind::Mouse, "bt-mouse", false);
    assert_eq!(choose_mode(&profile_now(), &preference), Mode::Desktop);
    input.remove_device(keyboard).unwrap();
    assert_eq!(choose_mode(&profile_now(), &preference), Mode::Mobile);
}

#[test]
fn panels_and_requirements_follow_the_backend() {
    let phone = capabilities_of(&MockBackend::phone());
    let server = capabilities_of(&MockBackend::server());
    let phone_panels = visible_panels(&phone);
    let server_panels = visible_panels(&server);
    for panel in [
        PanelId::MobileNetwork,
        PanelId::Battery,
        PanelId::Wifi,
        PanelId::Display,
        PanelId::Fingerprint,
    ] {
        assert!(phone_panels.contains(&panel), "{panel:?}");
        assert!(!server_panels.contains(&panel), "{panel:?}");
    }
    let dialer = CapabilitySet::of(&[Capability::Modem, Capability::AudioOut]);
    assert!(requirements_met(&dialer, &phone));
    assert!(!requirements_met(&dialer, &server));
    assert!(!requirements_met(
        &dialer,
        &capabilities_of(&MockBackend::laptop())
    ));
}
