//! Panels "Notifications", "Power", "Region and language", "Date and
//! time", "Privacy and security" and "Ultar account" (specification §8
//! and §12.1).

use super::CONFIRM_SECONDS;
use crate::schema::{Panel, Risk, Scope, SettingDef, TextFormat};
use ultar_profile::Capability;

pub(super) fn notifications() -> Vec<SettingDef> {
    const P: Panel = Panel::Notifications;
    vec![
        SettingDef::flag("notifications.do_not_disturb", P, false),
        SettingDef::flag("notifications.dnd_schedule.enabled", P, false),
        SettingDef::text(
            "notifications.dnd_schedule.start",
            P,
            TextFormat::Time,
            "22:00",
        ),
        SettingDef::text(
            "notifications.dnd_schedule.end",
            P,
            TextFormat::Time,
            "07:00",
        ),
        SettingDef::flag("notifications.lock_screen.show", P, true),
        SettingDef::flag("notifications.lock_screen.show_content", P, false),
        SettingDef::flag("notifications.app.<id>.enabled", P, true),
        SettingDef::flag("notifications.app.<id>.banner", P, true),
        SettingDef::flag("notifications.app.<id>.sound", P, true),
        SettingDef::flag("notifications.app.<id>.lock_screen", P, true),
        SettingDef::flag("notifications.app.<id>.important", P, false),
    ]
}

pub(super) fn power() -> Vec<SettingDef> {
    const P: Panel = Panel::Power;
    let battery = &[Capability::Battery];
    let button = &["menu", "suspend", "shutdown", "nothing"];
    let defs = vec![
        SettingDef::choice(
            "power.mode",
            P,
            &["performance", "balanced", "saver"],
            "balanced",
        ),
        SettingDef::flag("power.charge_limit.enabled", P, false).needs(battery),
        SettingDef::int("power.charge_limit.percent", P, (50, 100, 5), 80).needs(battery),
        // Seconds; 0 = never.
        SettingDef::int("power.screen_dim_after", P, (0, 3600, 1), 120),
        SettingDef::int("power.screen_off_after", P, (0, 7200, 1), 300),
        SettingDef::int("power.suspend_after_ac", P, (0, 86_400, 1), 1800),
        SettingDef::int("power.suspend_after_battery", P, (0, 86_400, 1), 900).needs(battery),
        SettingDef::choice("power.button_action", P, button, "menu"),
        SettingDef::choice(
            "power.lid_action",
            P,
            &["suspend", "lock", "nothing"],
            "suspend",
        ),
        SettingDef::flag("power.auto_saver.enabled", P, true).needs(battery),
        SettingDef::int("power.auto_saver.percent", P, (5, 50, 5), 20).needs(battery),
    ];
    let mut defs: Vec<SettingDef> = defs.into_iter().map(|d| d.scope(Scope::System)).collect();
    // What the bar shows is the user's own choice.
    defs.push(SettingDef::flag("power.show_battery_percent", P, false).needs(battery));
    defs
}

pub(super) fn region() -> Vec<SettingDef> {
    const P: Panel = Panel::Region;
    let weekdays = &["saturday", "sunday", "monday"];
    vec![
        // Arabic first.
        SettingDef::choice("region.language", P, &["ar", "en", "de"], "ar"),
        SettingDef::choice("region.format.date", P, &["dmy", "mdy", "ymd"], "dmy"),
        SettingDef::choice("region.format.numbers", P, &["point", "comma"], "point"),
        SettingDef::text("region.format.currency", P, TextFormat::Currency, "SYP"),
        SettingDef::choice(
            "region.format.measurement",
            P,
            &["metric", "imperial"],
            "metric",
        ),
        SettingDef::choice("region.format.paper", P, &["a4", "letter"], "a4"),
        SettingDef::choice("region.first_weekday", P, weekdays, "saturday"),
        SettingDef::choice(
            "region.calendar",
            P,
            &["gregorian", "gregorian_hijri"],
            "gregorian_hijri",
        ),
        SettingDef::choice(
            "region.month_names",
            P,
            &["levantine", "alternative"],
            "levantine",
        ),
        // Input languages (DESKTOP-SPEC §8 "Tastatur"): the layouts Ctrl+Shift, Alt+Shift and
        // Super+Space switch through, Arabic first; the active one is stored by the shell.
        SettingDef::choice(
            "region.keyboard.layouts",
            P,
            &["ar_en", "ar_de", "ar_en_de"],
            "ar_en",
        ),
        SettingDef::choice("region.keyboard.active", P, &["ar", "en", "de"], "ar"),
    ]
}

pub(super) fn date_time() -> Vec<SettingDef> {
    const P: Panel = Panel::DateTime;
    let system = |def: SettingDef| def.scope(Scope::System).risk(Risk::AdminOnly);
    vec![
        system(SettingDef::flag("datetime.automatic", P, true)),
        system(SettingDef::flag("datetime.timezone.automatic", P, true)),
        system(SettingDef::text(
            "datetime.timezone.name",
            P,
            TextFormat::TimeZone,
            "Asia/Damascus",
        )),
        SettingDef::choice("datetime.clock_format", P, &["h24", "h12"], "h24"),
        system(SettingDef::choice(
            "datetime.hardware_clock",
            P,
            &["utc", "local"],
            "utc",
        )),
    ]
}

pub(super) fn privacy() -> Vec<SettingDef> {
    const P: Panel = Panel::Privacy;
    let switches = vec![
        // Master switches.
        SettingDef::flag("privacy.camera", P, true).needs(&[Capability::Camera]),
        SettingDef::flag("privacy.microphone", P, true).needs(&[Capability::AudioIn]),
        SettingDef::flag("privacy.location", P, false).needs(&[Capability::Gnss]),
        SettingDef::flag("privacy.sensors", P, true).needs(&[Capability::Sensors]),
        // Cutting the network is risky: it asks "keep?" like a mode change.
        SettingDef::flag("privacy.network", P, true).risk(Risk::ConfirmRevert {
            seconds: CONFIRM_SECONDS,
        }),
        SettingDef::flag("privacy.tracker_blocking", P, true),
        SettingDef::flag("privacy.diagnostics", P, false),
        SettingDef::flag("privacy.allow_unsigned_packages", P, false).risk(Risk::AdminOnly),
    ];
    let mut defs: Vec<SettingDef> = switches
        .into_iter()
        .map(|d| d.scope(Scope::System))
        .collect();
    defs.extend([
        // Seconds; 0 = never.
        SettingDef::int("privacy.lock.after", P, (0, 3600, 1), 300),
        SettingDef::flag("privacy.lock.on_suspend", P, true),
        SettingDef::flag("privacy.file_history", P, true),
        SettingDef::flag("privacy.auto_clean.trash", P, false),
        SettingDef::flag("privacy.auto_clean.temp", P, false),
        SettingDef::int("privacy.auto_clean.after_days", P, (1, 365, 1), 30),
    ]);
    defs
}

pub(super) fn account() -> Vec<SettingDef> {
    const P: Panel = Panel::Account;
    vec![
        // "Accept Ultar calls" is off by default (specification §12.1).
        SettingDef::flag("account.calls", P, false),
        SettingDef::flag("account.vpn", P, false),
        SettingDef::flag("account.store_purchases", P, false),
        SettingDef::flag("account.backup", P, false),
        SettingDef::flag("account.mesh_over_internet", P, false),
        SettingDef::text(
            "account.server",
            P,
            TextFormat::Host,
            "ultar.code-craft.tech",
        )
        .scope(Scope::System)
        .risk(Risk::AdminOnly),
    ]
}
