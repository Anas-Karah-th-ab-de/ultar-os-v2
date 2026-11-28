//! Panels "Mobile network", "Search", "Apps", "Mouse and touchpad",
//! "Keyboard", "Printers and scanners", "Colour" and "Accessibility"
//! (specification §8, §12.3).

use crate::schema::{Panel, Risk, Scope, SettingDef, TextFormat};

pub(super) fn mobile() -> Vec<SettingDef> {
    const P: Panel = Panel::Mobile;
    let defs = vec![
        SettingDef::flag("mobile.data", P, true),
        SettingDef::flag("mobile.roaming", P, false),
        SettingDef::choice(
            "mobile.network_type",
            P,
            &["auto", "g5", "g4", "g3", "g2"],
            "auto",
        ),
        // Specification §12.2: automatic, smart (stays on the best cell) or manual.
        SettingDef::choice(
            "mobile.network_selection",
            P,
            &["automatic", "smart", "manual"],
            "automatic",
        ),
        SettingDef::text("mobile.apn", P, TextFormat::Free { max_chars: 64 }, ""),
        SettingDef::choice("mobile.data_sim", P, &["sim1", "sim2"], "sim1"),
        SettingDef::flag("mobile.data_limit.enabled", P, false),
        // Megabytes per month.
        SettingDef::int(
            "mobile.data_limit.megabytes",
            P,
            (100, 1_000_000, 100),
            5000,
        ),
        SettingDef::flag("mobile.sim_pin", P, false).risk(Risk::AdminOnly),
    ];
    defs.into_iter().map(|d| d.scope(Scope::System)).collect()
}

pub(super) fn search() -> Vec<SettingDef> {
    const P: Panel = Panel::Search;
    vec![
        SettingDef::flag("search.source.apps", P, true),
        SettingDef::flag("search.source.settings", P, true),
        SettingDef::flag("search.source.files", P, true),
        SettingDef::flag("search.source.calculator", P, true),
        SettingDef::flag("search.source.characters", P, true),
        SettingDef::flag("search.source.store", P, true),
        SettingDef::flag("search.source.commands", P, true),
        SettingDef::list("search.file_places", P, TextFormat::Path, 16),
        // Arabic search ignores vowel marks and spelling variants (specification §5.1).
        SettingDef::flag("search.arabic_variants", P, true),
        SettingDef::flag("search.history", P, true),
    ]
}

pub(super) fn apps() -> Vec<SettingDef> {
    const P: Panel = Panel::Apps;
    let name =
        |key: &'static str, default: &str| SettingDef::text(key, P, TextFormat::AppId, default);
    vec![
        name("apps.default.web", "browser"),
        name("apps.default.mail", "mail"),
        name("apps.default.calendar", "calendar"),
        name("apps.default.music", "media"),
        name("apps.default.video", "media"),
        name("apps.default.photos", "images"),
        name("apps.default.documents", "documents"),
        SettingDef::choice(
            "apps.removable_media",
            P,
            &["ask", "open_files", "nothing"],
            "ask",
        ),
        SettingDef::list("apps.autostart", P, TextFormat::AppId, 64),
        // Store ids of the components chosen at the installation (`ultar-firstboot`); the
        // Store marks those not installed yet. Read only by the Store, never set by it.
        SettingDef::list("apps.install_selection", P, TextFormat::AppId, 16),
        // Rights per app (the launcher asks on first use, the answer lands here).
        SettingDef::flag("apps.right.<id>.camera", P, false),
        SettingDef::flag("apps.right.<id>.microphone", P, false),
        SettingDef::flag("apps.right.<id>.location", P, false),
        SettingDef::flag("apps.right.<id>.files", P, false),
        SettingDef::flag("apps.right.<id>.network", P, true),
        SettingDef::flag("apps.right.<id>.background", P, false),
    ]
}

pub(super) fn pointer() -> Vec<SettingDef> {
    const P: Panel = Panel::Pointer;
    vec![
        SettingDef::choice("pointer.primary_button", P, &["left", "right"], "left"),
        SettingDef::int("pointer.speed", P, (-10, 10, 1), 0),
        SettingDef::flag("pointer.acceleration", P, true),
        SettingDef::flag("pointer.natural_scroll", P, false),
        SettingDef::choice("pointer.size", P, &["normal", "large", "larger"], "normal"),
        SettingDef::flag("pointer.touchpad.tap_to_click", P, true),
        SettingDef::flag("pointer.touchpad.two_finger_scroll", P, true),
        SettingDef::flag("pointer.touchpad.natural_scroll", P, true),
        SettingDef::flag("pointer.touchpad.disable_while_typing", P, true),
        SettingDef::flag("pointer.touchpad.gestures", P, true),
    ]
}

pub(super) fn keyboard() -> Vec<SettingDef> {
    const P: Panel = Panel::Keyboard;
    vec![
        // Input languages in order; empty = the system language and English.
        SettingDef::list("keyboard.input_languages", P, TextFormat::Name, 8),
        SettingDef::choice(
            "keyboard.switch_shortcut",
            P,
            &["super_space", "alt_shift", "ctrl_shift"],
            "super_space",
        ),
        SettingDef::flag("keyboard.per_window", P, false),
        SettingDef::choice(
            "keyboard.compose_key",
            P,
            &["none", "alt_gr", "menu", "caps_lock"],
            "none",
        ),
        SettingDef::flag("keyboard.repeat.enabled", P, true),
        SettingDef::int("keyboard.repeat.delay_ms", P, (150, 1000, 50), 500),
        SettingDef::int("keyboard.repeat.per_second", P, (5, 60, 1), 25),
        SettingDef::choice(
            "keyboard.on_screen",
            P,
            &["automatic", "always", "never"],
            "automatic",
        ),
        SettingDef::flag("keyboard.suggestions", P, true),
        SettingDef::flag("keyboard.autocorrect", P, false),
    ]
}

pub(super) fn printers() -> Vec<SettingDef> {
    const P: Panel = Panel::Printers;
    let defs = vec![
        SettingDef::flag("printers.discover", P, true),
        SettingDef::text(
            "printers.default",
            P,
            TextFormat::Free { max_chars: 64 },
            "",
        ),
        SettingDef::choice("printers.paper", P, &["a4", "a5", "letter"], "a4"),
        SettingDef::choice(
            "printers.duplex",
            P,
            &["off", "long_edge", "short_edge"],
            "off",
        ),
        SettingDef::choice(
            "printers.quality",
            P,
            &["draft", "normal", "high"],
            "normal",
        ),
        SettingDef::flag("printers.greyscale", P, false),
        SettingDef::choice(
            "printers.scan_resolution",
            P,
            &["dpi150", "dpi300", "dpi600"],
            "dpi300",
        ),
    ];
    defs.into_iter().map(|d| d.scope(Scope::System)).collect()
}

pub(super) fn colour() -> Vec<SettingDef> {
    const P: Panel = Panel::Colour;
    vec![
        SettingDef::flag("colour.management", P, true),
        // An ICC profile file; empty = the display's own data (EDID).
        SettingDef::text("colour.profile", P, TextFormat::Path, ""),
        SettingDef::choice(
            "colour.intent",
            P,
            &["perceptual", "relative", "saturation"],
            "perceptual",
        ),
        SettingDef::int("colour.calibration_reminder_days", P, (0, 365, 1), 0),
    ]
}

pub(super) fn accessibility() -> Vec<SettingDef> {
    const P: Panel = Panel::Accessibility;
    let filters = &[
        "none",
        "greyscale",
        "protanopia",
        "deuteranopia",
        "tritanopia",
        "invert",
    ];
    vec![
        // The four switches of the sign-in screen (`login.access`): followed by the shell,
        // the toolkit and the on-screen keyboard (docs/integration/system-services.md).
        SettingDef::flag("accessibility.large_text", P, false),
        SettingDef::flag("accessibility.high_contrast", P, false),
        SettingDef::flag("accessibility.screen_reader", P, false),
        SettingDef::flag("accessibility.on_screen_keyboard", P, false),
        // Seeing.
        SettingDef::flag("accessibility.zoom", P, false),
        SettingDef::int("accessibility.zoom_percent", P, (100, 800, 25), 200),
        SettingDef::int("accessibility.reader_rate", P, (50, 300, 10), 100),
        SettingDef::flag("accessibility.large_pointer", P, false),
        SettingDef::flag("accessibility.reduce_motion", P, false),
        SettingDef::choice("accessibility.colour_filter", P, filters, "none"),
        // Hearing.
        SettingDef::flag("accessibility.visual_alert", P, false),
        SettingDef::flag("accessibility.mono_audio", P, false),
        SettingDef::flag("accessibility.captions", P, false),
        // Typing.
        SettingDef::flag("accessibility.sticky_keys", P, false),
        SettingDef::flag("accessibility.slow_keys", P, false),
        SettingDef::flag("accessibility.bounce_keys", P, false),
        // Pointing.
        SettingDef::flag("accessibility.mouse_keys", P, false),
        SettingDef::flag("accessibility.dwell_click", P, false),
        SettingDef::int("accessibility.double_click_ms", P, (200, 1000, 50), 400),
        SettingDef::flag("accessibility.show_in_bar", P, false),
    ]
}
