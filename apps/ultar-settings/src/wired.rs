//! Which settings of the panels added for DESKTOP-SPEC §8/§12.3 a service puts into effect
//! today. A row whose setting nothing follows yet is shown disabled with the note "not yet
//! available in this version" instead of a switch that only stores a value
//! (docs/integration/settings-wiring.md lists every one and what it waits for).

/// The panels this rule applies to. Of the ten panels of the first seed, the displays and
/// the account are followed by the display service and the account service as a whole; the
/// other eight are checked row by row since 28.11.2026 (many of their rows only stored a
/// value before – now each row either takes effect at once or says that it waits).
pub const CHECKED_PANELS: [&str; 25] = [
    "appearance",
    "date_time",
    "desktop_bar",
    "region",
    "power",
    "windows",
    "notifications",
    "privacy",
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
    // The look of the whole system, at once and without a new start: the shell (bar,
    // sheets, background), the lock screen, the system dialogs and the on-screen keyboard
    // follow these; the launcher hands them to every app it starts (ULTAR_LANG, ULTAR_THEME,
    // ULTAR_ACCENT, ULTAR_FONT_SIZE); this window follows them itself; displayd tells the
    // compositor (title bars, `Request::ConfigureLook`) the language and the theme.
    (
        "region.language",
        "shell + lock + dialogs + launcher + compositor + settings",
    ),
    (
        "appearance.theme",
        "shell + lock + dialogs + launcher + compositor + settings",
    ),
    (
        "appearance.accent_color",
        "shell + lock + dialogs + launcher + settings",
    ),
    (
        "appearance.font_size",
        "shell + lock + dialogs + launcher + settings",
    ),
    // The input languages of the sign-in and the bar (the compositor's layouts).
    ("region.keyboard.layouts", "shell + lock"),
    ("region.keyboard.active", "shell + lock"),
    // The clock of the bar and of the lock screen: the zone (with summer time), 12/24 h,
    // seconds, weekday, date and the Hijri date.
    ("datetime.timezone.name", "shell + lock"),
    ("datetime.clock_format", "shell + lock"),
    ("bar.clock.show_seconds", "shell"),
    ("bar.clock.show_weekday", "shell"),
    ("bar.clock.show_date", "shell"),
    ("bar.clock.show_hijri", "shell"),
    // The bar: bottom or top with a dock, the tiles centred or at the start, the trash and
    // the drives, the battery level beside the status icons.
    ("bar.layout", "shell"),
    ("bar.icon_alignment", "shell"),
    ("bar.show_trash", "shell"),
    ("bar.show_volumes", "shell"),
    ("power.show_battery_percent", "shell"),
    // The app shortcuts on the desktop background: shown, their size, their corner.
    ("desktop.icons.enabled", "shell"),
    ("desktop.icons.size", "shell"),
    ("desktop.icons.start_corner", "shell"),
    // The power mode (quick settings), followed by displayd and the updater as well.
    ("power.mode", "shell + displayd + updated"),
    // Do not disturb: the shell shows no banners.
    ("notifications.do_not_disturb", "shell"),
    // The phone shell follows the location switch (the desktop has no location source).
    ("privacy.location", "phone shell"),
    // The on-screen keyboard (ultar-osk) follows these: when it appears (automatic without
    // a hardware keyboard, always, never; the accessibility switch shows it always), word
    // suggestions and autocorrect.
    ("keyboard.on_screen", "osk"),
    ("keyboard.suggestions", "osk"),
    ("keyboard.autocorrect", "osk"),
    ("accessibility.on_screen_keyboard", "osk"),
    // ultar-llm-bus starts and stops the model service with it (`init.switch`) and lets
    // the largest installed model of the chosen size answer (`assistant.model`).
    ("assistant.enabled", "llm-bus"),
    ("assistant.model", "llm-bus"),
    // ultar-llm-bus checks the folder and passes it to the model service, which reads it
    // with every model search (`/run/ultar/llmbus/model-place`).
    ("assistant.model_place", "llm-bus + llm"),
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
    // displayd passes the accessibility view to the compositor as well
    // (`Request::ConfigureVisual`): the magnifier scales the output around the
    // pointer, the colour filter is a final colour matrix over every pixel.
    ("accessibility.zoom", "compositor"),
    ("accessibility.zoom_percent", "compositor"),
    ("accessibility.colour_filter", "compositor"),
    // calld (right `sound`) writes it to the kernel's switch /dev/ultar/pcm-mono; the kernel
    // sums both channels in the card's output path for every writer (browser, calls).
    ("accessibility.mono_audio", "calld + kernel"),
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
    // The shell flashes the screen when a notification alerts (visual_alert,
    // through the compositor's `Request::VisualAlert`) and keeps an accessibility
    // button in the quick settings that opens this page (show_in_bar).
    ("accessibility.visual_alert", "shell"),
    ("accessibility.show_in_bar", "shell"),
    // The search of the shell's overview (model/search.rs `Sources`, read at start and on
    // settings.changed); the characters come from the shell's own table (model/characters.rs),
    // the history is kept beside the shell per person (model/history.rs); off deletes it.
    ("search.source.apps", "shell"),
    ("search.source.settings", "shell"),
    ("search.source.calculator", "shell"),
    ("search.source.commands", "shell"),
    ("search.source.characters", "shell"),
    // Store results: the Store publishes its verified catalogue in its publish directory
    // (launcher `publishers`), the shell reads it and checks the signature again.
    ("search.source.store", "shell + Store"),
    ("search.history", "shell"),
    ("search.arabic_variants", "shell"),
    // The Store (apps/ultar-store) follows updates.auto_apps: it installs available app
    // updates by itself under the shared updates.metered / updates.window.* rules (the
    // system updater installs only system images). Off = only offer updates.
    ("updates.auto_apps", "store"),
    // The update service (ultar-updated) maps these onto its policy under the ceiling of
    // /etc/ultar/update.conf, notifies through notifyd and offers updates from a plugged-in
    // drive.
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

    /// The panel of a key: its first word (`pointer` for the mouse page), with the panels
    /// whose id differs from the words of their keys.
    fn panel(key: &str) -> &str {
        match key.split('.').next().unwrap_or_default() {
            "datetime" => "date_time",
            "bar" | "dock" | "desktop" => "desktop_bar",
            "workspaces" => "windows",
            word => word,
        }
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

    /// Settings of the checked panels whose service follows them.
    const TAKING_EFFECT: &[&str] = &[
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
        // the accessibility view: magnifier and colour filter in the compositor,
        // the visual alert and the bar button in the shell
        "accessibility.zoom",
        "accessibility.zoom_percent",
        "accessibility.colour_filter",
        "accessibility.mono_audio",
        "accessibility.visual_alert",
        "accessibility.show_in_bar",
        "keyboard.repeat.delay_ms",
        "keyboard.compose_key",
        // the on-screen keyboard
        "keyboard.on_screen",
        "keyboard.suggestions",
        "keyboard.autocorrect",
        "accessibility.on_screen_keyboard",
        "search.source.apps",
        "search.source.characters",
        "search.source.store",
        "search.history",
        // the Store installs app updates by itself
        "updates.auto_apps",
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
    ];

    #[test]
    fn these_settings_take_effect() {
        for key in TAKING_EFFECT {
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
            // a per-person file-search service
            "search.source.files",
            "search.file_places",
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
    fn the_first_panels_say_which_rows_take_effect() {
        for key in [
            "region.language",
            "appearance.theme",
            "appearance.accent_color",
            "appearance.font_size",
            "datetime.timezone.name",
            "datetime.clock_format",
            "bar.clock.show_hijri",
            "bar.icon_alignment",
            "power.show_battery_percent",
            "notifications.do_not_disturb",
            "desktop.icons.enabled",
            "desktop.icons.size",
            "desktop.icons.start_corner",
        ] {
            assert!(takes_effect(panel(key), key), "{key}");
        }
        // nothing follows these yet: the row says so instead of only storing a value
        for key in [
            "appearance.wallpaper.custom_path",
            "appearance.icon_size",
            "region.format.date",
            "power.screen_off_after",
            "power.lid_action",
            "windows.edge_snap",
            "workspaces.count",
            "privacy.camera",
            "notifications.dnd_schedule.enabled",
            "bar.autohide",
            "dock.size",
            "datetime.automatic",
        ] {
            assert!(!takes_effect(panel(key), key), "{key}");
        }
        // the displays and the account are followed as a whole
        assert!(takes_effect("display", "display.night_light.schedule"));
        assert!(takes_effect("account", "account.server"));
    }

    #[test]
    fn the_assistant_and_keyboard_pages_have_their_consumers() {
        assert!(takes_effect("keyboard", "keyboard.switch_shortcut"));
        assert!(takes_effect("keyboard", "keyboard.per_window"));
        assert!(takes_effect("keyboard", "keyboard.input_languages"));
        assert!(takes_effect("assistant", "assistant.model"));
        assert!(takes_effect("assistant", "assistant.see_screen"));
        assert!(takes_effect("assistant", "assistant.check_links"));
        assert!(takes_effect("assistant", "assistant.model_place"));
        // No speech component yet.
        assert!(!takes_effect("assistant", "assistant.voice"));
        assert!(!takes_effect("assistant", "assistant.wake_word"));
        assert!(!takes_effect("assistant", "assistant.wake_phrase"));
    }
}
