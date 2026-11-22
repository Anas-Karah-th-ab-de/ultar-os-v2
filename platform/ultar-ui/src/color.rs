//! Colour value type. Concrete colours come only from [`crate::tokens`]; this module defines
//! the type, the two value-free constants (transparent, shadow black) and colour arithmetic.

/// A straight (non-premultiplied) 8-bit RGBA colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Opacity, 255 = opaque.
    pub a: u8,
}

const CHANNEL_MAX: f32 = 255.0;

impl Color {
    /// Fully transparent; paints nothing.
    pub const TRANSPARENT: Color = Color::rgba(0, 0, 0, 0);
    /// Shadow base colour; its opacity comes from the shadow tokens.
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// An opaque colour.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
        Color::rgba(r, g, b, u8::MAX)
    }

    /// A colour with explicit opacity.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color { r, g, b, a }
    }

    /// The same colour with its opacity multiplied by `factor` (clamped to 0..=1).
    pub fn with_alpha(self, factor: f32) -> Color {
        let a = (f32::from(self.a) * factor.clamp(0.0, 1.0)).round() as u8;
        Color { a, ..self }
    }

    /// Linear mix towards `other`; `t` = 0 keeps `self`, 1 gives `other`.
    pub fn mix(self, other: Color, t: f32) -> Color {
        let t = t.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * t).round() as u8;
        Color {
            r: lerp(self.r, other.r),
            g: lerp(self.g, other.g),
            b: lerp(self.b, other.b),
            a: lerp(self.a, other.a),
        }
    }

    /// `true` when nothing of the background shows through.
    pub const fn is_opaque(self) -> bool {
        self.a == u8::MAX
    }

    /// WCAG 2 relative luminance (0 = black, 1 = white); opacity is ignored.
    pub fn relative_luminance(self) -> f32 {
        let linear = |c: u8| {
            let v = f32::from(c) / CHANNEL_MAX;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(self.r) + 0.7152 * linear(self.g) + 0.0722 * linear(self.b)
    }

    /// WCAG 2 contrast ratio between two opaque colours (1..=21).
    pub fn contrast_ratio(self, other: Color) -> f32 {
        let a = self.relative_luminance();
        let b = other.relative_luminance();
        let (hi, lo) = if a >= b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Color = Color::rgb(u8::MAX, u8::MAX, u8::MAX);

    #[test]
    fn contrast_of_black_and_white_is_21() {
        let ratio = Color::BLACK.contrast_ratio(WHITE);
        assert!((ratio - 21.0).abs() < 0.01, "{ratio}");
        assert!((WHITE.contrast_ratio(Color::BLACK) - ratio).abs() < f32::EPSILON);
        assert!((WHITE.contrast_ratio(WHITE) - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn alpha_and_mix() {
        assert_eq!(WHITE.with_alpha(0.5).a, 128);
        assert_eq!(WHITE.with_alpha(7.0).a, u8::MAX);
        assert_eq!(WHITE.with_alpha(-1.0).a, 0);
        assert!(!Color::TRANSPARENT.is_opaque() && WHITE.is_opaque());
        assert_eq!(Color::BLACK.mix(WHITE, 0.0), Color::BLACK);
        assert_eq!(Color::BLACK.mix(WHITE, 1.0), WHITE);
        assert_eq!(Color::BLACK.mix(WHITE, 0.5).r, 128);
    }
}
