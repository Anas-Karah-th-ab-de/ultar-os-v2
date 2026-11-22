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

#[cfg(test)]
mod tests {
    use super::*;

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
