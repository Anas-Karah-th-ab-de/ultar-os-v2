//! Panels "Appearance", "Desktop bar" and "Windows and workspaces"
//! (specification §8 "Aussehen", "Arbeitsfläche & Dock", "Fenster &
//! Arbeitsflächen"; §11 for the one bottom bar and its alternatives).

use crate::schema::{Panel, SettingDef, TextFormat};

const SIZES: &[&str] = &["small", "medium", "large"];

pub(super) fn appearance() -> Vec<SettingDef> {
    const P: Panel = Panel::Appearance;
    let accents = &["gold", "blue", "green", "red", "purple", "teal"];
    vec![
        // Decision of 22.11.2026: dark ("Ultar Noir" with gold) is standard.
        SettingDef::choice("appearance.theme", P, &["dark", "light", "auto"], "dark"),
        SettingDef::choice("appearance.accent_color", P, accents, "gold"),
        SettingDef::choice(
            "appearance.wallpaper.mode",
            P,
            &["picture", "custom", "slideshow"],
            "picture",
        ),
        SettingDef::text(
            "appearance.wallpaper.picture",
            P,
            TextFormat::Name,
            "ultar-noir",
        ),
        SettingDef::text("appearance.wallpaper.custom_path", P, TextFormat::Path, ""),
        SettingDef::text(
            "appearance.wallpaper.slideshow_folder",
            P,
            TextFormat::Path,
            "",
        ),
        SettingDef::int(
            "appearance.wallpaper.slideshow_minutes",
            P,
            (1, 1440, 1),
            30,
        ),
        SettingDef::int("appearance.font_size", P, (80, 200, 10), 100),
        SettingDef::choice("appearance.icon_size", P, SIZES, "medium"),
    ]
}

pub(super) fn bar() -> Vec<SettingDef> {
    const P: Panel = Panel::DesktopBar;
    let corners = &["top_start", "top_end", "bottom_start", "bottom_end"];
    vec![
        // Standard: one single bar at the bottom. Alternatives: icons at
        // the start instead of the centre; bar at the top plus a dock.
        SettingDef::choice(
            "bar.layout",
            P,
            &["bottom_bar", "top_bar_dock"],
            "bottom_bar",
        ),
        SettingDef::choice("bar.icon_alignment", P, &["center", "start"], "center"),
        SettingDef::choice("bar.displays", P, &["primary", "all"], "primary"),
        SettingDef::flag("bar.autohide", P, false),
        SettingDef::flag("bar.show_trash", P, true),
        SettingDef::flag("bar.show_volumes", P, true),
        SettingDef::choice("dock.position", P, &["bottom", "start", "end"], "bottom"),
        SettingDef::int("dock.size", P, (32, 96, 8), 56),
        SettingDef::flag("desktop.icons.enabled", P, true),
        SettingDef::choice("desktop.icons.size", P, SIZES, "medium"),
        SettingDef::choice("desktop.icons.start_corner", P, corners, "top_start"),
        SettingDef::flag("bar.clock.show_seconds", P, false),
        SettingDef::flag("bar.clock.show_weekday", P, true),
        SettingDef::flag("bar.clock.show_date", P, true),
        SettingDef::flag("bar.clock.show_hijri", P, false),
    ]
}

pub(super) fn windows() -> Vec<SettingDef> {
    const P: Panel = Panel::Windows;
    vec![
        SettingDef::flag("windows.edge_snap", P, true),
        SettingDef::flag("windows.snap_suggest", P, true),
        SettingDef::choice("workspaces.mode", P, &["dynamic", "fixed"], "dynamic"),
        SettingDef::int("workspaces.count", P, (1, 36, 1), 4),
        SettingDef::flag("workspaces.primary_display_only", P, true),
        SettingDef::flag("windows.switch_current_workspace_only", P, false),
        SettingDef::flag("windows.hot_corner", P, false),
        SettingDef::choice("windows.focus_mode", P, &["click", "pointer"], "click"),
    ]
}
