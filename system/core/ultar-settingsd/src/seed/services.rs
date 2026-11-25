//! Panels "Sharing", "Users", "Storage", "Updates", "Backup", "Mesh",
//! "Assistant", "Cells and containers" and "Emergency" (specification §8,
//! §12.3). The services behind them follow `settings.changed`.

use crate::schema::{Panel, Risk, Scope, SettingDef, TextFormat};

fn system(defs: Vec<SettingDef>) -> Vec<SettingDef> {
    defs.into_iter().map(|d| d.scope(Scope::System)).collect()
}

pub(super) fn sharing() -> Vec<SettingDef> {
    const P: Panel = Panel::Sharing;
    system(vec![
        // The name other devices in the local network see (host name rules).
        SettingDef::text("sharing.device_name", P, TextFormat::Host, "ultar"),
        SettingDef::flag("sharing.visible", P, true),
        SettingDef::choice("sharing.networks", P, &["private", "all"], "private"),
        SettingDef::flag("sharing.files", P, false),
        SettingDef::list("sharing.files.folders", P, TextFormat::Path, 16),
        SettingDef::flag("sharing.files.read_only", P, true),
        SettingDef::flag("sharing.files.password", P, true),
        SettingDef::flag("sharing.media", P, false),
        // Screen sharing: only the switch until the remote desktop service exists.
        SettingDef::flag("sharing.screen", P, false),
        SettingDef::flag("sharing.ssh", P, false).risk(Risk::AdminOnly),
    ])
}

pub(super) fn users() -> Vec<SettingDef> {
    const P: Panel = Panel::Users;
    let admin = |d: SettingDef| d.risk(Risk::AdminOnly);
    system(vec![
        SettingDef::flag("users.list_on_login", P, true),
        admin(SettingDef::flag("users.guest", P, false)),
        // The user signed in at start without a password; empty = nobody.
        admin(SettingDef::text(
            "users.auto_login",
            P,
            TextFormat::Free { max_chars: 32 },
            "",
        )),
        admin(SettingDef::choice(
            "users.new_kind",
            P,
            &["standard", "administrator"],
            "standard",
        )),
        admin(SettingDef::flag("users.parental", P, false)),
        admin(SettingDef::text(
            "users.parental.from",
            P,
            TextFormat::Time,
            "07:00",
        )),
        admin(SettingDef::text(
            "users.parental.until",
            P,
            TextFormat::Time,
            "21:00",
        )),
    ])
}

pub(super) fn storage() -> Vec<SettingDef> {
    const P: Panel = Panel::Storage;
    system(vec![
        SettingDef::flag("storage.clean_old_updates", P, true),
        SettingDef::flag("storage.clean_downloads", P, false),
        SettingDef::int("storage.clean_downloads_days", P, (1, 365, 1), 30),
        SettingDef::int("storage.low_space_percent", P, (5, 30, 5), 10),
        SettingDef::flag("storage.automount", P, true),
        SettingDef::flag("storage.trim", P, true),
    ])
}

pub(super) fn updates() -> Vec<SettingDef> {
    const P: Panel = Panel::Updates;
    system(vec![
        SettingDef::flag("updates.auto_system", P, true),
        SettingDef::flag("updates.auto_apps", P, true),
        SettingDef::text("updates.window.from", P, TextFormat::Time, "02:00"),
        SettingDef::text("updates.window.until", P, TextFormat::Time, "05:00"),
        SettingDef::choice("updates.channel", P, &["stable", "preview"], "stable")
            .risk(Risk::AdminOnly),
        SettingDef::flag("updates.metered", P, false),
        SettingDef::flag("updates.offline_usb", P, true),
        SettingDef::flag("updates.notify", P, true),
    ])
}

pub(super) fn backup() -> Vec<SettingDef> {
    const P: Panel = Panel::Backup;
    vec![
        SettingDef::flag("backup.enabled", P, false),
        SettingDef::choice(
            "backup.target",
            P,
            &["drive", "network", "account"],
            "drive",
        ),
        // A folder on a drive or the mounted network share.
        SettingDef::text("backup.target_path", P, TextFormat::Path, ""),
        SettingDef::list("backup.folders", P, TextFormat::Path, 32),
        SettingDef::choice(
            "backup.schedule",
            P,
            &["hourly", "daily", "weekly", "manual"],
            "daily",
        ),
        SettingDef::text("backup.time", P, TextFormat::Time, "21:00"),
        SettingDef::flag("backup.encrypted", P, true),
        SettingDef::int("backup.keep_versions", P, (1, 100, 1), 10),
    ]
}

pub(super) fn mesh() -> Vec<SettingDef> {
    const P: Panel = Panel::Mesh;
    vec![
        SettingDef::flag("mesh.enabled", P, false),
        SettingDef::flag("mesh.visible", P, true),
        SettingDef::flag("mesh.share_compute", P, false),
        SettingDef::flag("mesh.share_compute.on_power_only", P, true),
        SettingDef::int("mesh.share_compute.percent", P, (10, 100, 10), 50),
        // Rights per paired device.
        SettingDef::flag("mesh.device.<id>.view", P, true),
        SettingDef::flag("mesh.device.<id>.control", P, false),
        SettingDef::flag("mesh.device.<id>.files", P, false),
        SettingDef::flag("mesh.device.<id>.compute", P, false),
        SettingDef::flag("mesh.device.<id>.sound", P, false),
    ]
}

pub(super) fn assistant() -> Vec<SettingDef> {
    const P: Panel = Panel::Assistant;
    vec![
        // Off until the person switches it on: the model service runs only then
        // (ultar-llm-bus, init.switch).
        SettingDef::flag("assistant.enabled", P, false),
        SettingDef::flag("assistant.wake_word", P, true),
        SettingDef::text(
            "assistant.wake_phrase",
            P,
            TextFormat::Free { max_chars: 32 },
            "Hey KM",
        ),
        SettingDef::choice("assistant.voice", P, &["female", "male"], "female"),
        SettingDef::choice("assistant.model", P, &["small", "medium", "large"], "small"),
        SettingDef::text("assistant.model_place", P, TextFormat::Path, ""),
        SettingDef::flag("assistant.see_screen", P, false),
        SettingDef::flag("assistant.act", P, false),
        SettingDef::flag("assistant.check_links", P, true),
        SettingDef::int("assistant.history_days", P, (0, 365, 1), 30),
    ]
}

pub(super) fn cells() -> Vec<SettingDef> {
    const P: Panel = Panel::Cells;
    system(vec![
        SettingDef::flag("cells.enabled", P, true),
        SettingDef::choice("cells.network", P, &["off", "vpn_only", "open"], "open"),
        SettingDef::flag("cells.gpu", P, true),
        SettingDef::flag("cells.upload_log", P, true),
        SettingDef::int("cells.storage_gb", P, (1, 1024, 1), 16),
        SettingDef::flag("cells.containers", P, false).risk(Risk::AdminOnly),
    ])
}

pub(super) fn emergency() -> Vec<SettingDef> {
    const P: Panel = Panel::Emergency;
    let free = |key: &'static str, max_chars: usize, default: &str| {
        SettingDef::text(key, P, TextFormat::Free { max_chars }, default)
    };
    system(vec![
        free("emergency.number", 16, "112"),
        free("emergency.contact_name", 64, ""),
        free("emergency.contact_phone", 32, ""),
        SettingDef::flag("emergency.on_lock_screen", P, false),
        free("emergency.medical", 500, ""),
        SettingDef::flag("emergency.sos_light", P, true),
        // Three stages with both volume keys (specification §12.3).
        SettingDef::flag("emergency.key_escape", P, true),
        SettingDef::flag("emergency.power_presses", P, false),
    ])
}
