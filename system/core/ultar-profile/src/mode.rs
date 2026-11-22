//! Interface mode: which experience the platform shows on this device right now.

use crate::capability::Capability;
use crate::edition::Edition;
use crate::form_factor::FormFactor;
use crate::msg::{self, Lang};
use crate::profile::DeviceProfile;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Experience shown by the platform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Touch-first shell: home screen, gestures, full-screen apps.
    Mobile,
    /// Window-based shell: panel, launcher, free windows.
    Desktop,
    /// No on-screen interface at all.
    Headless,
}

impl Mode {
    /// Every mode.
    pub const ALL: [Mode; 3] = [Mode::Mobile, Mode::Desktop, Mode::Headless];

    /// Stable name used in settings.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Mobile => "mobile",
            Mode::Desktop => "desktop",
            Mode::Headless => "headless",
        }
    }

    /// Parses a stable name (case-insensitive).
    pub fn parse(name: &str) -> Option<Mode> {
        let wanted = name.trim().to_ascii_lowercase();
        Mode::ALL
            .iter()
            .copied()
            .find(|mode| mode.as_str() == wanted)
    }

    /// Stable message key of the display name.
    pub const fn key(self) -> &'static str {
        match self {
            Mode::Mobile => "profile.mode.mobile",
            Mode::Desktop => "profile.mode.desktop",
            Mode::Headless => "profile.mode.headless",
        }
    }

    /// Display name in the given language.
    pub fn label(self, lang: Lang) -> &'static str {
        msg::text(self.key(), lang).unwrap_or(self.as_str())
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the user chose about the interface mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModePreference {
    /// A mode fixed by the user; it overrides every rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced: Option<Mode>,
    /// Whether the system may switch modes on its own when hardware comes and goes
    /// (docking a phone, detaching a laptop keyboard).
    #[serde(default = "auto_switch_default")]
    pub auto_switch: bool,
}

fn auto_switch_default() -> bool {
    true
}

impl Default for ModePreference {
    fn default() -> Self {
        ModePreference {
            forced: None,
            auto_switch: true,
        }
    }
}

impl ModePreference {
    /// Preference that fixes one mode.
    pub const fn forced(mode: Mode) -> Self {
        ModePreference {
            forced: Some(mode),
            auto_switch: false,
        }
    }

    /// Preference that keeps the native mode of the device and never switches on its own.
    pub const fn no_auto_switch() -> Self {
        ModePreference {
            forced: None,
            auto_switch: false,
        }
    }
}

/// Chooses the interface mode from the capabilities of the device.
///
/// Rules, first match wins:
/// 1. A mode forced by the user is taken as it is.
/// 2. A server (form factor or edition) is headless.
/// 3. A phone or tablet with an external display, or with keyboard and pointer, runs
///    the desktop mode if automatic switching is allowed; otherwise the mobile mode.
/// 4. A laptop with touch and without an attached keyboard runs the mobile mode if
///    automatic switching is allowed; otherwise the desktop mode.
/// 5. A desktop computer runs the desktop mode.
/// 6. A board without a display is headless; with touch and without keyboard it runs
///    the mobile mode; otherwise the desktop mode.
pub fn choose_mode(profile: &DeviceProfile, user_pref: &ModePreference) -> Mode {
    if let Some(mode) = user_pref.forced {
        return mode;
    }
    if profile.form_factor == FormFactor::Server || profile.edition == Edition::Server {
        return Mode::Headless;
    }
    let capabilities = &profile.capabilities;
    let touch_only = capabilities.has(Capability::Touch) && !capabilities.has(Capability::Keyboard);
    match profile.form_factor {
        FormFactor::Phone | FormFactor::Tablet => {
            let docked = capabilities.has(Capability::ExternalDisplay)
                || (capabilities.has(Capability::Keyboard)
                    && capabilities.has(Capability::Pointer));
            if docked && user_pref.auto_switch {
                Mode::Desktop
            } else {
                Mode::Mobile
            }
        }
        FormFactor::Laptop if touch_only && user_pref.auto_switch => Mode::Mobile,
        FormFactor::Laptop | FormFactor::Desktop => Mode::Desktop,
        FormFactor::Board if !capabilities.has_display() => Mode::Headless,
        FormFactor::Board if touch_only => Mode::Mobile,
        FormFactor::Board => Mode::Desktop,
        FormFactor::Server => Mode::Headless,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capabilities::Capabilities;

    fn profile(form_factor: FormFactor, edition: Edition, caps: &[Capability]) -> DeviceProfile {
        let mut capabilities = Capabilities::default().with_displays(1);
        for capability in caps {
            capabilities.flags.insert(*capability);
        }
        DeviceProfile {
            device_id: "test".into(),
            form_factor,
            edition,
            capabilities,
        }
    }

    const AUTO: ModePreference = ModePreference {
        forced: None,
        auto_switch: true,
    };

    #[test]
    fn server_is_headless() {
        let server = profile(FormFactor::Server, Edition::Server, &[]);
        assert_eq!(choose_mode(&server, &AUTO), Mode::Headless);
        // A mini PC running the server preset is headless as well.
        let mini = profile(
            FormFactor::Desktop,
            Edition::Server,
            &[Capability::Keyboard],
        );
        assert_eq!(choose_mode(&mini, &AUTO), Mode::Headless);
        // Server hardware with a desktop preset still has no interface.
        let odd = profile(FormFactor::Server, Edition::Desktop, &[]);
        assert_eq!(choose_mode(&odd, &AUTO), Mode::Headless);
    }

    #[test]
    fn phone_and_tablet_docking() {
        use Capability::{ExternalDisplay, Keyboard, Pointer, Touch};
        for form_factor in [FormFactor::Phone, FormFactor::Tablet] {
            let plain = profile(form_factor, Edition::Phone, &[Touch]);
            assert_eq!(choose_mode(&plain, &AUTO), Mode::Mobile);
            let screen = profile(form_factor, Edition::Phone, &[Touch, ExternalDisplay]);
            assert_eq!(choose_mode(&screen, &AUTO), Mode::Desktop);
            let desk = profile(form_factor, Edition::Phone, &[Touch, Keyboard, Pointer]);
            assert_eq!(choose_mode(&desk, &AUTO), Mode::Desktop);
            // Only a keyboard (or only a mouse) is not a desk setup.
            let keyboard = profile(form_factor, Edition::Phone, &[Touch, Keyboard]);
            assert_eq!(choose_mode(&keyboard, &AUTO), Mode::Mobile);
            let pointer = profile(form_factor, Edition::Phone, &[Touch, Pointer]);
            assert_eq!(choose_mode(&pointer, &AUTO), Mode::Mobile);
            // Without permission to switch, the phone stays mobile.
            let fixed = ModePreference::no_auto_switch();
            assert_eq!(choose_mode(&screen, &fixed), Mode::Mobile);
            assert_eq!(choose_mode(&desk, &fixed), Mode::Mobile);
        }
    }

    #[test]
    fn laptop_rules() {
        use Capability::{Keyboard, Pointer, Touch};
        let classic = profile(FormFactor::Laptop, Edition::Desktop, &[Keyboard, Pointer]);
        assert_eq!(choose_mode(&classic, &AUTO), Mode::Desktop);
        let touch = profile(FormFactor::Laptop, Edition::Desktop, &[Keyboard, Touch]);
        assert_eq!(choose_mode(&touch, &AUTO), Mode::Desktop);
        let detached = profile(FormFactor::Laptop, Edition::Desktop, &[Touch]);
        assert_eq!(choose_mode(&detached, &AUTO), Mode::Mobile);
        assert_eq!(
            choose_mode(&detached, &ModePreference::no_auto_switch()),
            Mode::Desktop
        );
        // No touch and no keyboard: nothing suggests the mobile mode.
        let bare = profile(FormFactor::Laptop, Edition::Desktop, &[]);
        assert_eq!(choose_mode(&bare, &AUTO), Mode::Desktop);
    }

    #[test]
    fn desktop_and_board() {
        let desktop = profile(FormFactor::Desktop, Edition::Desktop, &[Capability::Touch]);
        assert_eq!(choose_mode(&desktop, &AUTO), Mode::Desktop);
        let kiosk = profile(FormFactor::Board, Edition::Phone, &[Capability::Touch]);
        assert_eq!(choose_mode(&kiosk, &AUTO), Mode::Mobile);
        let desk_board = profile(FormFactor::Board, Edition::Desktop, &[Capability::Keyboard]);
        assert_eq!(choose_mode(&desk_board, &AUTO), Mode::Desktop);
        let mut blind = profile(FormFactor::Board, Edition::Phone, &[Capability::Touch]);
        blind.capabilities.display_count = 0;
        assert_eq!(choose_mode(&blind, &AUTO), Mode::Headless);
    }

    #[test]
    fn user_preference_overrides_everything() {
        let server = profile(FormFactor::Server, Edition::Server, &[]);
        let phone = profile(FormFactor::Phone, Edition::Phone, &[Capability::Touch]);
        for mode in Mode::ALL {
            assert_eq!(choose_mode(&server, &ModePreference::forced(mode)), mode);
            assert_eq!(choose_mode(&phone, &ModePreference::forced(mode)), mode);
        }
    }

    #[test]
    fn names_and_preference_serde() {
        for mode in Mode::ALL {
            assert_eq!(Mode::parse(mode.as_str()), Some(mode));
            assert_eq!(mode.to_string(), mode.as_str());
            assert_ne!(mode.label(Lang::Ar), mode.as_str());
        }
        assert_eq!(Mode::parse("tv"), None);
        assert_eq!(ModePreference::default(), AUTO);
        let empty: ModePreference = toml::from_str("").unwrap();
        assert_eq!(empty, AUTO);
        let forced = ModePreference::forced(Mode::Desktop);
        let text = toml::to_string(&forced).unwrap();
        assert_eq!(toml::from_str::<ModePreference>(&text).unwrap(), forced);
        assert!(toml::from_str::<ModePreference>("forced = \"tv\"").is_err());
    }
}
