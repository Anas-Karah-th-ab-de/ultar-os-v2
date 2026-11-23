//! What the user must be told about the account's devices (`docs/DEVICE-IDENTITY.md` §5.7, §5.8).
//!
//! A replacement of a lost device needs only the account password, a waiting time – and the
//! owner not objecting. So the owner has to *see* it: [`SecurityWatch::check`] reads from the
//! server whether a replacement is pending (a state that is reported for as long as it lasts,
//! not a message that can be missed) and which security notices are new; the user interface
//! shows a lasting alert with "this was not me", which calls [`SecurityWatch::not_me`].
//! The interface calls `check` at start, periodically, and when the signalling socket delivers
//! a notice.

use crate::client::Client;
use crate::client_device::{AddedDevice, DeviceEvent, RecoveryState};
use crate::device::DeviceHome;
use crate::error::CloudError;
use crate::http::write_atomic;
use std::path::PathBuf;

/// Message key of the lasting alert for a pending replacement (`{device}`, `{hours}`).
pub const KEY_RECOVERY_PENDING: &str = "cloud.alert.recovery_pending";
/// Message key of the lasting alert for a device that was added and may not manage yet
/// (`{device}`, `{hours}`).
pub const KEY_DEVICE_ADDED: &str = "cloud.alert.device_added_pending";
/// Message key of the action that removes such a device.
pub const KEY_REMOVE_IT: &str = "cloud.alert.remove_it";
/// Message key of the confirmation after such a device was removed.
pub const KEY_DEVICE_REMOVED: &str = "cloud.alert.device_removed_done";
/// Message key of the title of a security alert.
pub const KEY_ALERT_TITLE: &str = "cloud.alert.title";
/// Message key of the hint for a device that may not cancel (added recently).
pub const KEY_CANNOT_CANCEL: &str = "cloud.alert.cannot_cancel";
/// Message key of the action that cancels a pending replacement.
pub const KEY_NOT_ME: &str = "cloud.alert.not_me";
/// Message key of the confirmation after a cancelled replacement.
pub const KEY_RECOVERY_CANCELLED: &str = "cloud.alert.recovery_cancelled";

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SecurityState {
    /// A replacement of one of the account's devices that is waiting; show it until it is gone.
    pub recovery: Option<RecoveryState>,
    /// Security notices the user has not acknowledged yet, oldest first.
    pub notices: Vec<DeviceEvent>,
    /// True when this device may manage the account's devices (and so cancel a replacement).
    pub manage: bool,
    /// Devices added to the account that may not manage it yet; show them until that is over.
    pub unapproved: Vec<AddedDevice>,
}

impl SecurityState {
    /// True when there is something to show.
    pub fn needs_attention(&self) -> bool {
        self.recovery.is_some() || !self.unapproved.is_empty() || !self.notices.is_empty()
    }

    /// Whole hours (rounded up) until a pending replacement can be finished.
    pub fn recovery_hours(&self) -> Option<i64> {
        self.recovery
            .as_ref()
            .map(|r| (r.eligible_in.max(0) + 3599) / 3600)
    }
}

/// The message key for a security notice of the server; `None` for names this version does not
/// know (they are not shown as raw text).
pub fn notice_key(event: &str) -> Option<&'static str> {
    Some(match event {
        "device_added" => "cloud.alert.event.device_added",
        "device_removed" => "cloud.alert.event.device_removed",
        "device_replaced" => "cloud.alert.event.device_replaced",
        "device_upgraded" => "cloud.alert.event.device_upgraded",
        "device_approved" => "cloud.alert.event.device_approved",
        "recovery_started" => "cloud.alert.event.recovery_started",
        "recovery_cancelled" => "cloud.alert.event.recovery_cancelled",
        "recovery_finished" => "cloud.alert.event.recovery_finished",
        "password_changed" => "cloud.alert.event.password_changed",
        _ => return None,
    })
}

/// Reads the account's security state for one device and remembers what the user has seen.
#[derive(Debug, Clone)]
pub struct SecurityWatch {
    home: DeviceHome,
    seen_file: PathBuf,
}

impl SecurityWatch {
    /// A watch for the device whose state lives in `home`.
    pub fn new(home: DeviceHome) -> Self {
        let seen_file = home.path("events.seen");
        Self { home, seen_file }
    }

    /// Id of the newest notice the user acknowledged (0 = none).
    pub fn acknowledged(&self) -> i64 {
        std::fs::read_to_string(&self.seen_file)
            .ok()
            .and_then(|t| t.trim().parse::<i64>().ok())
            .filter(|id| *id >= 0)
            .unwrap_or(0)
    }

    /// Asks the server, with this device's key: is a replacement pending, and which notices are
    /// new? Works for every registered device; notices are only given to managing devices.
    pub fn check(&self, client: &Client) -> Result<SecurityState, CloudError> {
        let key = self.home.key_store()?;
        let session = client.device_session(&key)?;
        let notices = if session.manage {
            Client::new(client.base_url(), Some(session.token)).events_after(self.acknowledged())?
        } else {
            Vec::new()
        };
        Ok(SecurityState {
            recovery: session.recovery,
            notices,
            manage: session.manage,
            unapproved: session.unapproved,
        })
    }

    /// "This was not me" for an added device: removes it with this device's key (this device
    /// must be a managing device).
    pub fn remove_added(&self, client: &Client, device_id: &str) -> Result<(), CloudError> {
        client.remove_device(&self.home.key_store()?, device_id)
    }

    /// The user has seen the notices of `state`: they are not reported again. A pending
    /// replacement keeps being reported until it is over.
    pub fn acknowledge(&self, state: &SecurityState) -> Result<(), CloudError> {
        let newest = state.notices.iter().map(|n| n.id).max().unwrap_or(0);
        if newest > self.acknowledged() {
            write_atomic(&self.seen_file, newest.to_string().as_bytes(), true)?;
        }
        Ok(())
    }

    /// "This was not me": cancels the pending replacement with this device's key.
    pub fn not_me(&self, client: &Client) -> Result<(), CloudError> {
        client.recover_cancel(&self.home.key_store()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(id: i64) -> DeviceEvent {
        DeviceEvent {
            id,
            event: "device_added".into(),
            device: "abcdefgh".into(),
            at: 0,
        }
    }

    #[test]
    fn acknowledged_notices_are_remembered_and_never_move_back() {
        let dir = tempfile::tempdir().unwrap();
        let watch = SecurityWatch::new(DeviceHome::new(dir.path().join("ultar")));
        assert_eq!(watch.acknowledged(), 0);
        let mut state = SecurityState {
            notices: vec![notice(3), notice(7)],
            ..SecurityState::default()
        };
        watch.acknowledge(&state).unwrap();
        assert_eq!(watch.acknowledged(), 7);
        state.notices = vec![notice(5)];
        watch.acknowledge(&state).unwrap();
        assert_eq!(watch.acknowledged(), 7);
        watch.acknowledge(&SecurityState::default()).unwrap();
        assert_eq!(watch.acknowledged(), 7);
        for damaged in ["", "x", "-4", "7 7"] {
            std::fs::write(dir.path().join("ultar/events.seen"), damaged).unwrap();
            assert_eq!(
                watch.acknowledged(),
                0,
                "{damaged:?}: everything is shown again"
            );
        }
    }

    #[test]
    fn a_pending_replacement_always_needs_attention() {
        let mut state = SecurityState::default();
        assert!(!state.needs_attention());
        assert_eq!(state.recovery_hours(), None);
        state.recovery = Some(RecoveryState {
            old_device: "a".repeat(32),
            eligible_in: 3601,
        });
        assert!(state.needs_attention());
        assert_eq!(state.recovery_hours(), Some(2));
        state.recovery = Some(RecoveryState {
            old_device: "a".repeat(32),
            eligible_in: 0,
        });
        assert_eq!(state.recovery_hours(), Some(0));
        let added = SecurityState {
            unapproved: vec![AddedDevice {
                device: "b".repeat(32),
                manage_in: 10,
            }],
            ..SecurityState::default()
        };
        assert!(
            added.needs_attention(),
            "an added device is pinned like a recovery"
        );
        let only_notice = SecurityState {
            notices: vec![notice(1)],
            ..SecurityState::default()
        };
        assert!(only_notice.needs_attention());
    }

    #[test]
    fn every_server_notice_has_a_text_in_every_language() {
        let events = [
            "device_added",
            "device_removed",
            "device_replaced",
            "device_upgraded",
            "device_approved",
            "recovery_started",
            "recovery_cancelled",
            "recovery_finished",
            "password_changed",
        ];
        for lang in ultar_i18n::Lang::ALL {
            let mut tr = ultar_i18n::Translator::empty(lang);
            crate::messages::register(&mut tr).unwrap();
            let keys = events.iter().filter_map(|e| notice_key(e));
            let fixed = [
                KEY_RECOVERY_PENDING,
                KEY_NOT_ME,
                KEY_RECOVERY_CANCELLED,
                KEY_ALERT_TITLE,
                KEY_CANNOT_CANCEL,
                KEY_DEVICE_ADDED,
                KEY_REMOVE_IT,
                KEY_DEVICE_REMOVED,
            ];
            for key in keys.chain(fixed) {
                assert!(tr.catalog(lang).contains(key), "{key} missing in {lang:?}");
            }
        }
        assert_eq!(events.iter().filter_map(|e| notice_key(e)).count(), 9);
        assert_eq!(notice_key("something_new"), None);
    }

    #[test]
    fn checking_needs_a_usable_key_and_a_reachable_server() {
        let dir = tempfile::tempdir().unwrap();
        let watch = SecurityWatch::new(DeviceHome::new(dir.path()));
        let nowhere = Client::new("http://127.0.0.1:9", None);
        assert!(matches!(
            watch.check(&nowhere),
            Err(CloudError::Transport(_))
        ));
        assert!(matches!(
            watch.not_me(&nowhere),
            Err(CloudError::Transport(_))
        ));
        assert!(matches!(
            watch.remove_added(&nowhere, "x"),
            Err(CloudError::Transport(_))
        ));
    }
}
