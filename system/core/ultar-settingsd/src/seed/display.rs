//! Panel "Display" (specification §8 "Anzeige").

use super::CONFIRM_SECONDS;
use crate::displays::MAX_DISPLAYS;
use crate::schema::{Panel, Risk, Scope, SettingDef, TextFormat};
use ultar_profile::Capability;

const P: Panel = Panel::Display;
const KEEP: Risk = Risk::ConfirmRevert {
    seconds: CONFIRM_SECONDS,
};

fn per_combination(def: SettingDef) -> SettingDef {
    def.scope(Scope::DisplayCombination)
}

/// Mode, primary display and arrangement: remembered per combination,
/// every change asks "keep?".
fn arrangement() -> Vec<SettingDef> {
    vec![
        SettingDef::choice("display.mode", P, &["extend", "mirror", "single"], "extend").risk(KEEP),
        SettingDef::list(
            "display.mirror_displays",
            P,
            TextFormat::DisplayId,
            MAX_DISPLAYS,
        )
        .risk(KEEP),
        SettingDef::text("display.single_display", P, TextFormat::DisplayId, "")
            .risk(KEEP)
            .computed(),
        SettingDef::text("display.primary", P, TextFormat::DisplayId, "")
            .risk(KEEP)
            .computed(),
        SettingDef::int("display.<id>.position_x", P, (-65_536, 65_536, 1), 0)
            .risk(KEEP)
            .computed(),
        SettingDef::int("display.<id>.position_y", P, (-65_536, 65_536, 1), 0)
            .risk(KEEP)
            .computed(),
    ]
    .into_iter()
    .map(per_combination)
    .collect()
}

/// Values of one display, remembered per combination.
fn per_display() -> Vec<SettingDef> {
    let orientations = &[
        "landscape",
        "portrait",
        "landscape_flipped",
        "portrait_flipped",
    ];
    vec![
        SettingDef::text("display.<id>.resolution", P, TextFormat::Resolution, "auto")
            .risk(KEEP)
            .computed(),
        SettingDef::int("display.<id>.scale", P, (100, 300, 25), 100)
            .risk(KEEP)
            .computed(),
        // 0 = the rate the display recommends.
        SettingDef::float("display.<id>.refresh_rate", P, (0.0, 1000.0, 0.0), 0.0)
            .risk(KEEP)
            .computed(),
        SettingDef::flag("display.<id>.variable_refresh", P, true),
        SettingDef::choice("display.<id>.orientation", P, orientations, "landscape").risk(KEEP),
        SettingDef::flag("display.<id>.hdr", P, false),
        SettingDef::text(
            "display.<id>.color_profile",
            P,
            TextFormat::Free { max_chars: 128 },
            "",
        ),
        SettingDef::int("display.<id>.brightness", P, (0, 100, 1), 80),
        SettingDef::int("display.<id>.overscan", P, (0, 20, 1), 0),
    ]
    .into_iter()
    .map(per_combination)
    .collect()
}

/// Settings that hold for every combination.
fn general() -> Vec<SettingDef> {
    let schedules = &["sunset_to_sunrise", "custom", "always"];
    vec![
        SettingDef::flag("display.night_light.enabled", P, false),
        SettingDef::choice(
            "display.night_light.schedule",
            P,
            schedules,
            "sunset_to_sunrise",
        ),
        SettingDef::text("display.night_light.start", P, TextFormat::Time, "20:00"),
        SettingDef::text("display.night_light.end", P, TextFormat::Time, "06:00"),
        SettingDef::int(
            "display.night_light.temperature",
            P,
            (1700, 6500, 100),
            4000,
        ),
        SettingDef::flag("display.remember_window_positions", P, true),
        // Brightness from the light sensor (board CompactDisplay, CLICKS display.autobrightness).
        SettingDef::flag("display.auto_brightness", P, true).needs(&[Capability::Sensors]),
        SettingDef::choice(
            "display.lid_close_with_external",
            P,
            &["stay_on", "suspend"],
            "stay_on",
        )
        .scope(Scope::System),
    ]
}

pub(super) fn definitions() -> Vec<SettingDef> {
    let mut defs = arrangement();
    defs.extend(per_display());
    defs.extend(general());
    defs
}
