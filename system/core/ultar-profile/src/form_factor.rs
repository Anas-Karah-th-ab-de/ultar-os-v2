//! Form factor: the physical kind of device, and how it is detected.

use crate::edition::Edition;
use crate::msg::{self, Lang};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Smallest display diagonal (millimetres, 7 inches) at which a handheld counts as a tablet.
pub const TABLET_MIN_DIAGONAL_MM: u32 = 178;

/// Physical kind of the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FormFactor {
    /// Handheld phone.
    Phone,
    /// Tablet.
    Tablet,
    /// Laptop, notebook, convertible.
    Laptop,
    /// Desktop computer, mini PC, all-in-one.
    Desktop,
    /// Rack, blade or tower server.
    Server,
    /// Single-board computer or embedded board without battery.
    Board,
}

impl FormFactor {
    /// Every form factor.
    pub const ALL: [FormFactor; 6] = [
        FormFactor::Phone,
        FormFactor::Tablet,
        FormFactor::Laptop,
        FormFactor::Desktop,
        FormFactor::Server,
        FormFactor::Board,
    ];

    /// Stable name used in the stored profile.
    pub const fn as_str(self) -> &'static str {
        match self {
            FormFactor::Phone => "phone",
            FormFactor::Tablet => "tablet",
            FormFactor::Laptop => "laptop",
            FormFactor::Desktop => "desktop",
            FormFactor::Server => "server",
            FormFactor::Board => "board",
        }
    }

    /// Parses a stable name (case-insensitive).
    pub fn parse(name: &str) -> Option<FormFactor> {
        let wanted = name.trim().to_ascii_lowercase();
        FormFactor::ALL
            .iter()
            .copied()
            .find(|form_factor| form_factor.as_str() == wanted)
    }

    /// Stable message key of the display name.
    pub const fn key(self) -> &'static str {
        match self {
            FormFactor::Phone => "profile.form_factor.phone",
            FormFactor::Tablet => "profile.form_factor.tablet",
            FormFactor::Laptop => "profile.form_factor.laptop",
            FormFactor::Desktop => "profile.form_factor.desktop",
            FormFactor::Server => "profile.form_factor.server",
            FormFactor::Board => "profile.form_factor.board",
        }
    }

    /// Display name in the given language.
    pub fn label(self, lang: Lang) -> &'static str {
        msg::text(self.key(), lang).unwrap_or(self.as_str())
    }

    /// Edition preset that fits this form factor. A board gives no hint: it may be
    /// anything from a kiosk to a headless server, so the configured edition decides.
    pub const fn edition_hint(self) -> Option<Edition> {
        match self {
            FormFactor::Phone | FormFactor::Tablet => Some(Edition::Phone),
            FormFactor::Laptop | FormFactor::Desktop => Some(Edition::Desktop),
            FormFactor::Server => Some(Edition::Server),
            FormFactor::Board => None,
        }
    }

    /// Form factor assumed when nothing could be detected and only the edition is known.
    pub const fn from_edition(edition: Edition) -> FormFactor {
        match edition {
            Edition::Phone => FormFactor::Phone,
            Edition::Desktop => FormFactor::Desktop,
            Edition::Server => FormFactor::Server,
        }
    }

    /// True for devices that are carried in the hand.
    pub const fn is_handheld(self) -> bool {
        matches!(self, FormFactor::Phone | FormFactor::Tablet)
    }
}

impl fmt::Display for FormFactor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Maps an SMBIOS chassis type (DMTF SMBIOS type 3; the lock bit 0x80 is ignored)
/// to a form factor. `None` means the value says nothing (other, unknown, peripheral …).
pub fn form_factor_from_chassis(chassis_type: u8) -> Option<FormFactor> {
    match chassis_type & 0x7f {
        3..=7 | 13 | 15 | 16 | 24 | 34 | 35 | 36 => Some(FormFactor::Desktop),
        8 | 9 | 10 | 14 | 31 | 32 => Some(FormFactor::Laptop),
        17 | 23 | 25 | 28 | 29 => Some(FormFactor::Server),
        30 => Some(FormFactor::Tablet),
        11 => Some(FormFactor::Phone),
        _ => None,
    }
}

/// Raw facts used to detect the form factor. All of them are optional.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DetectInputs<'a> {
    /// Device description of the Ultar kernel: lines of `key=value`
    /// (`form_factor=` or its old spelling `art=`, `edition=` or its old spelling `ausgabe=`).
    pub kernel_info: Option<&'a str>,
    /// SMBIOS chassis type as decimal text, as firmware tables expose it.
    pub chassis: Option<&'a str>,
    /// A device tree is present (ARM boards and phones).
    pub device_tree: bool,
    /// A battery is present.
    pub battery: bool,
    /// Diagonal of the built-in display in millimetres, if known.
    pub display_diagonal_mm: Option<u32>,
}

fn kernel_value<'a>(info: &'a str, keys: &[&str]) -> Option<&'a str> {
    info.lines().map(str::trim).find_map(|line| {
        keys.iter()
            .find_map(|key| line.strip_prefix(key)?.strip_prefix('='))
    })
}

/// Edition the Ultar kernel reports for the device, if it reports one.
pub fn edition_from_kernel_info(info: &str) -> Option<Edition> {
    kernel_value(info, &["edition", "ausgabe"]).and_then(Edition::parse)
}

fn form_factor_from_kernel_info(info: &str) -> Option<FormFactor> {
    kernel_value(info, &["form_factor", "art"])
        .and_then(FormFactor::parse)
        .or_else(|| edition_from_kernel_info(info).map(FormFactor::from_edition))
}

/// Detects the form factor. Order: what the Ultar kernel reports, then the SMBIOS
/// chassis type, then device tree plus battery (phone, or tablet from 7 inches),
/// then device tree alone (board). `None` if nothing is conclusive.
pub fn detect_form_factor(inputs: &DetectInputs<'_>) -> Option<FormFactor> {
    if let Some(form_factor) = inputs.kernel_info.and_then(form_factor_from_kernel_info) {
        return Some(form_factor);
    }
    let from_chassis = inputs
        .chassis
        .and_then(|text| text.trim().parse::<u8>().ok())
        .and_then(form_factor_from_chassis);
    if let Some(form_factor) = from_chassis {
        return Some(form_factor);
    }
    if !inputs.device_tree {
        return None;
    }
    if !inputs.battery {
        return Some(FormFactor::Board);
    }
    let is_tablet = inputs
        .display_diagonal_mm
        .is_some_and(|diagonal| diagonal >= TABLET_MIN_DIAGONAL_MM);
    Some(if is_tablet {
        FormFactor::Tablet
    } else {
        FormFactor::Phone
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chassis(text: &str, battery: bool) -> DetectInputs<'_> {
        DetectInputs {
            chassis: Some(text),
            battery,
            ..DetectInputs::default()
        }
    }

    #[test]
    fn chassis_table() {
        assert_eq!(form_factor_from_chassis(3), Some(FormFactor::Desktop));
        assert_eq!(form_factor_from_chassis(35), Some(FormFactor::Desktop));
        assert_eq!(form_factor_from_chassis(9), Some(FormFactor::Laptop));
        assert_eq!(form_factor_from_chassis(31), Some(FormFactor::Laptop));
        assert_eq!(form_factor_from_chassis(23), Some(FormFactor::Server));
        assert_eq!(form_factor_from_chassis(30), Some(FormFactor::Tablet));
        assert_eq!(form_factor_from_chassis(11), Some(FormFactor::Phone));
        assert_eq!(form_factor_from_chassis(0), None);
        assert_eq!(form_factor_from_chassis(1), None);
        assert_eq!(form_factor_from_chassis(2), None);
        assert_eq!(form_factor_from_chassis(127), None);
        // The lock bit does not change the type.
        assert_eq!(
            form_factor_from_chassis(0x80 | 10),
            Some(FormFactor::Laptop)
        );
    }

    // Carries over the detection test of the old `ultar-edition` (via the edition hint).
    #[test]
    fn old_detection_cases() {
        let hint = |inputs: DetectInputs<'_>| {
            detect_form_factor(&inputs).and_then(FormFactor::edition_hint)
        };
        // Beelink GTi15 Ultra: mini PC.
        assert_eq!(hint(chassis("35\n", false)), Some(Edition::Desktop));
        assert_eq!(hint(chassis("10", true)), Some(Edition::Desktop));
        assert_eq!(hint(chassis("23", false)), Some(Edition::Server));
        assert_eq!(hint(chassis("30", true)), Some(Edition::Phone));
        assert_eq!(hint(chassis("2", false)), None);
        // Phone (Samsung A30): device tree plus battery.
        let phone = DetectInputs {
            device_tree: true,
            battery: true,
            ..DetectInputs::default()
        };
        assert_eq!(hint(phone), Some(Edition::Phone));
        // Device tree without battery gives no edition hint (the default stays in force).
        let board = DetectInputs {
            device_tree: true,
            ..DetectInputs::default()
        };
        assert_eq!(hint(board), None);
        // The Ultar kernel reports the device itself and wins over the chassis type.
        let kernel = DetectInputs {
            kernel_info: Some("art=server\nausgabe=server\n"),
            chassis: Some("35"),
            ..DetectInputs::default()
        };
        assert_eq!(hint(kernel), Some(Edition::Server));
    }

    #[test]
    fn device_tree_cases() {
        let mut inputs = DetectInputs {
            device_tree: true,
            battery: true,
            display_diagonal_mm: Some(162),
            ..DetectInputs::default()
        };
        assert_eq!(detect_form_factor(&inputs), Some(FormFactor::Phone));
        inputs.display_diagonal_mm = Some(TABLET_MIN_DIAGONAL_MM);
        assert_eq!(detect_form_factor(&inputs), Some(FormFactor::Tablet));
        inputs.battery = false;
        assert_eq!(detect_form_factor(&inputs), Some(FormFactor::Board));
        assert_eq!(detect_form_factor(&DetectInputs::default()), None);
        // A battery alone (no device tree, no chassis) is not conclusive.
        let battery_only = DetectInputs {
            battery: true,
            ..DetectInputs::default()
        };
        assert_eq!(detect_form_factor(&battery_only), None);
    }

    #[test]
    fn kernel_info_parsing() {
        let detect = |info| {
            detect_form_factor(&DetectInputs {
                kernel_info: Some(info),
                ..DetectInputs::default()
            })
        };
        assert_eq!(detect("form_factor=tablet\n"), Some(FormFactor::Tablet));
        assert_eq!(detect("  art=laptop  "), Some(FormFactor::Laptop));
        assert_eq!(detect("edition=desktop"), Some(FormFactor::Desktop));
        // An unknown kind falls back to the reported edition.
        assert_eq!(detect("art=pc\nausgabe=desktop"), Some(FormFactor::Desktop));
        assert_eq!(detect("artikel=server"), None);
        assert_eq!(detect(""), None);
        assert_eq!(
            edition_from_kernel_info("x=1\nedition=server"),
            Some(Edition::Server)
        );
        assert_eq!(edition_from_kernel_info("edition=none"), None);
    }

    #[test]
    fn invalid_chassis_text_is_ignored() {
        assert_eq!(detect_form_factor(&chassis("abc", false)), None);
        assert_eq!(detect_form_factor(&chassis("999", false)), None);
        assert_eq!(detect_form_factor(&chassis("", true)), None);
    }

    #[test]
    fn names_hints_and_labels() {
        for form_factor in FormFactor::ALL {
            assert_eq!(FormFactor::parse(form_factor.as_str()), Some(form_factor));
            assert_eq!(form_factor.to_string(), form_factor.as_str());
            assert_ne!(form_factor.label(Lang::Ar), form_factor.as_str());
        }
        assert_eq!(FormFactor::parse("fridge"), None);
        assert!(FormFactor::Tablet.is_handheld() && !FormFactor::Laptop.is_handheld());
        for edition in Edition::ALL {
            assert_eq!(
                FormFactor::from_edition(edition).edition_hint(),
                Some(edition)
            );
        }
    }
}
