//! What the person chose and enrolled, sealed in the service's state directory.

use crate::detect::{VoicePrint, WakeWord};
use serde::{Deserialize, Serialize};
use std::path::Path;
use ultar_svckit::store;

/// The number of takes needed before a word or the voice counts as enrolled.
pub const TAKES: u8 = 3;

/// The standard wake word the person may keep.
pub const DEFAULT_WAKE_WORD: &str = "Hey KM";

/// The person's choices. Voice control is off until they switch it on.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// Voice control is on.
    pub enabled: bool,
    /// The wake word (data).
    pub wake_word: String,
    /// STT language tag: `ar`, `en` or `de`.
    pub language: String,
    /// Chosen STT model name (data; empty: the default).
    pub model: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            wake_word: DEFAULT_WAKE_WORD.to_owned(),
            language: "ar".to_owned(),
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
    settings_file: std::path::PathBuf,
    enrollment_file: std::path::PathBuf,
}

impl Store {
    /// State files below `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            settings_file: dir.join("settings"),
            enrollment_file: dir.join("enrollment"),
        }
    }

    /// Reads both files; a missing or damaged file falls back to the defaults.
    pub fn load(&self) -> (Settings, Enrollment) {
        let settings = store::load::<Settings>(&self.settings_file).unwrap_or_default();
        let enrollment = store::load::<Enrollment>(&self.enrollment_file).unwrap_or_default();
        (settings, enrollment)
    }

    /// Writes both files atomically; logs are emitted by the caller on failure.
    pub fn save(&self, settings: &Settings, enrollment: &Enrollment) -> bool {
        store::save(&self.settings_file, settings).is_ok()
            && store::save(&self.enrollment_file, enrollment).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_off_with_the_arabic_language() {
        let s = Settings::default();
        assert!(!s.enabled);
        assert_eq!(s.wake_word, "Hey KM");
        assert_eq!(s.language, "ar");
        assert_eq!(TAKES, 3);
    }

    #[test]
    fn enrollment_round_trips() {
        let dir = std::env::temp_dir().join(format!("voiced-state-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let mut settings = Settings::default();
        settings.enabled = true;
        let enrollment = Enrollment::default();
        assert!(store.save(&settings, &enrollment));
        let (s, e) = store.load();
        assert!(s.enabled);
        assert!(e.wake.is_none() && e.secret.is_none() && e.print.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}