//! Security alert of the Ultar account (docs/DEVICE-IDENTITY.md §5.7): when somebody who knows the
//! account password asks to move this account's number to another device, the owner must see it
//! and be able to stop it. Submodule of `app`.
//!
//! A background thread asks the server (`ultar_cloud::SecurityWatch`) at start, every
//! [`CHECK_EVERY`], and at once when the signalling socket delivers a notice. What it found is
//! shown by the phone app as a lasting banner with "This was not me"; a new pending move is also
//! posted as a system notification so that it reaches the user with the screen off.
//! The final look comes with the new desktop shell; the logic here is complete.

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use ultar_cloud::SecurityState;

/// Time between two checks without any other reason.
const CHECK_EVERY: Duration = Duration::from_secs(600);

/// What the banner shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Banner {
    /// A move of a number is pending: first characters of the device concerned, hours until it
    /// can happen, and whether this device may stop it ("This was not me").
    Pending {
        device: String,
        hours: i64,
        can_stop: bool,
    },
    /// The user stopped the move on this device.
    Stopped,
}

/// What the thread found last, and what the user did.
#[derive(Default)]
struct Shared {
    state: SecurityState,
    stopped: bool,
    /// A pending move was seen for the first time and not yet announced.
    fresh: bool,
    /// The texts drawn last (language, banner, texts): rendered once, not every frame.
    drawn: Option<(String, Banner, BannerText)>,
}

/// Decides what to show for a state. `stopped`: the user pressed "This was not me" and the
/// server confirmed. A pending move always wins: it is shown until the server no longer
/// reports it.
pub fn banner_for(state: &SecurityState, stopped: bool) -> Option<Banner> {
    match (&state.recovery, state.recovery_hours()) {
        (Some(recovery), Some(hours)) => Some(Banner::Pending {
            device: recovery.old_device.chars().take(8).collect(),
            hours,
            can_stop: state.manage,
        }),
        _ if stopped => Some(Banner::Stopped),
        _ => None,
    }
}

/// The texts of a banner in one language: the message, and the action (or the hint that takes
/// its place when this device may not stop the move).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BannerText {
    pub message: String,
    pub action: String,
    /// False: `action` is a hint, not a button.
    pub can_act: bool,
}

/// Renders a banner from the message catalogue of `ultar-cloud` (ar/en/de).
pub fn banner_text(lang: &str, banner: &Banner) -> BannerText {
    use ultar_cloud::messages::render;
    use ultar_cloud::watch::{
        KEY_CANNOT_CANCEL, KEY_NOT_ME, KEY_RECOVERY_CANCELLED, KEY_RECOVERY_PENDING,
    };
    match banner {
        Banner::Pending {
            device,
            hours,
            can_stop,
        } => {
            let args = [
                ("device", ultar_cloud::messages::Arg::Str(device)),
                ("hours", ultar_cloud::messages::Arg::Int(*hours)),
            ];
            let action = if *can_stop {
                KEY_NOT_ME
            } else {
                KEY_CANNOT_CANCEL
            };
            BannerText {
                message: render(lang, KEY_RECOVERY_PENDING, &args),
                action: render(lang, action, &[]),
                can_act: *can_stop,
            }
        }
        Banner::Stopped => BannerText {
            message: render(lang, KEY_RECOVERY_CANCELLED, &[]),
            action: String::new(),
            can_act: true,
        },
    }
}

/// Takes a new result into the shared data. Returns true when a pending move appeared that was
/// not there before (it has to be announced).
fn absorb(shared: &mut Shared, found: SecurityState) -> bool {
    let appeared = found.recovery.is_some() && shared.state.recovery.is_none();
    if found.recovery.is_some() {
        shared.stopped = false;
    }
    shared.state = found;
    shared.fresh |= appeared;
    appeared
}

enum Cmd {
    Check,
    NotMe,
}

/// The running watch of one device.
pub struct SecurityAlert {
    shared: Arc<Mutex<Shared>>,
    tx: Option<mpsc::Sender<Cmd>>,
}

impl Default for SecurityAlert {
    fn default() -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        // Only a registered device has something to watch.
        let tx = ultar_cloud::my_number().is_some().then(|| {
            let (tx, rx) = mpsc::channel();
            let for_thread = shared.clone();
            std::thread::spawn(move || run(rx, for_thread));
            tx
        });
        Self { shared, tx }
    }
}

fn run(rx: mpsc::Receiver<Cmd>, shared: Arc<Mutex<Shared>>) {
    let watch = ultar_cloud::SecurityWatch::new(ultar_cloud::DeviceHome::system());
    let mut cmd = Cmd::Check;
    loop {
        let client = ultar_cloud::client();
        let stopped_now = matches!(cmd, Cmd::NotMe) && watch.not_me(&client).is_ok();
        // Offline or refused: keep what is shown and try again at the next occasion.
        if let Ok(found) = watch.check(&client) {
            let _ = watch.acknowledge(&found);
            if let Ok(mut s) = shared.lock() {
                absorb(&mut s, found);
                s.stopped |= stopped_now;
            }
        }
        cmd = match rx.recv_timeout(CHECK_EVERY) {
            Ok(cmd) => cmd,
            Err(mpsc::RecvTimeoutError::Timeout) => Cmd::Check,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
    }
}

impl SecurityAlert {
    /// Ask the server now (the signalling socket delivered a notice).
    pub fn recheck(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Check);
        }
    }

    /// "This was not me": stop the pending move with this device's key.
    pub fn not_me(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::NotMe);
        }
    }

    /// What to show now, with its texts in `lang`.
    pub fn banner_with_text(&self, lang: &str) -> Option<(Banner, BannerText)> {
        let mut shared = self.shared.lock().ok()?;
        let banner = banner_for(&shared.state, shared.stopped)?;
        if let Some((l, b, text)) = &shared.drawn {
            if l == lang && *b == banner {
                return Some((banner, text.clone()));
            }
        }
        let text = banner_text(lang, &banner);
        shared.drawn = Some((lang.to_string(), banner.clone(), text.clone()));
        Some((banner, text))
    }

    /// True once for every pending move that appeared (to post the system notification).
    pub fn take_fresh(&self) -> bool {
        self.shared
            .lock()
            .map(|mut s| std::mem::take(&mut s.fresh))
            .unwrap_or(false)
    }

    /// The user closed the "stopped" confirmation.
    pub fn dismiss(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.stopped = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ultar_cloud::RecoveryState;

    fn pending(eligible_in: i64, manage: bool) -> SecurityState {
        SecurityState {
            recovery: Some(RecoveryState {
                old_device: "a".repeat(32),
                eligible_in,
            }),
            notices: Vec::new(),
            manage,
        }
    }

    #[test]
    fn a_pending_move_is_shown_until_the_server_no_longer_reports_it() {
        assert_eq!(banner_for(&SecurityState::default(), false), None);
        assert_eq!(
            banner_for(&pending(71 * 3600 + 1, true), false),
            Some(Banner::Pending {
                device: "aaaaaaaa".into(),
                hours: 72,
                can_stop: true
            })
        );
        // Even after "stopped" was shown: a move that is (again) pending wins.
        assert_eq!(
            banner_for(&pending(0, false), true),
            Some(Banner::Pending {
                device: "aaaaaaaa".into(),
                hours: 0,
                can_stop: false
            })
        );
        assert_eq!(
            banner_for(&SecurityState::default(), true),
            Some(Banner::Stopped)
        );
    }

    #[test]
    fn a_new_pending_move_is_announced_once_and_a_second_one_again() {
        let mut shared = Shared::default();
        assert!(absorb(&mut shared, pending(100, true)));
        assert!(std::mem::take(&mut shared.fresh));
        assert!(!absorb(&mut shared, pending(50, true)), "the same move");
        assert!(!shared.fresh);
        // It is over (stopped by the user on this device) …
        shared.stopped = true;
        assert!(!absorb(&mut shared, SecurityState::default()));
        assert_eq!(
            banner_for(&shared.state, shared.stopped),
            Some(Banner::Stopped)
        );
        // … and somebody starts another one: announced again, "stopped" is gone.
        assert!(absorb(&mut shared, pending(100, true)));
        assert!(shared.fresh && !shared.stopped);
    }

    #[test]
    fn the_banner_has_texts_in_the_three_languages_and_a_button_only_when_it_can_act() {
        let alert = SecurityAlert {
            shared: Arc::new(Mutex::new(Shared::default())),
            tx: None,
        };
        assert_eq!(alert.banner_with_text("en"), None);
        absorb(&mut alert.shared.lock().unwrap(), pending(5 * 3600, true));
        let (banner, en) = alert.banner_with_text("en").unwrap();
        assert!(matches!(banner, Banner::Pending { can_stop: true, .. }));
        assert!(en.message.contains("aaaaaaaa") && en.message.contains("5 hours"));
        assert!(en.can_act && en.action.contains("not me"));
        assert_eq!(alert.banner_with_text("en").unwrap().1, en, "cached");
        let de = alert.banner_with_text("de").unwrap().1;
        let ar = alert.banner_with_text("ar").unwrap().1;
        assert!(de.message.contains("Kennwort") && ar.message.contains("كلمة مرورك"));
        // A device that was added recently cannot stop it: a hint in place of the button.
        absorb(&mut alert.shared.lock().unwrap(), pending(3600, false));
        let hint = alert.banner_with_text("en").unwrap().1;
        assert!(!hint.can_act && hint.action.contains("other devices"));
        let stopped = banner_text("en", &Banner::Stopped);
        assert!(stopped.can_act && stopped.message.contains("password"));
    }

    #[test]
    fn without_a_registered_device_nothing_runs_and_nothing_is_shown() {
        let alert = SecurityAlert {
            shared: Arc::new(Mutex::new(Shared::default())),
            tx: None,
        };
        alert.recheck();
        alert.not_me();
        alert.dismiss();
        assert_eq!(alert.banner_with_text("en"), None);
        assert!(!alert.take_fresh());
    }
}
