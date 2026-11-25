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
pub const WIRED: &[(&str, &str)] = &[
    // ultar-llm-bus starts and stops the model service with it (`init.switch`) and lets
    // the largest installed model of the chosen size answer (`assistant.model`).
    ("assistant.enabled", "llm-bus"),
    ("assistant.model", "llm-bus"),
    // The helper (ultar-agentd) follows these: whether it acts, which areas it reaches,
    // how long the list of what it did is kept, whether it may see the screen when asked
    // (with the person's consent in the system dialog, through the capture service), and
    // whether suspicious links in the model's answers are warned about.
    ("assistant.act", "agentd"),
    ("assistant.see_screen", "agentd"),
    ("assistant.check_links", "agentd"),
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
    // displayd passes these to the compositor too (`Request::ConfigurePointer`): the
    // button swap, the acceleration profile, the cursor size (a large pointer forces at
    // least `large`) and the double-click time, which the shell and this window follow
    // in their own widgets as well.
    ("pointer.primary_button", "compositor"),
    ("pointer.acceleration", "compositor"),
    ("pointer.size", "compositor"),
    ("accessibility.large_pointer", "compositor"),
    ("accessibility.double_click_ms", "compositor"),
    ("keyboard.repeat.enabled", "compositor"),
    ("keyboard.repeat.delay_ms", "compositor"),
    ("keyboard.repeat.per_second", "compositor"),
    // displayd passes these to the compositor too (`Request::ConfigureKeyboard`): the keys
    // that switch the input language and whether every window keeps its own.
    ("keyboard.switch_shortcut", "compositor"),
    ("keyboard.per_window", "compositor"),
    // The shell gives the compositor the input languages of the Keyboard page (they win
    // over the region's layouts when there are any); the sign-in reads the same list.
    ("keyboard.input_languages", "shell"),
    // The shell (bar, sheets, lock screen) and this window follow these.
    ("accessibility.large_text", "shell"),
    ("accessibility.high_contrast", "shell"),
    ("accessibility.reduce_motion", "shell"),
    // The search of the shell's overview (model/search.rs `Sources`, read at start and on
    // settings.changed); the characters come from the shell's own table (model/characters.rs),
    // the history is kept beside the shell per person (model/history.rs); off deletes it.
    ("search.source.apps", "shell"),
    ("search.source.settings", "shell"),
    ("search.source.calculator", "shell"),
    ("search.source.commands", "shell"),
    ("search.source.characters", "shell"),
    ("search.history", "shell"),
    ("search.arabic_variants", "shell"),
    // The update service (ultar-updated) maps these onto its policy under the ceiling of
    // /etc/ultar/update.conf, notifies through notifyd and offers updates from a plugged-in
    // drive. updates.auto_apps stays out: app updates belong to the launcher/Store.
    ("updates.auto_system", "updated"),
    ("updates.window.from", "updated"),
    ("updates.window.until", "updated"),
    ("updates.channel", "updated"),
    ("updates.metered", "updated"),
    ("updates.offline_usb", "updated"),
    ("updates.notify", "updated"),
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
        assert!(takes_effect("pointer", "pointer.primary_button"));
        assert!(takes_effect("pointer", "pointer.acceleration"));
        assert!(takes_effect("pointer", "pointer.size"));
        assert!(takes_effect("accessibility", "accessibility.large_pointer"));
        assert!(takes_effect(
            "accessibility",
            "accessibility.double_click_ms"
        ));
        assert!(takes_effect("accessibility", "accessibility.high_contrast"));
        assert!(!takes_effect("sharing", "sharing.files"));
        assert!(!takes_effect("sharing", "sharing.screen"));
        // The touchpad options wait for an input layer that tells touchpads apart.
        assert!(!takes_effect("pointer", "pointer.touchpad.gestures"));
        assert!(!takes_effect("pointer", "pointer.touchpad.tap_to_click"));
        assert!(!takes_effect("pointer", "pointer.touchpad.natural_scroll"));
        assert!(takes_effect("keyboard", "keyboard.repeat.delay_ms"));
        assert!(!takes_effect("keyboard", "keyboard.compose_key"));
        assert!(takes_effect("search", "search.source.apps"));
        assert!(takes_effect("search", "search.source.characters"));
        assert!(takes_effect("search", "search.history"));
        assert!(!takes_effect("search", "search.source.files"));
        assert!(!takes_effect("search", "search.source.store"));
        assert!(!takes_effect("search", "search.file_places"));
        assert!(takes_effect("updates", "updates.auto_system"));
        assert!(takes_effect("updates", "updates.window.from"));
        assert!(takes_effect("updates", "updates.window.until"));
        assert!(takes_effect("updates", "updates.channel"));
        assert!(takes_effect("updates", "updates.metered"));
        assert!(takes_effect("updates", "updates.offline_usb"));
        assert!(takes_effect("updates", "updates.notify"));
        // App updates belong to the launcher/Store, not the system updater.
        assert!(!takes_effect("updates", "updates.auto_apps"));
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

    #[test]
    fn the_assistant_and_keyboard_pages_have_their_consumers() {
        assert!(takes_effect("keyboard", "keyboard.switch_shortcut"));
        assert!(takes_effect("keyboard", "keyboard.per_window"));
        assert!(takes_effect("keyboard", "keyboard.input_languages"));
        // The on-screen keyboard program is not on this branch (build/phonekb).
        assert!(!takes_effect("keyboard", "keyboard.on_screen"));
        assert!(!takes_effect("keyboard", "keyboard.suggestions"));
        assert!(!takes_effect("keyboard", "keyboard.autocorrect"));
        assert!(!takes_effect(
            "accessibility",
            "accessibility.on_screen_keyboard"
        ));
        assert!(takes_effect("assistant", "assistant.model"));
        assert!(takes_effect("assistant", "assistant.see_screen"));
        assert!(takes_effect("assistant", "assistant.check_links"));
        // No speech component and no runtime model-folder contract yet.
        assert!(!takes_effect("assistant", "assistant.model_place"));
        assert!(!takes_effect("assistant", "assistant.voice"));
        assert!(!takes_effect("assistant", "assistant.wake_word"));
        assert!(!takes_effect("assistant", "assistant.wake_phrase"));
    }
}
