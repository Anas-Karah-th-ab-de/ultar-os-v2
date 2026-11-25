//! The Store-side wired setting `updates.auto_apps` (the Store installs available app updates
//! by itself). `search.source.store` is checked with the shell (`search_sources`). Split out of
//! `wired.rs` to keep both files within the size limit.

use super::{change, page, Follower, Run};
use ultar_bus::Client;
use ultar_store::settings::AppUpdateSettings;

/// Updates page: the Store follows updates.auto_apps. Its own code turns the change into the
/// decision (automatic app updates allowed in the window on an unmetered connection, blocked
/// when off or metered). The Store identity is told the change over the real bus.
pub(crate) fn auto_apps(bus: &Client, admin: &Client, run: &Run) -> Result<(), String> {
    let store = Follower::connect(run, "the Store", "store")?;
    let mut s = AppUpdateSettings::default();
    // 03:00 UTC on a fixed day: inside the default 02:00–05:00 window.
    let now = 86_400 * 3 + 3 * 3600;
    if !s.allowed_now(now, Some(false)) {
        return Err("by default automatic app updates run in the window".into());
    }
    let mut app = page(bus, run, "updates")?;
    let off = change((&mut app, admin), ("updates", "updates.auto_apps"), None)?;
    store.told("updates.auto_apps", &off)?;
    if !s.take("updates.auto_apps", &off) {
        return Err("the Store ignores updates.auto_apps".into());
    }
    if s.allowed_now(now, Some(false)) {
        return Err("automatic app updates still run after switching them off".into());
    }
    let on = change((&mut app, admin), ("updates", "updates.auto_apps"), None)?;
    store.told("updates.auto_apps", &on)?;
    s.take("updates.auto_apps", &on);
    if !s.allowed_now(now, Some(false)) {
        return Err("automatic app updates do not run after switching them on".into());
    }
    // A metered connection blocks them (updates.metered is off by default).
    if s.allowed_now(now, Some(true)) {
        return Err("a metered connection does not block automatic app updates".into());
    }
    println!(
        "SETTINGS: updates page -> Store: automatic app updates off then on; \
         a metered connection blocks them"
    );
    Ok(())
}
