//! The company logo of KM Code Craft (circuit cloud, the letters, the name below).
//!
//! The logo exists as a bitmap only (`assets/brand/logo.png`, 520 × 218, transparent
//! background; the picture alone: `assets/brand/mark-circuit.png`, 364 × 327). It stands
//! directly on the surface in both themes (no plate); the boot picture draws it on the boot
//! background ([`PLATE`]). It is scaled with its proportions kept, never above its own size,
//! never recoloured, cropped or mirrored (also not in right-to-left layouts: it is centred
//! or stands at the start side unchanged).

/// Width of the logo bitmap in pixels.
pub const WIDTH: u32 = 520;
/// Height of the logo bitmap in pixels.
pub const HEIGHT: u32 = 218;
/// Width of the picture-only bitmap in pixels.
pub const PICTURE_WIDTH: u32 = 364;
/// Height of the picture-only bitmap in pixels.
pub const PICTURE_HEIGHT: u32 = 327;

/// The boot background (`0xRRGGBB`) the boot picture draws the logo on.
pub const PLATE: u32 = 0x0B0D11;

/// Smallest logo width in physical pixels: below it the name line is not readable; use the
/// picture alone ([`PICTURE_MIN_WIDTH`]) or the company name as text.
pub const MIN_WIDTH: u32 = 160;
/// Smallest width of the picture alone in physical pixels.
pub const PICTURE_MIN_WIDTH: u32 = 32;

/// Size the logo is drawn at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Why a logo size was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SizeError {
    /// The space is narrower than [`MIN_WIDTH`] or too low for it.
    TooSmall,
}

impl core::fmt::Display for SizeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SizeError::TooSmall => f.write_str("brand.error.logo_too_small"),
        }
    }
}

impl core::error::Error for SizeError {}

/// The largest allowed logo size inside `max_width` × `max_height` pixels: proportions kept,
/// never larger than the bitmap ([`WIDTH`] × [`HEIGHT`]), never smaller than [`MIN_WIDTH`].
pub fn fit(max_width: u32, max_height: u32) -> Result<Size, SizeError> {
    let by_height = u64::from(max_height) * u64::from(WIDTH) / u64::from(HEIGHT);
    let width = u64::from(max_width).min(by_height).min(u64::from(WIDTH));
    // `width` is at most WIDTH, so it fits u32.
    let width = u32::try_from(width).unwrap_or(WIDTH);
    if width < MIN_WIDTH {
        return Err(SizeError::TooSmall);
    }
    let height = u64::from(width) * u64::from(HEIGHT) / u64::from(WIDTH);
    Ok(Size {
        width,
        height: u32::try_from(height).unwrap_or(HEIGHT),
    })
}

/// Free space on every side of a logo of `height` pixels: a quarter of its height.
pub const fn clear_space(height: u32) -> u32 {
    height / 4
}

/// A brand bitmap: 8-bit RGBA, rows from the top, not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bitmap<'a> {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: &'a [u8],
}

/// Why a bitmap file was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitmapError {
    /// Shorter than the 8-byte header.
    Truncated,
    /// The pixel data does not have `width * height * 4` bytes, or a side is zero.
    WrongLength,
}

impl core::fmt::Display for BitmapError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            BitmapError::Truncated => f.write_str("brand.error.bitmap_truncated"),
            BitmapError::WrongLength => f.write_str("brand.error.bitmap_length"),
        }
    }
}

impl core::error::Error for BitmapError {}

impl<'a> Bitmap<'a> {
    /// Reads the raw form written by `scripts/brand-assets.py`: width and height as
    /// little-endian `u32`, then the pixels.
    pub fn parse(raw: &'a [u8]) -> Result<Bitmap<'a>, BitmapError> {
        let (Some(w), Some(h), Some(rgba)) = (raw.get(0..4), raw.get(4..8), raw.get(8..)) else {
            return Err(BitmapError::Truncated);
        };
        let width = u32::from_le_bytes([w[0], w[1], w[2], w[3]]);
        let height = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        let expected = u64::from(width)
            .checked_mul(u64::from(height))
            .and_then(|pixels| pixels.checked_mul(4));
        if width == 0 || height == 0 || expected != u64::try_from(rgba.len()).ok() {
            return Err(BitmapError::WrongLength);
        }
        Ok(Bitmap {
            width,
            height,
            rgba,
        })
    }
}

/// The embedded company logo (name line included), transparent background.
#[cfg(feature = "bitmaps")]
pub fn bitmap() -> Result<Bitmap<'static>, BitmapError> {
    Bitmap::parse(include_bytes!("../../../../assets/brand/logo.rgba"))
}

/// The embedded picture of the company logo alone, transparent background.
#[cfg(feature = "bitmaps")]
pub fn picture() -> Result<Bitmap<'static>, BitmapError> {
    Bitmap::parse(include_bytes!("../../../../assets/brand/mark-circuit.rgba"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_keeps_proportions_and_limits() {
        assert_eq!(
            fit(10_000, 10_000),
            Ok(Size {
                width: WIDTH,
                height: HEIGHT
            })
        );
        assert_eq!(
            fit(260, 10_000),
            Ok(Size {
                width: 260,
                height: 109
            })
        );
        // height decides
        assert_eq!(
            fit(10_000, 109),
            Ok(Size {
                width: 260,
                height: 109
            })
        );
        assert_eq!(fit(u32::MAX, u32::MAX).map(|s| s.width), Ok(WIDTH));
    }

    #[test]
    fn fit_refuses_what_is_too_small() {
        assert_eq!(fit(MIN_WIDTH - 1, 10_000), Err(SizeError::TooSmall));
        assert_eq!(fit(10_000, 10), Err(SizeError::TooSmall));
        assert_eq!(fit(0, 0), Err(SizeError::TooSmall));
        assert!(fit(MIN_WIDTH, 10_000).is_ok());
    }

    #[test]
    fn clear_space_is_a_quarter_of_the_height() {
        assert_eq!(clear_space(HEIGHT), 54);
        assert_eq!(clear_space(0), 0);
    }

    #[test]
    fn bitmap_parser_checks_header_and_length() {
        let mut raw = [0u8; 8 + 2 * 3 * 4];
        raw[0] = 2;
        raw[4] = 3;
        let parsed = Bitmap::parse(&raw).unwrap();
        assert_eq!((parsed.width, parsed.height, parsed.rgba.len()), (2, 3, 24));
        assert_eq!(Bitmap::parse(&raw[..7]), Err(BitmapError::Truncated));
        assert_eq!(Bitmap::parse(&[]), Err(BitmapError::Truncated));
        assert_eq!(Bitmap::parse(&raw[..31]), Err(BitmapError::WrongLength));
        assert_eq!(Bitmap::parse(&[0u8; 8]), Err(BitmapError::WrongLength));
        // a header that claims more than any buffer holds must not overflow
        let huge = [0xFFu8; 12];
        assert_eq!(Bitmap::parse(&huge), Err(BitmapError::WrongLength));
    }

    #[cfg(feature = "bitmaps")]
    #[test]
    fn embedded_bitmaps_have_the_documented_sizes() {
        let logo = bitmap().unwrap();
        assert_eq!((logo.width, logo.height), (WIDTH, HEIGHT));
        let picture = picture().unwrap();
        assert_eq!(
            (picture.width, picture.height),
            (PICTURE_WIDTH, PICTURE_HEIGHT)
        );
    }
}
