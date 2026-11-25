//! Following the person's settings and the mounted drives at runtime.
//!
//! [`follow_settings`] reads the `updates.*` settings at start (until `settingsd` answers) and
//! then follows `settings.changed`, mapping them onto the policy under the administrator's
//! ceiling. [`follow_volumes`] watches `volumes.state` and, while `updates.offline_usb` is on,
//! offers an update found on a newly plugged-in drive (through `notifyd`, when notifying is on).
//! Both threads end when the bus connection ends.

use crate::bootctl::BootControl;
use crate::error::UpdateError;
use crate::notify::{usb_offer, PostNote};
use crate::policy::Policy;
use crate::service::{lock, Core};
use crate::settings::{policy_relevant, Ceiling, UpdateSettings, KEYS};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_bus::messages::settings::{Changed, Get};
use ultar_bus::messages::volumes::VolumesState;
use ultar_bus::Client;

/// How long a `settings.get` call may take.
const GET_TIMEOUT: Duration = Duration::from_secs(3);
/// Pause before reading the settings again when `settingsd` has not answered yet.
const READ_PAUSE: Duration = Duration::from_secs(1);

impl<B: BootControl> Core<B> {
    /// Stores a settings-derived policy (no caller: the service itself follows the settings).
    /// Waits for a running operation to finish, then stores through the engine – a staged
    /// bundle of another channel goes, as for [`Core::set_policy`].
    pub fn apply_policy(&self, policy: &Policy) -> Result<(), UpdateError> {
        let engine = lock(&self.engine);
        let changed_channel = engine.policy().channel != policy.channel;
        engine.set_policy(policy)?;
        if changed_channel {
            lock(&self.snapshot).available = None;
        }
        self.refresh(&engine, None);
        Ok(())
    }
}

/// Reads every followed setting once. True when `settingsd` answered them all (a missing value
/// keeps the default); false when a call failed, so the caller tries again.
pub fn read_initial(client: &Client, settings: &mut UpdateSettings) -> bool {
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

/// Maps the settings onto the stored policy (keeping its rate limit) under the ceiling.
fn apply<B: BootControl>(
    core: &Core<B>,
    settings: &UpdateSettings,
    ceiling: &Ceiling,
) -> Result<(), UpdateError> {
    let stored = core.policy();
    let base = Policy {
        automatic: stored.automatic,
        window: stored.window,
        channel: stored.channel,
        allow_metered: stored.allow_metered,
        rate_limit_kib: stored.rate_limit_kib,
    };
    core.apply_policy(&settings.policy(&base, ceiling))
}

/// Spawns the thread that follows the settings: reads them at start, applies them, and applies
/// every later change of a policy-relevant key. The shared settings are kept current for the
/// notification and drive-offer paths.
pub fn follow_settings<B: BootControl + 'static>(
    core: Arc<Core<B>>,
    client: Client,
    ceiling: Ceiling,
    shared: Arc<Mutex<UpdateSettings>>,
) {
    let _ = std::thread::Builder::new()
        .name("update-settings".into())
        .spawn(move || {
            // Subscribe before reading, so no change between the two is lost.
            let Ok(changes) = client.subscribe::<Changed>() else {
                return;
            };
            let mut settings = UpdateSettings::default();
            while client.is_connected() {
                if read_initial(&client, &mut settings) {
                    break;
                }
                std::thread::sleep(READ_PAUSE);
            }
            if !client.is_connected() {
                return;
            }
            *lock(&shared) = settings.clone();
            let _ = apply(&core, &settings, &ceiling);
            while let Ok(change) = changes.recv() {
                let relevant = {
                    let mut current = lock(&shared);
                    current.take(&change.key, &change.value) && policy_relevant(&change.key)
                };
                if relevant {
                    let snapshot = lock(&shared).clone();
                    let _ = apply(&core, &snapshot, &ceiling);
                }
            }
        });
}

/// True when the drive at `uri` lies inside one of the allowed `roots` and holds a bundle.
/// The path is resolved (links and `..` included) before it is checked, as for offline calls.
pub fn has_bundle(uri: &str, roots: &[PathBuf]) -> bool {
    let Some(path) = uri.strip_prefix("file://") else {
        return false;
    };
    let Ok(real) = std::fs::canonicalize(path) else {
        return false;
    };
    let allowed = roots
        .iter()
        .any(|root| std::fs::canonicalize(root).is_ok_and(|root| real.starts_with(root)));
    allowed && crate::source::bundles_in(&real).is_ok_and(|found| !found.is_empty())
}

/// Spawns the thread that watches the mounted drives and offers an update found on a new one
/// while `updates.offline_usb` is on (the offer is a notification, so it also needs
/// `updates.notify`). A drive is offered once, until it is unplugged.
pub fn follow_volumes(
    client: Client,
    roots: Vec<PathBuf>,
    settings: Arc<Mutex<UpdateSettings>>,
    poster: Arc<dyn PostNote>,
) {
    let _ = std::thread::Builder::new()
        .name("update-volumes".into())
        .spawn(move || {
            let Ok(state) = client.subscribe::<VolumesState>() else {
                return;
            };
            let mut seen: Vec<String> = Vec::new();
            while let Ok(volumes) = state.recv() {
                seen.retain(|id| volumes.mounted.iter().any(|m| &m.id == id));
                let (offline, notify) = {
                    let s = lock(&settings);
                    (s.offline_usb, s.notify)
                };
                if !offline {
                    continue;
                }
                for drive in &volumes.mounted {
                    if seen.contains(&drive.id) {
                        continue;
                    }
                    seen.push(drive.id.clone());
                    if notify && has_bundle(&drive.uri, &roots) {
                        poster.post(&usb_offer(&drive.name));
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn a_drive_offers_only_inside_the_roots_and_with_a_bundle() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("media");
        let stick = root.join("STICK");
        std::fs::create_dir_all(&stick).unwrap();
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        let uri = |p: &Path| format!("file://{}", p.display());
        let roots = std::slice::from_ref(&root);
        // No bundle yet.
        assert!(!has_bundle(&uri(&stick), roots));
        std::fs::write(stick.join("update.uup"), b"UUP1").unwrap();
        assert!(has_bundle(&uri(&stick), roots));
        // Outside the allowed root: refused even with a bundle.
        std::fs::write(outside.join("update.uup"), b"UUP1").unwrap();
        assert!(!has_bundle(&uri(&outside), roots));
        // A non-file URI and a missing path are refused.
        assert!(!has_bundle("https://example/x.uup", roots));
        assert!(!has_bundle(&uri(&root.join("gone")), roots));
    }
}
