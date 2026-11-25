//! The accessibility view settings on their way to the compositor:
//! `accessibility.zoom`, `accessibility.zoom_percent` and
//! `accessibility.colour_filter` become `Request::ConfigureVisual` (the display
//! service already holds the compositor's configuration right). Kept apart from
//! the pointer and key-repeat settings (`crate::input`) so the two grow on their
//! own.

use ultar_bus::messages::settings::SettingValue;
use ultar_surface::proto::{
    ColourFilter, Request, DEFAULT_ZOOM_PERCENT, MAX_ZOOM_PERCENT, MIN_ZOOM_PERCENT,
};

/// Setting: the magnifier is on.
pub const ZOOM: &str = "accessibility.zoom";
/// Setting: magnification in percent.
pub const ZOOM_PERCENT: &str = "accessibility.zoom_percent";
/// Setting: the colour filter (`none`, `greyscale`, `protanopia`, `deuteranopia`,
/// `tritanopia`, `invert`).
pub const COLOUR_FILTER: &str = "accessibility.colour_filter";

/// What the compositor was last told about the accessibility view (or should be).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VisualSettings {
    /// The magnifier is on.
    pub zoom: bool,
    /// Magnification in percent.
    pub zoom_percent: u16,
    /// The colour filter.
    pub filter: ColourFilter,
}

impl Default for VisualSettings {
    /// The defaults of the settings schema.
    fn default() -> Self {
        VisualSettings {
            zoom: false,
            zoom_percent: DEFAULT_ZOOM_PERCENT,
            filter: ColourFilter::None,
        }
    }
}

/// The colour filter a word of the setting names; anything else is the default.
fn filter_word(t: &str) -> ColourFilter {
    match t {
        "greyscale" => ColourFilter::Greyscale,
        "protanopia" => ColourFilter::Protanopia,
        "deuteranopia" => ColourFilter::Deuteranopia,
        "tritanopia" => ColourFilter::Tritanopia,
        "invert" => ColourFilter::Invert,
        _ => ColourFilter::None,
    }
}

impl VisualSettings {
    /// Takes a setting that changed; `true` when it was one of these and differs
    /// from what is known (the compositor must be told). Values of the wrong type
    /// change nothing; a number outside its schema falls back to the nearest end,
    /// an unknown filter word to the default.
    pub fn on_changed(&mut self, key: &str, value: &SettingValue) -> bool {
        let before = *self;
        match (key, value) {
            (ZOOM, SettingValue::Flag(on)) => self.zoom = *on,
            (ZOOM_PERCENT, SettingValue::Integer(v)) => {
                let limits = (i64::from(MIN_ZOOM_PERCENT), i64::from(MAX_ZOOM_PERCENT));
                self.zoom_percent =
                    u16::try_from((*v).clamp(limits.0, limits.1)).unwrap_or(DEFAULT_ZOOM_PERCENT);
            }
            (COLOUR_FILTER, SettingValue::Text(t)) => self.filter = filter_word(t),
            _ => {}
        }
        *self != before
    }

    /// The request that puts these values into effect.
    pub fn requests(&self) -> [Request; 1] {
        [Request::ConfigureVisual {
            zoom: self.zoom,
            zoom_percent: self.zoom_percent,
            filter: self.filter,
        }]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(p: &VisualSettings) -> Request {
        p.requests()[0].clone()
    }

    #[test]
    fn only_the_view_settings_reach_the_compositor() {
        let mut p = VisualSettings::default();
        assert!(!p.on_changed("pointer.speed", &SettingValue::Integer(5)));
        assert!(!p.on_changed(ZOOM, &SettingValue::Flag(false)));
        assert!(p.on_changed(ZOOM, &SettingValue::Flag(true)));
        assert!(p.on_changed(ZOOM_PERCENT, &SettingValue::Integer(400)));
        assert!(p.on_changed(COLOUR_FILTER, &SettingValue::Text("greyscale".into())));
        assert_eq!(
            view(&p),
            Request::ConfigureVisual {
                zoom: true,
                zoom_percent: 400,
                filter: ColourFilter::Greyscale,
            }
        );
        // A value of the wrong type changes nothing.
        assert!(!p.on_changed(ZOOM, &SettingValue::Integer(1)));
        assert!(!p.on_changed(ZOOM_PERCENT, &SettingValue::Flag(true)));
        assert!(!p.on_changed(COLOUR_FILTER, &SettingValue::Integer(2)));
    }

    #[test]
    fn the_zoom_percent_stays_within_its_range() {
        let mut p = VisualSettings::default();
        assert!(!p.on_changed(ZOOM_PERCENT, &SettingValue::Integer(200)));
        assert!(p.on_changed(ZOOM_PERCENT, &SettingValue::Integer(5000)));
        assert_eq!(p.zoom_percent, MAX_ZOOM_PERCENT);
        assert!(p.on_changed(ZOOM_PERCENT, &SettingValue::Integer(0)));
        assert_eq!(p.zoom_percent, MIN_ZOOM_PERCENT);
    }

    #[test]
    fn every_filter_word_is_followed_and_unknown_ones_fall_back() {
        let mut p = VisualSettings::default();
        for (word, filter) in [
            ("greyscale", ColourFilter::Greyscale),
            ("protanopia", ColourFilter::Protanopia),
            ("deuteranopia", ColourFilter::Deuteranopia),
            ("tritanopia", ColourFilter::Tritanopia),
            ("invert", ColourFilter::Invert),
            ("none", ColourFilter::None),
        ] {
            assert!(p.on_changed(COLOUR_FILTER, &SettingValue::Text(word.into())));
            assert_eq!(p.filter, filter, "{word}");
        }
        // An unknown word falls back to the default (here from invert); a wrong
        // type changes nothing.
        p.on_changed(COLOUR_FILTER, &SettingValue::Text("invert".into()));
        assert!(p.on_changed(COLOUR_FILTER, &SettingValue::Text("sepia".into())));
        assert_eq!(p.filter, ColourFilter::None);
        assert!(!p.on_changed(COLOUR_FILTER, &SettingValue::Flag(true)));
    }

    #[test]
    fn turning_the_magnifier_off_keeps_the_percent_and_filter() {
        let mut p = VisualSettings::default();
        p.on_changed(ZOOM, &SettingValue::Flag(true));
        p.on_changed(ZOOM_PERCENT, &SettingValue::Integer(300));
        p.on_changed(COLOUR_FILTER, &SettingValue::Text("invert".into()));
        assert!(p.on_changed(ZOOM, &SettingValue::Flag(false)));
        assert_eq!(
            view(&p),
            Request::ConfigureVisual {
                zoom: false,
                zoom_percent: 300,
                filter: ColourFilter::Invert,
            }
        );
    }
}
