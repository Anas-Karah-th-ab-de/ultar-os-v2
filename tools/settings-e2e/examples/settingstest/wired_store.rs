//! The Store-side wired settings: `search.source.store` (the shell's overview offers catalogue
//! apps from a signature-checked cache) and `updates.auto_apps` (the Store installs available
//! app updates by itself). Split out of `wired.rs` to keep both files within the size limit.

use super::{change, page, Follower, Run};
use ed25519_dalek::SigningKey;
use ultar_bus::Client;
use ultar_catalog::sign::sign_catalog;
use ultar_catalog::source::FileCatalogSource;
use ultar_desktop::app::desktop::link::apply_setting;
use ultar_desktop::model::search::{search_in_with_history, Hit};
use ultar_desktop::model::Desktop;
use ultar_i18n::Lang;
use ultar_store::settings::AppUpdateSettings;

/// A signed catalogue the shell can read (one app, km-office, not installed).
const FIXTURE: &str = include_str!("store_fixture.toml");

/// Search page: the Store source offers catalogue apps that are not installed, read from a
/// signature-checked cache through the shell's own code; switching it off removes them, on
/// offers them again. The cache is signed here exactly as the Store publishes it.
pub(crate) fn store_search(
    bus: &Client,
    admin: &Client,
    run: &Run,
    shell: &Follower,
) -> Result<(), String> {
    let signing = SigningKey::from_bytes(&[31u8; 32]);
    let base = std::env::temp_dir().join(format!("settingstest-store-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let cache = base.join("cache");
    let keys = base.join("keys");
    std::fs::create_dir_all(&keys).map_err(|e| format!("keys dir: {e}"))?;
    std::fs::write(
        keys.join("v.ed25519"),
        hex::encode(signing.verifying_key().to_bytes()) + "\n",
    )
    .map_err(|e| format!("key file: {e}"))?;
    let blob = sign_catalog(FIXTURE.as_bytes(), &signing);
    FileCatalogSource::write_signed(&cache, FIXTURE, &blob).map_err(|e| format!("cache: {e:?}"))?;
    let catalog =
        ultar_desktop::model::store::load(&cache, &keys).ok_or("the cache did not verify")?;
    let mut desk = Desktop::new(Lang::En);
    desk.overview.store = Some(catalog);
    let tr = ultar_desktop::i18n::translator(Lang::En).map_err(|e| format!("{e:?}"))?;
    let apps = ultar_desktop::model::apps::base_apps();
    let finds_store = |desk: &Desktop| {
        search_in_with_history(
            "office",
            &apps,
            &tr,
            desk.overview.sources,
            &Default::default(),
            desk.overview.store.as_ref(),
        )
        .iter()
        .any(|hit| matches!(hit, Hit::Store(_)))
    };
    if !finds_store(&desk) {
        return Err("the shell's search does not offer Store apps".into());
    }
    let mut app = page(bus, run, "search")?;
    let off = change((&mut app, admin), ("search", "search.source.store"), None)?;
    shell.told("search.source.store", &off)?;
    apply_setting(&mut desk, "search.source.store", &off);
    if finds_store(&desk) || desk.overview.sources.store {
        return Err("the shell still searches the Store after switching it off".into());
    }
    let on = change((&mut app, admin), ("search", "search.source.store"), None)?;
    shell.told("search.source.store", &on)?;
    apply_setting(&mut desk, "search.source.store", &on);
    if !finds_store(&desk) {
        return Err("the shell does not search the Store again after switching it on".into());
    }
    let _ = std::fs::remove_dir_all(&base);
    println!(
        "SETTINGS: search page -> shell: Store catalogue apps offered (verified cache), \
         off removes them, on restores them"
    );
    Ok(())
}

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
