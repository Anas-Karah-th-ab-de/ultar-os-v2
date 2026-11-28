//! What the person chose and enrolled, kept in the service's state directory (own user, files
//! mode 0600, written atomically and checked by `ultar_svckit::store`).

use crate::detect::{VoicePrint, WakeWord};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use ultar_svckit::store;

/// The standard wake word, shown in Settings.
pub const DEFAULT_WAKE_WORD: &str = "Hey KM";

/// The speech languages: Arabic first.
pub const LANGUAGES: [&str; 3] = ["ar", "en", "de"];

/// The person's choices. Voice control is off until they switch it on.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// Voice control is on.
    pub enabled: bool,
    /// Speech language tag: `ar`, `en` or `de`.
    pub language: String,
    /// Chosen speech-to-text model (empty: the best installed one).
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            language: LANGUAGES[0].to_owned(),
            model: String::new(),
        }
    }
}

/// The enrolled templates and print (only this is stored; no request is ever stored).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug, Default)]
pub struct Enrollment {
    /// The wake word template.
    pub wake: Option<WakeWord>,
    /// The secret word template (works while locked).
    pub secret: Option<WakeWord>,
    /// The person's voice print.
    pub print: Option<VoicePrint>,
}

/// The two state files of the service.
pub struct Store {
    settings_file: PathBuf,
    enrollment_file: PathBuf,
}

impl Store {
    /// State files below `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            settings_file: dir.join("settings"),
            enrollment_file: dir.join("enrollment"),
        }
    }

    /// Reads both files; a missing or damaged file falls back to the defaults (voice control
    /// off, nothing enrolled).
    pub fn load(&self) -> (Settings, Enrollment) {
        let settings = store::load::<Settings>(&self.settings_file).unwrap_or_default();
        let enrollment = store::load::<Enrollment>(&self.enrollment_file).unwrap_or_default();
        (settings, enrollment)
    }

    /// Writes the settings atomically.
    pub fn save_settings(&self, settings: &Settings) -> bool {
        store::save(&self.settings_file, settings).is_ok()
    }

    /// Writes the enrolment atomically.
    pub fn save_enrollment(&self, enrollment: &Enrollment) -> bool {
        store::save(&self.enrollment_file, enrollment).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mfcc::Mfcc;
    use std::os::unix::fs::PermissionsExt;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("voiced-state-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn defaults_are_off_with_the_arabic_language() {
        let s = Settings::default();
        assert!(!s.enabled);
        assert_eq!(s.language, "ar");
        assert!(s.model.is_empty());
        let d = dir("none");
        let (settings, enrollment) = Store::new(&d).load();
        assert_eq!(settings, Settings::default());
        assert_eq!(enrollment, Enrollment::default());
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn choices_and_enrolment_round_trip_and_only_the_owner_can_read_them() {
        let d = dir("round");
        let store = Store::new(&d);
        let settings = Settings {
            enabled: true,
            language: "de".into(),
            model: "base".into(),
        };
        let takes: Vec<Vec<f32>> = [1.0f32, 1.1]
            .iter()
            .map(|s| {
                (0..(12_000.0 * s) as usize)
                    .map(|i| (i as f32 * 0.05 / s).sin() * 0.3)
                    .collect()
            })
            .collect();
        let wake = WakeWord::from_takes(&Mfcc::new(), &takes).ok();
        let enrollment = Enrollment {
            wake,
            ..Enrollment::default()
        };
        assert!(store.save_settings(&settings) && store.save_enrollment(&enrollment));
        let (s, e) = store.load();
        assert_eq!(s, settings);
        assert_eq!(e, enrollment);
        for name in ["settings", "enrollment"] {
            let mode = std::fs::metadata(d.join(name))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o077, 0, "{name} is private");
        }
        // a damaged file means the safe default: off and not enrolled
        std::fs::write(d.join("settings"), b"damaged").unwrap();
        assert_eq!(store.load().0, Settings::default());
        std::fs::remove_dir_all(&d).unwrap();
    }
}
