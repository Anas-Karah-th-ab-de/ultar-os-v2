//! Conversions between the bus contracts and the service's own types.

use crate::i18n::store_notice_key;
use crate::store::Notice;
use ultar_bus::messages::devices;
use ultar_bus::messages::settings::StoreNotice;
use ultar_bus::{Text, TextArg};
use ultar_profile::{Capabilities, Capability, CapabilitySet};

/// Turns a capability report of the device service into the profile form.
/// Memory, CPU and geometry stay as they were; the display count follows
/// the `display` and `external_display` capabilities when the report
/// mentions them.
pub fn capabilities_from_bus(
    previous: &Capabilities,
    report: &devices::Capabilities,
) -> Capabilities {
    // The device service reports driver classes; keyboard, pointer and touch are not among
    // them, so what the device itself found for those stays (else the Keyboard and Mouse pages
    // vanish as soon as the first report arrives). A working input driver ("input": PS/2, USB
    // HID) stands for a keyboard and a pointer.
    let mut flags = CapabilitySet::EMPTY;
    for kept in [Capability::Keyboard, Capability::Pointer, Capability::Touch] {
        if previous.flags.contains(kept) {
            flags.insert(kept);
        }
    }
    for capability in &report.capabilities {
        if capability.availability == devices::Availability::Unavailable {
            continue;
        }
        if capability.id == "input" {
            flags.insert(Capability::Keyboard);
            flags.insert(Capability::Pointer);
        }
        if let Some(known) = Capability::parse(&capability.id) {
            flags.insert(known);
        }
    }
    let mentions_displays =
        report.get("display").is_some() || report.get("external_display").is_some();
    let display_count = if !mentions_displays {
        previous.display_count
    } else {
        u8::from(report.has("display")) + u8::from(report.has("external_display"))
    };
    Capabilities {
        flags,
        display_count,
        ..*previous
    }
}

/// The bus form of a store notice, with its message for the user.
pub fn store_notice(notice: &Notice) -> StoreNotice {
    StoreNotice {
        file: notice.file.clone(),
        kind: notice.kind,
        detail: notice.detail.clone(),
        text: Text::key(store_notice_key(notice.kind))
            .with(TextArg::Verbatim(notice.file.clone()))
            .with(TextArg::Verbatim(notice.detail.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_bus::messages::settings::StoreNoticeKind;

    fn capability(id: &str, availability: devices::Availability) -> devices::Capability {
        devices::Capability {
            id: id.into(),
            availability,
            note: None,
        }
    }

    #[test]
    fn capability_reports_become_profile_capabilities() {
        use devices::Availability::{Available, Degraded, Unavailable};
        let previous = Capabilities {
            memory_mb: 16_000,
            display_count: 1,
            ..Capabilities::default()
        };
        let report = devices::Capabilities {
            revision: 2,
            capabilities: vec![
                capability("battery", Available),
                capability("wifi", Degraded),
                capability("camera", Unavailable),
                capability("something_new", Available),
            ],
        };
        let updated = capabilities_from_bus(&previous, &report);
        assert!(updated.has(Capability::Battery));
        assert!(updated.has(Capability::Wifi));
        assert!(!updated.has(Capability::Camera));
        assert_eq!(updated.display_count, 1);
        assert_eq!(updated.memory_mb, 16_000);

        let mut with_displays = report.clone();
        with_displays
            .capabilities
            .push(capability("display", Available));
        with_displays
            .capabilities
            .push(capability("external_display", Available));
        assert_eq!(
            capabilities_from_bus(&previous, &with_displays).display_count,
            2
        );
        let headless = devices::Capabilities {
            revision: 3,
            capabilities: vec![capability("display", Unavailable)],
        };
        assert_eq!(capabilities_from_bus(&previous, &headless).display_count, 0);
    }

    #[test]
    fn store_notice_carries_a_message_with_file_and_detail() {
        let notice = store_notice(&Notice {
            file: "system.toml".into(),
            kind: StoreNoticeKind::Quarantined,
            detail: "quarantine/system.toml.0".into(),
        });
        assert_eq!(notice.text.key, "settingsd.store.quarantined");
        assert_eq!(
            notice.text.args,
            [
                TextArg::Verbatim("system.toml".into()),
                TextArg::Verbatim("quarantine/system.toml.0".into())
            ]
        );
    }

    #[test]
    fn an_input_driver_stands_for_keyboard_and_pointer_and_found_ones_stay() {
        let report = devices::Capabilities {
            capabilities: vec![capability("input", devices::Availability::Available)],
            ..Default::default()
        };
        let caps = capabilities_from_bus(&Capabilities::default(), &report);
        assert!(caps.flags.contains(Capability::Keyboard));
        assert!(caps.flags.contains(Capability::Pointer));
        let mut touch = Capabilities::default();
        touch.flags.insert(Capability::Touch);
        let caps = capabilities_from_bus(&touch, &devices::Capabilities::default());
        assert!(caps.flags.contains(Capability::Touch));
    }
}
