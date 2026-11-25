//! Which settings of the panels added for DESKTOP-SPEC §8/§12.3 a service puts into effect
//! today. A row whose setting nothing follows yet is shown disabled with the note "not yet
//! available in this version" instead of a switch that only stores a value
//! (docs/integration/settings-wiring.md lists every one and what it waits for).

/// The panels this rule applies to (the ten panels of the first seed are followed by the
/// shell, the display service and the session as before).
pub const CHECKED_PANELS: [&str; 17] = [
    "mobile",
    "search",
    "apps",
    "pointer",
    "keyboard",
    "printers",
    "colour",
    "accessibility",
    "sharing",
    "users",
    "storage",
    "updates",
    "backup",
    "mesh",
    "assistant",
    "cells",
    "emergency",
];

/// Settings of those panels with an effect: who follows them.
pub const WIRED: [(&str, &str); 14] = [
    // ultar-llm-bus starts and stops the model service with it (`init.switch`).
    ("assistant.enabled", "llm-bus"),
    // displayd passes these to the compositor (`Request::ConfigurePointer`,
    // `Request::ConfigureKeyRepeat`).
    ("pointer.speed", "compositor"),
    ("pointer.natural_scroll", "compositor"),
    ("keyboard.repeat.enabled", "compositor"),
    ("keyboard.repeat.delay_ms", "compositor"),
    ("keyboard.repeat.per_second", "compositor"),
    // The shell (bar, sheets, lock screen) and this window follow these.
    ("accessibility.large_text", "shell"),
    ("accessibility.high_contrast", "shell"),
    ("accessibility.reduce_motion", "shell"),
    // The search of the shell's overview.
    ("search.source.apps", "shell"),
    ("search.source.settings", "shell"),
    ("search.source.calculator", "shell"),
    ("search.source.commands", "shell"),
    ("search.arabic_variants", "shell"),
];

/// `true` when changing `key` of `panel` has an effect in this version.
pub fn takes_effect(panel: &str, key: &str) -> bool {
    !CHECKED_PANELS.contains(&panel) || WIRED.iter().any(|(wired, _)| *wired == key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_panels_are_untouched_and_new_ones_need_a_consumer() {
        assert!(takes_effect("power", "power.mode"));
        assert!(takes_effect("pointer", "pointer.speed"));
        assert!(takes_effect("accessibility", "accessibility.high_contrast"));
        assert!(!takes_effect("sharing", "sharing.files"));
        assert!(!takes_effect("sharing", "sharing.screen"));
        assert!(!takes_effect("pointer", "pointer.touchpad.gestures"));
        assert!(takes_effect("keyboard", "keyboard.repeat.delay_ms"));
        assert!(!takes_effect("keyboard", "keyboard.compose_key"));
        assert!(takes_effect("search", "search.source.apps"));
        assert!(!takes_effect("search", "search.source.files"));
        assert!(takes_effect("accessibility", "accessibility.reduce_motion"));
        assert!(!takes_effect(
            "accessibility",
            "accessibility.screen_reader"
        ));
        // Every wired key belongs to a checked panel (its first word, `pointer` for the
        // mouse page).
        for (key, _) in WIRED {
            let panel = key.split('.').next().unwrap_or_default();
            assert!(CHECKED_PANELS.contains(&panel), "{key}");
        }
        assert!(!takes_effect("printers", "printers.paper"));
    }
}
