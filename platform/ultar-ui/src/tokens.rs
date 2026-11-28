//! Design tokens, generated at build time from `tokens.toml` (design spec §3).
//!
//! This is the only module that contains visual values. Everything else in the crate – and in
//! every app – names roles ([`ColorRole`]), steps ([`Space`], [`TypeSize`]) and radii
//! ([`Radius`]); the test `tests/tokens.rs` scans the sources to keep it that way.

use crate::color::Color;

include!(concat!(env!("OUT_DIR"), "/tokens_gen.rs"));

/// The token source this crate was generated from (for the drift test against `ULTAR_TOKENS`).
pub const SOURCE: &str = include_str!("../tokens.toml");

/// One colour per [`ColorRole`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    colors: [Color; ColorRole::ALL.len()],
}

impl Palette {
    /// A palette from colours in [`ColorRole::ALL`] order (used by the generated constants).
    pub const fn from_roles(colors: [Color; ColorRole::ALL.len()]) -> Palette {
        Palette { colors }
    }

    /// The colour of a role.
    pub const fn get(&self, role: ColorRole) -> Color {
        self.colors[role as usize]
    }
}

/// Which palette is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ThemeKind {
    /// The dark palette (standard).
    #[default]
    Dark,
    /// The light palette.
    Light,
}

impl ThemeKind {
    /// Both themes, standard first.
    pub const ALL: [ThemeKind; 2] = [ThemeKind::Dark, ThemeKind::Light];

    /// Short lowercase name (`dark` / `light`), used in snapshot file names.
    pub const fn name(self) -> &'static str {
        match self {
            ThemeKind::Dark => "dark",
            ThemeKind::Light => "light",
        }
    }
}

/// Both palettes of the system.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Theme {
    /// Dark palette.
    pub dark: Palette,
    /// Light palette.
    pub light: Palette,
}

/// The theme generated from `tokens.toml`.
pub const THEME: Theme = Theme {
    dark: DARK,
    light: LIGHT,
};

impl Theme {
    /// The palette of a theme kind.
    pub const fn palette(&self, kind: ThemeKind) -> &Palette {
        match kind {
            ThemeKind::Dark => &self.dark,
            ThemeKind::Light => &self.light,
        }
    }

    /// Opacity of the one shadow level for a theme kind.
    pub const fn shadow_alpha(kind: ThemeKind) -> f32 {
        match kind {
            ThemeKind::Dark => shadow::ALPHA_DARK,
            ThemeKind::Light => shadow::ALPHA_LIGHT,
        }
    }
}

/// Size class of a surface by its logical width (compact / medium / wide).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeClass {
    /// Narrower than 600 points (phones).
    Compact,
    /// 600 to 1023 points (tablets, small windows).
    Medium,
    /// 1024 points and wider.
    Wide,
}

impl SizeClass {
    /// The class for a logical width.
    pub fn for_width(width: f32) -> SizeClass {
        if width <= layout::COMPACT_MAX {
            SizeClass::Compact
        } else if width <= layout::MEDIUM_MAX {
            SizeClass::Medium
        } else {
            SizeClass::Wide
        }
    }
}

/// Which pointing device sizes the targets (design spec §3 "Ziele").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum InputMode {
    /// Mouse or touchpad.
    #[default]
    Pointer,
    /// Finger.
    Touch,
}

impl InputMode {
    /// Smallest allowed height/width of an interactive target in logical points.
    pub const fn min_target(self) -> f32 {
        match self {
            InputMode::Pointer => size::TARGET_POINTER_MIN,
            InputMode::Touch => size::TARGET_TOUCH_MIN,
        }
    }
}

/// The accent colour the person chose (`appearance.accent_color`): it replaces the three
/// accent roles ([`ColorRole::ACCENT`], [`ColorRole::ON_ACCENT`], [`ColorRole::ACCENT_TEXT`])
/// of the theme. Gold is the brand's own and the palette's value; the others keep the same
/// contrast rules (text on the fill and the text colour on every surface at least 4.5:1).
/// High contrast keeps its own palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Accent {
    /// The brand gold (standard).
    #[default]
    Gold,
    /// Blue.
    Blue,
    /// Green.
    Green,
    /// Red.
    Red,
    /// Purple.
    Purple,
    /// Teal.
    Teal,
}

impl Accent {
    /// Every accent, in the order of the setting's choices.
    pub const ALL: [Accent; 6] = [
        Accent::Gold,
        Accent::Blue,
        Accent::Green,
        Accent::Red,
        Accent::Purple,
        Accent::Teal,
    ];

    /// The setting's word.
    pub const fn word(self) -> &'static str {
        match self {
            Accent::Gold => "gold",
            Accent::Blue => "blue",
            Accent::Green => "green",
            Accent::Red => "red",
            Accent::Purple => "purple",
            Accent::Teal => "teal",
        }
    }

    /// The accent of a setting's word (`None` for an unknown word).
    pub fn parse(word: &str) -> Option<Accent> {
        Accent::ALL.into_iter().find(|a| a.word() == word.trim())
    }

    /// Fill, text on the fill and text colour of the accent in a theme; `None` for gold
    /// (the palette's own values).
    pub const fn colors(self, kind: ThemeKind) -> Option<[Color; 3]> {
        let dark = matches!(kind, ThemeKind::Dark);
        let c = Color::rgb;
        Some(match (self, dark) {
            (Accent::Gold, _) => return None,
            (Accent::Blue, true) => [
                c(0x5B, 0x9B, 0xFF),
                c(0x08, 0x11, 0x1F),
                c(0x8A, 0xB8, 0xFF),
            ],
            (Accent::Blue, false) => [
                c(0x2F, 0x6F, 0xDB),
                c(0xFF, 0xFF, 0xFF),
                c(0x1F, 0x56, 0xB3),
            ],
            (Accent::Green, true) => [
                c(0x3F, 0xBF, 0x6E),
                c(0x07, 0x14, 0x0C),
                c(0x6F, 0xD0, 0x8C),
            ],
            (Accent::Green, false) => [
                c(0x1C, 0x7F, 0x42),
                c(0xFF, 0xFF, 0xFF),
                c(0x17, 0x69, 0x3A),
            ],
            (Accent::Red, true) => [
                c(0xF0, 0x6A, 0x5F),
                c(0x1A, 0x08, 0x06),
                c(0xFF, 0x8F, 0x85),
            ],
            (Accent::Red, false) => [
                c(0xC8, 0x37, 0x2D),
                c(0xFF, 0xFF, 0xFF),
                c(0xA3, 0x28, 0x1F),
            ],
            (Accent::Purple, true) => [
                c(0xA5, 0x7C, 0xF2),
                c(0x11, 0x0A, 0x1E),
                c(0xC2, 0xA6, 0xFF),
            ],
            (Accent::Purple, false) => [
                c(0x70, 0x46, 0xC9),
                c(0xFF, 0xFF, 0xFF),
                c(0x5B, 0x35, 0xA8),
            ],
            (Accent::Teal, true) => [
                c(0x2C, 0xC4, 0xB5),
                c(0x04, 0x15, 0x12),
                c(0x5E, 0xD9, 0xCC),
            ],
            (Accent::Teal, false) => [
                c(0x0F, 0x80, 0x75),
                c(0xFF, 0xFF, 0xFF),
                c(0x0B, 0x6A, 0x61),
            ],
        })
    }

    /// The colour of `role` in `palette` with this accent.
    pub fn apply(self, kind: ThemeKind, palette: &Palette, role: ColorRole) -> Color {
        match (self.colors(kind), role) {
            (Some([fill, _, _]), ColorRole::Accent) => fill,
            (Some([_, on, _]), ColorRole::OnAccent) => on,
            (Some([_, _, text]), ColorRole::AccentText) => text,
            _ => palette.get(role),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_accent_keeps_the_contrast_rules() {
        for kind in ThemeKind::ALL {
            let palette = THEME.palette(kind);
            for accent in Accent::ALL {
                let fill = accent.apply(kind, palette, ColorRole::Accent);
                let on = accent.apply(kind, palette, ColorRole::OnAccent);
                let text = accent.apply(kind, palette, ColorRole::AccentText);
                assert!(
                    on.contrast_ratio(fill) >= 4.5,
                    "{accent:?} {kind:?} text on fill"
                );
                for ground in [ColorRole::Ground, ColorRole::Surface, ColorRole::Raised] {
                    let g = palette.get(ground);
                    assert!(
                        text.contrast_ratio(g) >= 4.5,
                        "{accent:?} {kind:?} text on {ground:?}"
                    );
                }
                assert_eq!(Accent::parse(accent.word()), Some(accent));
            }
        }
        assert_eq!(Accent::parse("pink"), None);
    }

    #[test]
    fn scales_are_sorted_and_positive() {
        for pair in Space::ALL.windows(2) {
            assert!(pair[0].points() < pair[1].points());
        }
        for pair in TypeSize::ALL.windows(2) {
            assert!(pair[0].points() < pair[1].points());
        }
        assert!(Space::ALL[0].points() > 0.0);
        assert!(Radius::Round.points() > Radius::Card.points());
        assert!(Radius::Card.points() > Radius::Control.points());
    }

    #[test]
    fn size_classes_follow_the_limits() {
        assert_eq!(
            SizeClass::for_width(layout::COMPACT_MAX),
            SizeClass::Compact
        );
        assert_eq!(
            SizeClass::for_width(layout::COMPACT_MAX + 1.0),
            SizeClass::Medium
        );
        assert_eq!(SizeClass::for_width(layout::MEDIUM_MAX), SizeClass::Medium);
        assert_eq!(
            SizeClass::for_width(layout::MEDIUM_MAX + 1.0),
            SizeClass::Wide
        );
    }

    #[test]
    fn high_contrast_text_reaches_7_to_1_on_every_ground() {
        let p = HIGH_CONTRAST;
        for text in [ColorRole::Text, ColorRole::TextMuted, ColorRole::AccentText] {
            for ground in [
                ColorRole::Ground,
                ColorRole::Surface,
                ColorRole::Raised,
                ColorRole::Bar,
            ] {
                assert!(
                    p.get(text).contrast_ratio(p.get(ground)) >= 7.0,
                    "{text:?} on {ground:?}"
                );
            }
        }
        assert!(
            p.get(ColorRole::OnAccent)
                .contrast_ratio(p.get(ColorRole::Accent))
                >= 7.0
        );
        assert!(
            p.get(ColorRole::ControlBorder)
                .contrast_ratio(p.get(ColorRole::Surface))
                >= 7.0
        );
    }

    #[test]
    fn palettes_and_modes() {
        assert_eq!(THEME.palette(ThemeKind::Dark), &DARK);
        assert_ne!(DARK.get(ColorRole::Ground), LIGHT.get(ColorRole::Ground));
        assert!(Theme::shadow_alpha(ThemeKind::Dark) > Theme::shadow_alpha(ThemeKind::Light));
        assert!(InputMode::Touch.min_target() > InputMode::Pointer.min_target());
        assert_eq!(ColorRole::ALL[ColorRole::Text as usize], ColorRole::Text);
        assert_eq!(ColorRole::TextMuted.name(), "text_muted");
        assert_eq!(typography::BODY, TypeSize::S14);
        assert_eq!(ThemeKind::Light.name(), "light");
    }
}
