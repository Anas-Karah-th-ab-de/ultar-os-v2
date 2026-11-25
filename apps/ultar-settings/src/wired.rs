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
pub const WIRED: [(&str, &str); 37] = [
    // ultar-llm-bus starts and stops the model service with it (`init.switch`).
    ("assistant.enabled", "llm-bus"),
    // The helper (ultar-agentd) follows these: whether it acts, which areas it reaches,
    // and how long the list of what it did is kept.
    ("assistant.act", "agentd"),
    ("assistant.reach.apps", "agentd"),
    ("assistant.reach.settings", "agentd"),
    ("assistant.reach.network", "agentd"),
    ("assistant.reach.sound", "agentd"),
    ("assistant.reach.screen", "agentd"),
    ("assistant.reach.notifications", "agentd"),
    ("assistant.reach.storage", "agentd"),
    ("assistant.reach.devices", "agentd"),
    ("assistant.reach.power", "agentd"),
    ("assistant.history_days", "agentd"),
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
    // The lock screen (ultar-lock) follows these: the user list at the sign-in,
    // automatic sign-in at a fresh start, the parental time window (the session
    // service ends open sessions at its end), and the emergency panel on the cover
    // with its call (the call service dials the number itself) and SOS light.
    ("users.list_on_login", "lock"),
    ("users.auto_login", "accountd + lock"),
    ("users.parental", "lock + sessiond"),
    ("users.parental.from", "lock + sessiond"),
    ("users.parental.until", "lock + sessiond"),
    ("emergency.on_lock_screen", "lock"),
    ("emergency.contact_name", "lock"),
    ("emergency.contact_phone", "lock"),
    ("emergency.medical", "lock"),
    ("emergency.number", "calld + lock"),
    ("emergency.sos_light", "lock"),
    // The Users page of this app proposes the configured kind when adding a user.
    ("users.new_kind", "settings"),
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
        // Users and Emergency: what the lock screen, the account service, the session
        // service, the call service and this app follow.
        for key in [
            "users.list_on_login",
            "users.auto_login",
            "users.new_kind",
            "users.parental",
            "users.parental.from",
            "users.parental.until",
            "emergency.on_lock_screen",
            "emergency.contact_name",
            "emergency.contact_phone",
            "emergency.medical",
            "emergency.number",
            "emergency.sos_light",
        ] {
            let panel = key.split('.').next().unwrap_or_default();
            assert!(takes_effect(panel, key), "{key}");
        }
        // Still waiting: a guest session needs per-user desktops (review S-1), the
        // power key and both volume keys never reach a system handler on this branch.
        assert!(!takes_effect("users", "users.guest"));
        assert!(!takes_effect("emergency", "emergency.power_presses"));
        assert!(!takes_effect("emergency", "emergency.key_escape"));
    }
}
