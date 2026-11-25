//! The Storage page checks (included as `wired::storage`): the page shows what the volume
//! service measured, and the volume service follows the low-space warning, automount and
//! the downloads clean-up – each change reaches it with its own identity and its own code
//! turns the change into the effect.

use super::super::{click, setup::Run, texts};
use super::{change, control, page, whole, Follower, WAIT};
use ultar_bus::messages::settings::{Get, SettingValue};
use ultar_bus::Client;
use ultar_settings::App;
use ultar_ui::event::{Event, Key, Modifiers};

/// The page shows the size the kernel reports for this device's storage, as the volume
/// service measured it.
pub(super) fn storage(bus: &Client, run: &Run) -> Result<(), String> {
    let own = if std::path::Path::new("/data").is_dir() {
        "/data"
    } else {
        "/"
    };
    let stat = rustix::fs::statvfs(own).map_err(|e| format!("statvfs {e}"))?;
    let total = stat.f_blocks.saturating_mul(stat.f_frsize);
    let mut app = page(bus, run, "storage")?;
    let size = ultar_i18n::units::format_size(total, app.ui.translator());
    let shown = texts(&mut app);
    let this = app.ui.translator().t("app.settings.storage.internal");
    if !shown.iter().any(|t| *t == this) || !shown.iter().any(|t| t.contains(&size)) {
        return Err(format!(
            "storage page without {size} of this device: {shown:?}"
        ));
    }
    println!("SETTINGS: storage page shows the volume service's measurement ({size})");
    Ok(())
}

/// The volume service's low-space hysteresis at `threshold`: warn once just below, stay
/// quiet while still below, re-arm at the threshold, warn again on the next drop.
fn low_space_hysteresis(threshold: u8) -> Result<(), String> {
    let mut jobs = ultar_volumed::jobs::Jobs::default();
    let free = u64::from(threshold).saturating_sub(1);
    if jobs.low_space(100, free, threshold).is_none()
        || jobs.low_space(100, free, threshold).is_some()
        || jobs
            .low_space(100, u64::from(threshold), threshold)
            .is_some()
        || jobs.low_space(100, free, threshold).is_none()
    {
        return Err(format!("low-space hysteresis at {threshold}%"));
    }
    Ok(())
}

/// Types `7` into the days row (365 steps: a text field, not a slider), stores it on Enter
/// and confirms the volume service was told and follows it.
fn set_clean_days(
    app: &mut App,
    admin: &Client,
    volumed: &Follower,
    s: &mut ultar_volumed::settings::StorageSettings,
) -> Result<(), String> {
    use ultar_volumed::settings::CLEAN_DAYS;
    let node = control(app, "storage", CLEAN_DAYS)?;
    click(app, node)?;
    let key = |k| Event::Key {
        key: k,
        modifiers: Modifiers::NONE,
    };
    app.handle(Event::Key {
        key: Key::Char('a'),
        modifiers: Modifiers {
            ctrl: true,
            ..Modifiers::NONE
        },
    })
    .map_err(|e| format!("{e:?}"))?;
    app.handle(key(Key::Char('7')))
        .map_err(|e| format!("{e:?}"))?;
    app.handle(key(Key::Enter)).map_err(|e| format!("{e:?}"))?;
    let days = admin
        .call(
            &Get {
                key: CLEAN_DAYS.into(),
            },
            WAIT,
        )
        .map_err(|e| format!("get {e}"))?
        .value
        .ok_or("clean_downloads_days: no value")?;
    volumed.told(CLEAN_DAYS, &days)?;
    s.on_changed(CLEAN_DAYS, &days);
    (s.clean_downloads_days == 7)
        .then_some(())
        .ok_or(format!("days {:?}", s.clean_downloads_days))
}

/// Runs the service's own clean-up on a temp folder: the person's old download goes, the
/// fresh one stays.
fn clean_check(run: &Run, days: u16) -> Result<(), String> {
    let dir = run.at("downloads-check");
    std::fs::create_dir_all(&dir).map_err(|e| format!("downloads {e}"))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "clock")?
        .as_secs();
    let (old, fresh) = (dir.join("old.bin"), dir.join("fresh.bin"));
    std::fs::write(&old, b"old").map_err(|e| format!("{e}"))?;
    std::fs::write(&fresh, b"fresh").map_err(|e| format!("{e}"))?;
    let age = u64::from(days) * 86_400 + 86_400;
    let long_ago = std::time::UNIX_EPOCH + std::time::Duration::from_secs(now.saturating_sub(age));
    std::fs::File::options()
        .write(true)
        .open(&old)
        .map_err(|e| format!("{e}"))?
        .set_modified(long_ago)
        .map_err(|e| format!("{e}"))?;
    let me = rustix::process::getuid().as_raw();
    let removed = ultar_volumed::clean::clean_downloads(&dir, me, days, now);
    if removed != 1 || old.exists() || !fresh.exists() {
        return Err(format!(
            "clean-up removed {removed} (old {}, fresh {})",
            old.exists(),
            fresh.exists()
        ));
    }
    Ok(())
}

/// The volume service follows the low-space warning, automount and the downloads clean-up.
pub(super) fn storage_settings(
    bus: &Client,
    admin: &Client,
    run: &Run,
    volumed: &Follower,
) -> Result<(), String> {
    use ultar_volumed::jobs::{drive_post, MOUNT_ACTION};
    use ultar_volumed::settings::{StorageSettings, AUTOMOUNT, CLEAN_DOWNLOADS, LOW_SPACE};

    let mut app = page(bus, run, "storage")?;
    let mut s = StorageSettings::default();
    let follow = |s: &mut StorageSettings, key: &str, value: SettingValue| {
        volumed.told(key, &value)?;
        s.on_changed(key, &value);
        Ok::<(), String>(())
    };

    let low = change((&mut app, admin), ("storage", LOW_SPACE), Some(0.9))?;
    follow(&mut s, LOW_SPACE, low.clone())?;
    let threshold = u8::try_from(whole(LOW_SPACE, &low)?).map_err(|_| "low_space_percent")?;
    low_space_hysteresis(threshold)?;

    // Automount off: a drive is offered with a "Mount" button instead of mounted.
    let off = change((&mut app, admin), ("storage", AUTOMOUNT), None)?;
    follow(&mut s, AUTOMOUNT, off)?;
    let offer = drive_post("STICK");
    if s.automount || offer.actions.len() != 1 || offer.actions[0].id != MOUNT_ACTION {
        return Err(format!("automount off offer {:?}", offer.actions));
    }

    // Clean-up on with a shorter window: the person's old download goes, the fresh stays.
    let on = change((&mut app, admin), ("storage", CLEAN_DOWNLOADS), None)?;
    follow(&mut s, CLEAN_DOWNLOADS, on)?;
    set_clean_days(&mut app, admin, volumed, &mut s)?;
    if !s.clean_downloads {
        return Err("the clean-up is still off".into());
    }
    clean_check(run, s.clean_downloads_days)?;
    println!(
        "SETTINGS: storage page -> volume service: low-space {threshold}%, automount off \
         (Mount offer), downloads cleaned after {} days",
        s.clean_downloads_days
    );
    Ok(())
}
