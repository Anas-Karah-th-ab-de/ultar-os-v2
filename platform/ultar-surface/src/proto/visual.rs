//! The accessibility view configuration ([`Request::ConfigureVisual`]) and the
//! visual-alert flash ([`Request::VisualAlert`]): their limits, the colour filter
//! and the wire decoding. The display service passes `accessibility.zoom`,
//! `accessibility.zoom_percent` and `accessibility.colour_filter` on; the shell
//! asks for a flash when a notification alerts and `accessibility.visual_alert`
//! is on.

use super::Request;
use crate::wire::{Reader, WireError, Writer};

/// Least magnification of [`Request::ConfigureVisual`] (100 % = the output as it is).
pub const MIN_ZOOM_PERCENT: u16 = 100;
/// Most magnification of [`Request::ConfigureVisual`].
pub const MAX_ZOOM_PERCENT: u16 = 800;
/// Magnification when the setting does not say (the settings schema's default).
pub const DEFAULT_ZOOM_PERCENT: u16 = 200;
/// Longest visual-alert flash of [`Request::VisualAlert`], in milliseconds.
pub const MAX_FLASH_MS: u32 = 1_000;

/// The colour filter applied to the composed output (`accessibility.colour_filter`):
/// a final colour matrix over every pixel. The colour-blind modes are the usual
/// simulation matrices (Viénot–Brettel); greyscale is the luminance weights.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ColourFilter {
    /// No filter.
    #[default]
    None,
    /// Luminance only.
    Greyscale,
    /// Red-blind (protanopia).
    Protanopia,
    /// Green-blind (deuteranopia).
    Deuteranopia,
    /// Blue-blind (tritanopia).
    Tritanopia,
    /// Inverted colours.
    Invert,
}

impl ColourFilter {
    /// The byte on the wire.
    pub fn encode(self, w: &mut Writer) {
        w.u8(match self {
            ColourFilter::None => 0,
            ColourFilter::Greyscale => 1,
            ColourFilter::Protanopia => 2,
            ColourFilter::Deuteranopia => 3,
            ColourFilter::Tritanopia => 4,
            ColourFilter::Invert => 5,
        });
    }

    /// Reads one byte; unknown filters are refused.
    pub fn decode(r: &mut Reader<'_>) -> Result<ColourFilter, WireError> {
        Ok(match r.u8()? {
            0 => ColourFilter::None,
            1 => ColourFilter::Greyscale,
            2 => ColourFilter::Protanopia,
            3 => ColourFilter::Deuteranopia,
            4 => ColourFilter::Tritanopia,
            5 => ColourFilter::Invert,
            _ => return Err(WireError::BadValue),
        })
    }
}

/// The language of the compositor's own drawing ([`Request::ConfigureLook`]); the byte on
/// the wire is its number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum LookLang {
    /// Arabic (right to left; the standard).
    #[default]
    Ar = 0,
    /// English.
    En = 1,
    /// German.
    De = 2,
}

impl LookLang {
    /// The language of a settings word (`ar`, `en`, `de`).
    pub fn from_word(word: &str) -> Option<LookLang> {
        match word.trim() {
            "ar" => Some(LookLang::Ar),
            "en" => Some(LookLang::En),
            "de" => Some(LookLang::De),
            _ => None,
        }
    }

    /// Reads the wire byte; unknown languages are refused.
    pub fn decode(byte: u8) -> Result<LookLang, WireError> {
        match byte {
            0 => Ok(LookLang::Ar),
            1 => Ok(LookLang::En),
            2 => Ok(LookLang::De),
            _ => Err(WireError::BadValue),
        }
    }
}

/// Encodes the body of [`Request::ConfigureVisual`] (56), [`Request::VisualAlert`] (57) and
/// [`Request::ConfigureLook`] (58); other requests write nothing.
pub(super) fn encode(request: &Request, w: &mut Writer) {
    match request {
        Request::ConfigureVisual {
            zoom,
            zoom_percent,
            filter,
        } => {
            w.bool(*zoom).u16(*zoom_percent);
            filter.encode(w);
        }
        Request::VisualAlert { ms } => {
            w.u32(*ms);
        }
        Request::ConfigureLook { lang, light } => {
            w.u8(*lang as u8).bool(*light);
        }
        _ => {}
    }
}

/// Decodes the body of [`Request::ConfigureVisual`] (56) or [`Request::VisualAlert`]
/// (57). Every field is checked; out-of-range values are errors.
pub(super) fn decode(opcode: u16, r: &mut Reader<'_>) -> Result<Request, WireError> {
    if opcode == 56 {
        let zoom = r.bool()?;
        let zoom_percent = r.u16()?;
        if !(MIN_ZOOM_PERCENT..=MAX_ZOOM_PERCENT).contains(&zoom_percent) {
            return Err(WireError::BadValue);
        }
        let filter = ColourFilter::decode(r)?;
        return Ok(Request::ConfigureVisual {
            zoom,
            zoom_percent,
            filter,
        });
    }
    let ms = r.u32()?;
    if ms == 0 || ms > MAX_FLASH_MS {
        return Err(WireError::BadValue);
    }
    Ok(Request::VisualAlert { ms })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_round_trip_and_unknown_ones_are_refused() {
        assert_eq!(ColourFilter::default(), ColourFilter::None);
        for filter in [
            ColourFilter::None,
            ColourFilter::Greyscale,
            ColourFilter::Protanopia,
            ColourFilter::Deuteranopia,
            ColourFilter::Tritanopia,
            ColourFilter::Invert,
        ] {
            let mut w = Writer::new(0);
            filter.encode(&mut w);
            let frame = w.finish().unwrap();
            let mut r = Reader::new(&frame[8..]);
            assert_eq!(ColourFilter::decode(&mut r).unwrap(), filter);
        }
        let mut r = Reader::new(&[6]);
        assert_eq!(ColourFilter::decode(&mut r), Err(WireError::BadValue));
    }

    #[test]
    fn the_look_request_round_trips_and_refuses_unknown_languages() {
        for lang in [LookLang::Ar, LookLang::En, LookLang::De] {
            let look = Request::ConfigureLook { lang, light: true };
            assert_eq!(look.opcode(), 58);
            let frame = look.encode().unwrap();
            assert_eq!(Request::decode(58, &frame[8..]).unwrap(), look);
        }
        assert_eq!(LookLang::from_word("de"), Some(LookLang::De));
        assert_eq!(LookLang::from_word("fr"), None);
        let mut w = Writer::new(58);
        w.u8(3).bool(false);
        let frame = w.finish().unwrap();
        assert!(Request::decode(58, &frame[8..]).is_err());
    }

    #[test]
    fn the_visual_requests_round_trip_and_check_their_ranges() {
        let config = Request::ConfigureVisual {
            zoom: true,
            zoom_percent: 400,
            filter: ColourFilter::Deuteranopia,
        };
        assert_eq!(config.opcode(), 56);
        let frame = config.encode().unwrap();
        assert_eq!(Request::decode(56, &frame[8..]).unwrap(), config);
        let flash = Request::VisualAlert { ms: 200 };
        assert_eq!(flash.opcode(), 57);
        let frame = flash.encode().unwrap();
        assert_eq!(Request::decode(57, &frame[8..]).unwrap(), flash);
        // The decoder refuses out-of-range values (the encoder does not clamp).
        let mut w = Writer::new(56);
        w.bool(true).u16(MAX_ZOOM_PERCENT + 1);
        ColourFilter::None.encode(&mut w);
        let frame = w.finish().unwrap();
        assert!(Request::decode(56, &frame[8..]).is_err());
        for ms in [0, MAX_FLASH_MS + 1] {
            let mut w = Writer::new(57);
            w.u32(ms);
            let frame = w.finish().unwrap();
            assert!(Request::decode(57, &frame[8..]).is_err(), "ms {ms}");
        }
    }
}
