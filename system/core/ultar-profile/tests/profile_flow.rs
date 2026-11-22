//! End-to-end tests of the public API: detect a device, store the profile, derive
//! the mode and the visible panels – the way devd, the platform and settings use it.

use ultar_profile::{
    choose_mode, detect, device_id, requirements_met, visible_panels, Capabilities, Capability,
    CapabilitySet, CpuTopology, DeviceProfile, DisplayGeometry, Edition, FormFactor, MemorySource,
    Mode, ModePreference, PanelId, ProfileError, ProfileSource, SimdLevel,
};

fn phone_capabilities() -> Capabilities {
    Capabilities {
        flags: CapabilitySet::of(&[
            Capability::Touch,
            Capability::Battery,
            Capability::Modem,
            Capability::Gnss,
            Capability::Camera,
            Capability::Sensors,
            Capability::Wifi,
            Capability::Bluetooth,
            Capability::AudioOut,
            Capability::AudioIn,
            Capability::Fingerprint,
            Capability::Haptics,
        ]),
        display_count: 1,
        memory_mb: 3712,
        cpu: CpuTopology {
            performance_cores: 2,
            efficiency_cores: 6,
            simd: SimdLevel::Neon,
        },
        primary_display: Some(DisplayGeometry::from_physical(1080, 2340, 68, 147)),
    }
}

fn phone_source() -> MemorySource {
    MemorySource::new()
        .with_device_tree()
        .with_identity("Samsung Galaxy A30", "4.4.177")
        .with_capabilities(phone_capabilities())
}

fn server_source() -> MemorySource {
    MemorySource::new()
        .with_chassis(23)
        .with_identity("Rack 1U", "6.8.0")
        .with_capabilities(Capabilities {
            flags: CapabilitySet::of(&[Capability::Tpm]),
            memory_mb: 65536,
            ..Capabilities::default()
        })
}

#[test]
fn phone_is_detected_from_device_tree_and_battery() {
    let profile = detect(&phone_source());
    assert_eq!(profile.form_factor, FormFactor::Phone);
    assert_eq!(profile.edition, Edition::Phone);
    assert_eq!(
        profile.device_id,
        device_id("Samsung Galaxy A30", "4.4.177")
    );
    assert_eq!(
        choose_mode(&profile, &ModePreference::default()),
        Mode::Mobile
    );
}

#[test]
fn large_handheld_is_a_tablet() {
    let mut capabilities = phone_capabilities();
    capabilities.primary_display = Some(DisplayGeometry::from_physical(1600, 2560, 136, 217));
    let profile = detect(&phone_source().with_capabilities(capabilities));
    assert_eq!(profile.form_factor, FormFactor::Tablet);
    assert_eq!(profile.edition, Edition::Phone);
}

#[test]
fn docking_a_phone_switches_to_desktop_and_back() {
    let mut source = phone_source();
    let preference = ModePreference::default();
    assert_eq!(choose_mode(&detect(&source), &preference), Mode::Mobile);

    source
        .capabilities
        .flags
        .insert(Capability::ExternalDisplay);
    source.capabilities.display_count = 2;
    assert_eq!(choose_mode(&detect(&source), &preference), Mode::Desktop);
    assert_eq!(
        choose_mode(&detect(&source), &ModePreference::no_auto_switch()),
        Mode::Mobile
    );
    assert_eq!(
        choose_mode(&detect(&source), &ModePreference::forced(Mode::Mobile)),
        Mode::Mobile
    );

    source
        .capabilities
        .flags
        .remove(Capability::ExternalDisplay);
    source.capabilities.display_count = 1;
    assert_eq!(choose_mode(&detect(&source), &preference), Mode::Mobile);
}

#[test]
fn server_is_headless_and_has_no_hardware_panels() {
    let profile = detect(&server_source());
    assert_eq!(profile.form_factor, FormFactor::Server);
    assert_eq!(profile.edition, Edition::Server);
    assert!(!profile.edition.has_ui());
    assert_eq!(
        choose_mode(&profile, &ModePreference::default()),
        Mode::Headless
    );
    let panels = visible_panels(&profile.capabilities);
    for hidden in [
        PanelId::MobileNetwork,
        PanelId::Battery,
        PanelId::Wifi,
        PanelId::Display,
        PanelId::Sound,
        PanelId::Camera,
    ] {
        assert!(!panels.contains(&hidden), "{hidden:?}");
    }
    assert!(panels.contains(&PanelId::Network) && panels.contains(&PanelId::Storage));
}

#[test]
fn phone_panels_and_app_requirements() {
    let profile = detect(&phone_source());
    let panels = visible_panels(&profile.capabilities);
    for shown in [
        PanelId::MobileNetwork,
        PanelId::Battery,
        PanelId::Wifi,
        PanelId::Display,
        PanelId::Fingerprint,
        PanelId::Gestures,
        PanelId::Keyboard,
    ] {
        assert!(panels.contains(&shown), "{shown:?}");
    }
    assert!(!panels.contains(&PanelId::Pointer));

    let dialer = CapabilitySet::of(&[Capability::Modem, Capability::AudioOut, Capability::AudioIn]);
    assert!(requirements_met(&dialer, &profile.capabilities));
    assert!(!requirements_met(
        &dialer,
        &detect(&server_source()).capabilities
    ));
    let ai_app = CapabilitySet::of(&[Capability::Npu]);
    assert!(!requirements_met(&ai_app, &profile.capabilities));
    assert_eq!(
        dialer.missing_in(detect(&server_source()).capabilities.flags),
        dialer
    );
}

#[test]
fn stored_profile_survives_and_is_bound_to_the_device() {
    let profile = detect(&phone_source());
    let stored = profile.to_toml().unwrap();
    let loaded = DeviceProfile::from_toml_for_device(&stored, &profile.device_id).unwrap();
    assert_eq!(loaded, profile);

    // Same phone after a kernel update: the stored profile no longer applies.
    let updated = detect(&phone_source().with_identity("Samsung Galaxy A30", "4.4.200"));
    let result = DeviceProfile::from_toml_for_device(&stored, &updated.device_id);
    assert!(matches!(result, Err(ProfileError::DeviceMismatch { .. })));
}

#[test]
fn laptop_with_detached_keyboard() {
    let caps = |list: &[Capability]| Capabilities {
        flags: CapabilitySet::of(list),
        display_count: 1,
        ..Capabilities::default()
    };
    let source = MemorySource::new().with_chassis(32);
    let attached = source.clone().with_capabilities(caps(&[
        Capability::Touch,
        Capability::Keyboard,
        Capability::Pointer,
    ]));
    let detached = source.with_capabilities(caps(&[Capability::Touch]));
    let preference = ModePreference::default();
    assert_eq!(detect(&attached).form_factor, FormFactor::Laptop);
    assert_eq!(choose_mode(&detect(&attached), &preference), Mode::Desktop);
    assert_eq!(choose_mode(&detect(&detached), &preference), Mode::Mobile);
}

/// A source written outside the crate works through the trait object.
struct FixedSource;

impl ProfileSource for FixedSource {
    fn kernel_cmdline(&self) -> Option<String> {
        Some("ro ultar.edition=desktop".into())
    }
    fn edition_env(&self) -> Option<String> {
        Some("server".into())
    }
    fn edition_file(&self) -> Option<String> {
        None
    }
    fn kernel_device_info(&self) -> Option<String> {
        None
    }
    fn chassis_type(&self) -> Option<String> {
        Some("not a number".into())
    }
    fn has_device_tree(&self) -> bool {
        false
    }
    fn model(&self) -> Option<String> {
        None
    }
    fn kernel_id(&self) -> Option<String> {
        Some("ultar-kern 0.7".into())
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::default()
    }
}

#[test]
fn custom_source_through_trait_object() {
    let source: Box<dyn ProfileSource> = Box::new(FixedSource);
    let profile = detect(source.as_ref());
    assert_eq!(profile.edition, Edition::Desktop);
    assert_eq!(profile.form_factor, FormFactor::Desktop);
    assert_eq!(profile.device_id, "unknown | ultar-kern 0.7");
}
