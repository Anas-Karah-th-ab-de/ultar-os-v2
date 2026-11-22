//! The device profile and its stored form.

use crate::capabilities::Capabilities;
use crate::edition::Edition;
use crate::error::ProfileError;
use crate::form_factor::FormFactor;
use serde::{Deserialize, Serialize};

/// Model name used in the device id when the hardware reports none.
pub const UNKNOWN_MODEL: &str = "unknown";

/// Builds the stable device id from the model name and the kernel id (release string).
///
/// The id changes when the profile no longer applies: other hardware or another kernel.
/// Quotes are replaced and surrounding whitespace and NUL bytes are dropped, so the id
/// can be stored in any text format.
pub fn device_id(model: &str, kernel_id: &str) -> String {
    let clean = |text: &str| {
        text.trim_matches(|c: char| c == '\0' || c.is_whitespace())
            .replace('"', "'")
            .replace(['\n', '\r'], " ")
    };
    let model = clean(model);
    let model = if model.is_empty() {
        UNKNOWN_MODEL
    } else {
        model.as_str()
    };
    format!("{model} | {}", clean(kernel_id))
}

/// What the system knows about the device it runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceProfile {
    /// Stable id of device and kernel (see [`device_id`]).
    pub device_id: String,
    /// Physical kind of the device.
    pub form_factor: FormFactor,
    /// Build and service preset.
    pub edition: Edition,
    /// What the device can do.
    #[serde(default)]
    pub capabilities: Capabilities,
}

impl DeviceProfile {
    /// Writes the profile as TOML.
    pub fn to_toml(&self) -> Result<String, ProfileError> {
        toml::to_string(self).map_err(|error| ProfileError::Serialize(error.to_string()))
    }

    /// Reads a profile from TOML.
    pub fn from_toml(text: &str) -> Result<DeviceProfile, ProfileError> {
        toml::from_str(text).map_err(|error| ProfileError::Parse(error.message().to_string()))
    }

    /// Reads a stored profile and rejects it if it belongs to another device or kernel.
    pub fn from_toml_for_device(
        text: &str,
        expected_device_id: &str,
    ) -> Result<DeviceProfile, ProfileError> {
        let profile = DeviceProfile::from_toml(text)?;
        if profile.device_id == expected_device_id {
            Ok(profile)
        } else {
            Err(ProfileError::DeviceMismatch {
                expected: expected_device_id.to_string(),
                found: profile.device_id,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::{CpuTopology, DisplayGeometry, SimdLevel};
    use crate::capability::{Capability, CapabilitySet};

    fn sample() -> DeviceProfile {
        DeviceProfile {
            device_id: device_id("Samsung Galaxy A30", "4.4.177-ultar"),
            form_factor: FormFactor::Phone,
            edition: Edition::Phone,
            capabilities: Capabilities {
                flags: CapabilitySet::of(&[
                    Capability::Touch,
                    Capability::Battery,
                    Capability::Modem,
                    Capability::Wifi,
                ]),
                display_count: 1,
                memory_mb: 3712,
                cpu: CpuTopology {
                    performance_cores: 2,
                    efficiency_cores: 6,
                    simd: SimdLevel::Neon,
                },
                primary_display: Some(DisplayGeometry::from_physical(1080, 2340, 68, 147)),
            },
        }
    }

    #[test]
    fn device_id_rules() {
        assert_eq!(device_id("Model X", "6.1"), "Model X | 6.1");
        assert_eq!(device_id("  \0", "6.1\n"), "unknown | 6.1");
        assert_eq!(device_id("A \"B\"\0", ""), "A 'B' | ");
        assert_eq!(device_id("a\nb", "k"), "a b | k");
        // Same inputs, same id; another kernel, another id.
        assert_eq!(device_id("M", "1"), device_id("M", "1"));
        assert_ne!(device_id("M", "1"), device_id("M", "2"));
    }

    #[test]
    fn toml_round_trip() {
        let profile = sample();
        let text = profile.to_toml().unwrap();
        assert!(text.contains("form_factor = \"phone\""));
        assert!(text.contains("simd = \"neon\""));
        assert!(text.contains("[capabilities.primary_display]"));
        assert_eq!(DeviceProfile::from_toml(&text).unwrap(), profile);
    }

    #[test]
    fn round_trip_without_display() {
        let mut profile = sample();
        profile.form_factor = FormFactor::Server;
        profile.edition = Edition::Server;
        profile.capabilities.primary_display = None;
        profile.capabilities.display_count = 0;
        let text = profile.to_toml().unwrap();
        assert!(!text.contains("primary_display"));
        assert_eq!(DeviceProfile::from_toml(&text).unwrap(), profile);
    }

    #[test]
    fn minimal_text_uses_defaults() {
        let text = "device_id = \"x | y\"\nform_factor = \"board\"\nedition = \"desktop\"\n";
        let profile = DeviceProfile::from_toml(text).unwrap();
        assert_eq!(profile.form_factor, FormFactor::Board);
        assert_eq!(profile.capabilities, Capabilities::default());
    }

    #[test]
    fn parse_errors() {
        let broken = [
            "",
            "device_id = 3",
            "this is not toml",
            "device_id = \"a\"\nform_factor = \"fridge\"\nedition = \"phone\"",
            "device_id = \"a\"\nform_factor = \"phone\"\nedition = \"phone\"\n[capabilities]\nflags = [\"x\"]",
            "device_id = \"a\"\nform_factor = \"phone\"\nedition = \"phone\"\n[capabilities]\ndisplay_count = 999",
        ];
        for text in broken {
            let error = DeviceProfile::from_toml(text).unwrap_err();
            assert!(matches!(error, ProfileError::Parse(_)), "{text}");
        }
    }

    #[test]
    fn device_check() {
        let profile = sample();
        let text = profile.to_toml().unwrap();
        let same = DeviceProfile::from_toml_for_device(&text, &profile.device_id);
        assert_eq!(same.unwrap(), profile);
        let other = DeviceProfile::from_toml_for_device(&text, "other | 1");
        assert_eq!(
            other.unwrap_err(),
            ProfileError::DeviceMismatch {
                expected: "other | 1".into(),
                found: profile.device_id.clone(),
            }
        );
        assert!(matches!(
            DeviceProfile::from_toml_for_device("x", "y"),
            Err(ProfileError::Parse(_))
        ));
    }
}
