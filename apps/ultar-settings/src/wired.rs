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
    // displayd passes the keyboard helpers and the compose key to the compositor too
    // (`Request::ConfigureKeyHelpers`): sticky, slow, bounce and mouse keys and the compose
    // key in the compositor's key input path.
    ("accessibility.sticky_keys", "compositor"),
    ("accessibility.slow_keys", "compositor"),
    ("accessibility.bounce_keys", "compositor"),
    ("accessibility.mouse_keys", "compositor"),
    ("keyboard.compose_key", "compositor"),
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
    // The launcher resolves a link/file through the default apps (and offers the installed
    // apps that declare each kind as the choices); it starts the autostart list after sign-in.
    ("apps.default.web", "launcher"),
    ("apps.default.mail", "launcher"),
    ("apps.default.calendar", "launcher"),
    ("apps.default.music", "launcher"),
    ("apps.default.video", "launcher"),
    ("apps.default.photos", "launcher"),
    ("apps.default.documents", "launcher"),
    ("apps.autostart", "launcher"),
    // The shell asks, opens Files, or does nothing when a drive with media is plugged in.
    ("apps.removable_media", "shell"),
    // The volume service (ultar-volumed) follows these: it warns once when the data
    // partition runs low and offers a drive with a "Mount" button when automount is off.
    ("storage.low_space_percent", "volumed"),
    ("storage.automount", "volumed"),
    // The launcher owns the person's file areas and cleans their downloads folder once a
    // day (the volume service handles untrusted USB file systems and must not reach them).
    ("storage.clean_downloads", "launcher"),
    ("storage.clean_downloads_days", "launcher"),
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

    /// The panel of a key: its first word (`pointer` for the mouse page).
    fn panel(key: &str) -> &str {
        key.split('.').next().unwrap_or_default()
    }

    #[test]
    fn old_panels_are_untouched_and_wired_keys_belong_to_checked_panels() {
        // The ten panels of the first seed are followed as before.
        assert!(takes_effect("power", "power.mode"));
        for (key, _) in WIRED {
            assert!(CHECKED_PANELS.contains(&panel(key)), "{key}");
            assert!(takes_effect(panel(key), key), "{key}");
        }
    }

    #[test]
    fn these_settings_take_effect() {
        for key in [
            "pointer.speed",
            "pointer.primary_button",
            "pointer.acceleration",
            "pointer.size",
            "accessibility.large_pointer",
            "accessibility.double_click_ms",
            "accessibility.high_contrast",
            "accessibility.reduce_motion",
            // the keyboard helpers reach the compositor's key input path
            "accessibility.sticky_keys",
            "accessibility.slow_keys",
            "accessibility.bounce_keys",
            "accessibility.mouse_keys",
            "keyboard.repeat.delay_ms",
            "keyboard.compose_key",
            "search.source.apps",
            "search.source.characters",
            "search.history",
            "updates.auto_system",
            "updates.window.from",
            "updates.window.until",
            "updates.channel",
            "updates.metered",
            "updates.offline_usb",
            "updates.notify",
            "apps.default.web",
            "apps.default.documents",
            "apps.removable_media",
            "apps.autostart",
            // volumed: low-space warning and automount; the launcher: the downloads clean-up
            "storage.low_space_percent",
            "storage.automount",
            "storage.clean_downloads",
            "storage.clean_downloads_days",
            // the lock screen, the account, session and call services and this app
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
            assert!(takes_effect(panel(key), key), "{key}");
        }
    }

    #[test]
    fn these_still_wait_and_say_so() {
        for key in [
            "sharing.files",
            "sharing.screen",
            "printers.paper",
            // an input layer that tells touchpads apart
            "pointer.touchpad.gestures",
            "pointer.touchpad.tap_to_click",
            "pointer.touchpad.natural_scroll",
            // a pointer helper that is not built yet; no screen reader yet
            "accessibility.dwell_click",
            "accessibility.screen_reader",
            // a per-person file-search service; the catalogue in a shared crate
            "search.source.files",
            "search.file_places",
            "search.source.store",
            // app updates belong to the launcher/Store, not the system updater
            "updates.auto_apps",
            // no FITRIM in the kernel; the updater keeps no superseded images
            "storage.trim",
            "storage.clean_old_updates",
            // per-user desktops (review S-1); the power and volume keys reach no handler
            "users.guest",
            "emergency.power_presses",
            "emergency.key_escape",
        ] {
            assert!(!takes_effect(panel(key), key), "{key}");
        }
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
