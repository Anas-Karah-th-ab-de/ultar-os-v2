//! Tones: the meaning a badge or info bar carries. A tone always comes with an icon and a
//! spoken name, never as colour alone (design spec §3 "Farben").

use crate::icon::IconId;
use crate::tokens::ColorRole;

/// The meaning of a status element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Tone {
    /// No particular meaning.
    #[default]
    Neutral,
    /// Belongs to the main action or "on".
    Accent,
    /// Information.
    Info,
    /// Something worked.
    Success,
    /// Something needs attention.
    Warning,
    /// Something failed or is destructive.
    Danger,
}

impl Tone {
    /// All tones.
    pub const ALL: [Tone; 6] = [
        Tone::Neutral,
        Tone::Accent,
        Tone::Info,
        Tone::Success,
        Tone::Warning,
        Tone::Danger,
    ];

    /// The colour role of the tone's icon, border and text.
    pub const fn role(self) -> ColorRole {
        match self {
            Tone::Neutral => ColorRole::TextMuted,
            Tone::Accent => ColorRole::AccentText,
            Tone::Info => ColorRole::Info,
            Tone::Success => ColorRole::Success,
            Tone::Warning => ColorRole::Warning,
            Tone::Danger => ColorRole::Danger,
        }
    }

    /// The icon that carries the tone's meaning without colour.
    pub const fn icon(self) -> IconId {
        match self {
            Tone::Neutral | Tone::Info => IconId::Info,
            Tone::Accent => IconId::Check,
            Tone::Success => IconId::Check,
            Tone::Warning => IconId::Warning,
            Tone::Danger => IconId::Error,
        }
    }

    /// Catalog key of the tone's spoken name; `None` for the neutral tone.
    pub const fn name_key(self) -> Option<&'static str> {
        match self {
            Tone::Neutral => None,
            Tone::Accent => Some("ui.tone.accent"),
            Tone::Info => Some("ui.tone.info"),
            Tone::Success => Some("ui.tone.success"),
            Tone::Warning => Some("ui.tone.warning"),
            Tone::Danger => Some("ui.tone.danger"),
        }
    }

    /// Short lowercase name, used in snapshot file names.
    pub const fn name(self) -> &'static str {
        match self {
            Tone::Neutral => "neutral",
            Tone::Accent => "accent",
            Tone::Info => "info",
            Tone::Success => "success",
            Tone::Warning => "warning",
            Tone::Danger => "danger",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_meaning_has_its_own_icon_and_name() {
        let meaningful = [Tone::Info, Tone::Success, Tone::Warning, Tone::Danger];
        for (i, a) in meaningful.iter().enumerate() {
            assert!(a.name_key().is_some());
            for b in &meaningful[i + 1..] {
                assert_ne!(a.icon(), b.icon(), "{a:?} / {b:?}: icon, not colour alone");
                assert_ne!(a.role(), b.role());
                assert_ne!(a.name_key(), b.name_key());
            }
        }
        assert_eq!(Tone::Neutral.name_key(), None);
        assert_eq!(Tone::ALL.len(), 6);
        assert_eq!(Tone::default().name(), "neutral");
    }
}
