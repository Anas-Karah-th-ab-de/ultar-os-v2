//! The input languages (keyboard layouts) as the settings service keeps them: the list the
//! user switches through (`region.keyboard.layouts`, e.g. `ar_en`) and the active one
//! (`region.keyboard.active`). The shell gives both to the compositor at start and on every
//! change, and stores the active one when the user switched (Ctrl+Shift, Alt+Shift,
//! Super+Space or the bar's language menu), so it survives a restart.

use crate::ports::SettingsPort;
use ultar_bus::messages::settings::SettingValue;

/// The layouts to switch through, `_`-separated, Arabic first (settings schema).
pub const LAYOUTS: &str = "region.keyboard.layouts";
/// The active layout (`ar`, `en`, `de`).
pub const ACTIVE: &str = "region.keyboard.active";

/// Layout names of a `region.keyboard.layouts` value (only known names, no repeats).
pub fn names(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in value.split('_') {
        if matches!(n, "ar" | "en" | "de") && !out.iter().any(|o| o == n) {
            out.push(n.to_owned());
        }
    }
    out
}

/// The list and the index of the active layout in it, as the settings say
/// (`None`: no answer or nothing usable).
pub fn wanted(settings: &dyn SettingsPort) -> Option<(Vec<String>, u8)> {
    let list = match settings.get(LAYOUTS) {
        Ok(SettingValue::Text(v)) => names(&v),
        _ => return None,
    };
    if list.is_empty() {
        return None;
    }
    let active = match settings.get(ACTIVE) {
        Ok(SettingValue::Text(v)) => list.iter().position(|n| *n == v).unwrap_or(0),
        _ => 0,
    };
    Some((list, u8::try_from(active).unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_lists() {
        assert_eq!(names("ar_en"), vec!["ar", "en"]);
        assert_eq!(names("ar_de_en"), vec!["ar", "de", "en"]);
        assert_eq!(names("ar_xx_ar"), vec!["ar"]);
        assert!(names("").is_empty());
    }
}
