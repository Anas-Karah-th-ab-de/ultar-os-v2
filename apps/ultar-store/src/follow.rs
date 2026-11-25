//! Following the person's settings and the network at runtime for automatic app updates
//! (`updates.auto_apps`). Mirrors `system/core/ultar-update/src/follow.rs`: read the followed
//! settings at start (until `settingsd` answers), then follow `settings.changed`; follow
//! `net.state` for the metered flag. Both threads end when the bus connection ends.
//!
//! The threads only keep the shared state current and send a tick on every relevant change;
//! the window loop decides with [`crate::settings::AppUpdateSettings::allowed_now`] and
//! [`crate::Store::auto_update`] (the model lives on the UI thread).

use crate::settings::{AppUpdateSettings, KEYS};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::net::NetState;
use ultar_bus::messages::settings::{Changed, Get};
use ultar_bus::Client;

/// How long a `settings.get` call may take.
const GET_TIMEOUT: Duration = Duration::from_secs(3);
/// Pause before reading the settings again when `settingsd` has not answered yet.
const READ_PAUSE: Duration = Duration::from_secs(1);

/// The shared app-update settings (kept current by [`follow_settings`]).
pub type SharedSettings = Arc<Mutex<AppUpdateSettings>>;
/// The shared metered flag (`None` = unknown until the network service reports).
pub type SharedMetered = Arc<Mutex<Option<bool>>>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poison| poison.into_inner())
}

/// Reads every followed setting once. True when `settingsd` answered them all (a missing
/// value keeps the default); false when a call failed, so the caller tries again.
pub fn read_initial(client: &Client, settings: &mut AppUpdateSettings) -> bool {
    for key in KEYS {
        let call = Get {
            key: key.to_string(),
        };
        match client.call(&call, GET_TIMEOUT) {
            Ok(reply) => {
                if let Some(value) = reply.value {
                    settings.take(key, &value);
                }
            }
            Err(_) => return false,
        }
    }
    true
}

/// Spawns the thread that follows `updates.*`: reads them at start and takes every later
/// change, ticking the window loop so it rechecks whether automatic updates may run.
pub fn follow_settings(client: Client, settings: SharedSettings, tick: Sender<()>) {
    let _ = std::thread::Builder::new()
        .name("store-settings".into())
        .spawn(move || {
            // Subscribe before reading, so no change between the two is lost.
            let Ok(changes) = client.subscribe::<Changed>() else {
                return;
            };
            let mut current = AppUpdateSettings::default();
            while client.is_connected() {
                if read_initial(&client, &mut current) {
                    break;
                }
                std::thread::sleep(READ_PAUSE);
            }
            if !client.is_connected() {
                return;
            }
            *lock(&settings) = current.clone();
            let _ = tick.send(());
            while let Ok(change) = changes.recv() {
                let taken = lock(&settings).take(&change.key, &change.value);
                if taken {
                    let _ = tick.send(());
                }
            }
        });
}

/// Spawns the thread that follows `net.state`: keeps the metered flag current and ticks the
/// window loop when it changes (a metered connection blocks automatic updates unless allowed).
pub fn follow_net(client: Client, metered: SharedMetered, tick: Sender<()>) {
    let _ = std::thread::Builder::new()
        .name("store-net".into())
        .spawn(move || {
            let Ok(state) = client.subscribe::<NetState>() else {
                return;
            };
            while let Ok(net) = state.recv() {
                let changed = {
                    let mut m = lock(&metered);
                    let was = *m;
                    *m = Some(net.metered);
                    was != Some(net.metered)
                };
                if changed {
                    let _ = tick.send(());
                }
            }
        });
}
